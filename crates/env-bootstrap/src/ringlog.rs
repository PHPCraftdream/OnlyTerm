//! This module sets up a logger that captures recent log entries
//! into an in-memory ring-buffer, as well as passed them on to
//! a pretty logger on stderr.
//! This allows other code to collect the ring buffer and display it
//! within the application.
use chrono::prelude::*;
use env_logger::filter::{Builder as FilterBuilder, Filter};
use log::{Level, LevelFilter, Record};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TryRecvError, TrySendError};
use std::sync::{mpsc, Mutex};
use std::thread::JoinHandle;
use termwiz::istty::IsTty;

const ASYNC_QUEUE_CAPACITY: usize = 256;
const MAX_QUEUED_LINE_BYTES: usize = 64 * 1024;

fn use_background_output(is_gui: bool, max_level: LevelFilter) -> bool {
    // Explicit diagnostics need their tail written before returning to callers.
    is_gui && max_level == LevelFilter::Info
}

lazy_static::lazy_static! {
    static ref RINGS: Mutex<Rings> = Mutex::new(Rings::new());
}

#[derive(Debug, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct Entry {
    pub then: DateTime<Local>,
    pub level: Level,
    pub target: String,
    pub msg: String,
}

struct LevelRing {
    entries: Vec<Entry>,
    first: usize,
    last: usize,
    len: usize,
}

impl LevelRing {
    fn new(level: Level) -> Self {
        let mut entries = vec![];
        let now = Local::now();
        for _ in 0..16 {
            entries.push(Entry {
                then: now,
                level,
                target: String::new(),
                msg: String::new(),
            });
        }
        Self {
            entries,
            first: 0,
            last: 0,
            len: 0,
        }
    }

    // Returns the number of entries in the ring
    fn len(&self) -> usize {
        self.len
    }

    fn rolling_inc(&self, value: usize) -> usize {
        let incremented = value + 1;
        if incremented >= self.entries.len() {
            0
        } else {
            incremented
        }
    }

    fn push(&mut self, entry: Entry) {
        if self.len() == self.entries.len() {
            // We are full; effectively pop the first entry to
            // make room
            self.entries[self.first] = entry;
            self.first = self.rolling_inc(self.first);
        } else {
            self.entries[self.last] = entry;
            self.len += 1;
        }
        self.last = self.rolling_inc(self.last);
    }

    fn append_to_vec(&self, target: &mut Vec<Entry>) {
        if self.len == 0 {
            return;
        }
        if self.first < self.last {
            target.extend_from_slice(&self.entries[self.first..self.last]);
        } else {
            target.extend_from_slice(&self.entries[self.first..]);
            target.extend_from_slice(&self.entries[..self.last]);
        }
    }
}

struct Rings {
    rings: HashMap<Level, LevelRing>,
}

impl Rings {
    fn new() -> Self {
        let mut rings = HashMap::new();
        for level in &[
            Level::Error,
            Level::Warn,
            Level::Info,
            Level::Debug,
            Level::Trace,
        ] {
            rings.insert(*level, LevelRing::new(*level));
        }
        Self { rings }
    }

    fn get_entries(&self) -> Vec<Entry> {
        let mut results = vec![];
        for ring in self.rings.values() {
            ring.append_to_vec(&mut results);
        }
        results
    }

    fn log(&mut self, entry: Entry) {
        if let Some(ring) = self.rings.get_mut(&entry.level) {
            ring.push(entry);
        }
    }
}

#[derive(Clone)]
struct FormattedRecord {
    stderr: String,
    file: String,
}

enum AsyncMessage {
    Record(FormattedRecord),
    Flush(mpsc::Sender<()>),
    Shutdown(mpsc::Sender<()>),
}

struct AsyncOutput {
    tx: SyncSender<AsyncMessage>,
    join: Mutex<Option<JoinHandle<()>>>,
}

impl AsyncOutput {
    fn new(file_name: PathBuf) -> Option<Self> {
        let (tx, rx) = sync_channel(ASYNC_QUEUE_CAPACITY);
        let join = std::thread::Builder::new()
            .name("onlyterm-diagnostic-log".to_string())
            .spawn(move || async_output_worker(rx, file_name))
            .ok()?;
        Some(Self {
            tx,
            join: Mutex::new(Some(join)),
        })
    }

    fn send(&self, record: FormattedRecord) -> Result<(), FormattedRecord> {
        match self.tx.try_send(AsyncMessage::Record(record)) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(AsyncMessage::Record(record)))
            | Err(TrySendError::Disconnected(AsyncMessage::Record(record))) => Err(record),
            Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {
                unreachable!("only Record messages are sent through AsyncOutput::send")
            }
        }
    }

    fn flush_before_direct(&self, result: Result<(), FormattedRecord>) -> Option<FormattedRecord> {
        let record = result.err()?;
        self.flush();
        Some(record)
    }

    fn flush(&self) {
        let (tx, rx) = mpsc::channel();
        if self.tx.send(AsyncMessage::Flush(tx)).is_ok() {
            let _ = rx.recv();
        }
    }

    fn shutdown(&self) {
        let (tx, rx) = mpsc::channel();
        if self.tx.send(AsyncMessage::Shutdown(tx)).is_ok() {
            let _ = rx.recv();
        }
        if let Some(join) = self.join.lock().unwrap().take() {
            let _ = join.join();
        }
    }
}

impl Drop for AsyncOutput {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn async_output_worker(rx: Receiver<AsyncMessage>, file_name: PathBuf) {
    let mut file: Option<BufWriter<File>> = None;
    let mut stderr = std::io::stderr();
    loop {
        let first = match rx.recv_timeout(std::time::Duration::from_millis(50)) {
            Ok(message) => message,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                flush_outputs(&mut file, &mut stderr);
                continue;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };

        let mut should_exit = false;
        process_async_message(first, &file_name, &mut file, &mut stderr, &mut should_exit);
        while !should_exit {
            match rx.try_recv() {
                Ok(message) => {
                    process_async_message(
                        message,
                        &file_name,
                        &mut file,
                        &mut stderr,
                        &mut should_exit,
                    );
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    should_exit = true;
                }
            }
        }
        flush_outputs(&mut file, &mut stderr);
        if should_exit {
            break;
        }
    }
    flush_outputs(&mut file, &mut stderr);
}

fn process_async_message(
    message: AsyncMessage,
    file_name: &PathBuf,
    file: &mut Option<BufWriter<File>>,
    stderr: &mut std::io::Stderr,
    should_exit: &mut bool,
) {
    match message {
        AsyncMessage::Record(record) => {
            let _ = stderr.write_all(record.stderr.as_bytes());
            if file.is_none() {
                if let Ok(handle) = std::fs::OpenOptions::new()
                    .append(true)
                    .create(true)
                    .open(file_name)
                {
                    file.replace(BufWriter::new(handle));
                }
            }
            if let Some(file) = file.as_mut() {
                let _ = file.write_all(record.file.as_bytes());
            }
        }
        AsyncMessage::Flush(done) => {
            flush_outputs(file, stderr);
            let _ = done.send(());
        }
        AsyncMessage::Shutdown(done) => {
            flush_outputs(file, stderr);
            let _ = done.send(());
            *should_exit = true;
        }
    }
}

fn flush_outputs(file: &mut Option<BufWriter<File>>, stderr: &mut std::io::Stderr) {
    if let Some(file) = file.as_mut() {
        let _ = file.flush();
    }
    let _ = stderr.flush();
}

struct Logger {
    file_name: PathBuf,
    file: Mutex<Option<BufWriter<File>>>,
    async_output: Option<AsyncOutput>,
    filter: Filter,
    padding: AtomicUsize,
    is_tty: bool,
}

impl Drop for Logger {
    fn drop(&mut self) {
        if let Some(async_output) = self.async_output.take() {
            async_output.shutdown();
        }
        self.flush_direct();
    }
}

impl log::Log for Logger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        self.filter.enabled(metadata)
    }

    fn flush(&self) {
        if let Some(async_output) = &self.async_output {
            async_output.flush();
        }
        self.flush_direct();
    }

    fn log(&self, record: &Record) {
        if self.filter.matches(record) {
            let entry = Entry {
                then: Local::now(),
                level: record.level(),
                target: record.target().to_string(),
                msg: record.args().to_string(),
            };
            let ts = entry.then.format("%H:%M:%S%.3f").to_string();
            let level = record.level().as_str();

            let padding = self.padding.fetch_max(entry.target.len(), Ordering::SeqCst);

            let level_color = if self.is_tty {
                match record.level() {
                    Level::Error => "\u{1b}[31m",
                    Level::Warn => "\u{1b}[33m",
                    Level::Info => "\u{1b}[32m",
                    Level::Debug => "\u{1b}[36m",
                    Level::Trace => "\u{1b}[35m",
                }
            } else {
                ""
            };

            let reset = if self.is_tty { "\u{1b}[0m" } else { "" };
            let target_color = if self.is_tty { "\u{1b}[1m" } else { "" };

            let output = FormattedRecord {
                stderr: format!(
                    "{}  {level_color}{:6}{reset} {target_color}{:padding$}{reset} > {}\n",
                    ts,
                    level,
                    entry.target,
                    entry.msg,
                    padding = padding,
                    level_color = level_color,
                    reset = reset,
                    target_color = target_color
                ),
                file: format!(
                    "{}  {:6} {:padding$} > {}\n",
                    ts,
                    level,
                    entry.target,
                    entry.msg,
                    padding = padding
                ),
            };

            let critical = matches!(record.level(), Level::Error | Level::Warn);
            let direct_output = if let Some(async_output) = &self.async_output {
                if critical {
                    async_output.flush();
                    Some(output)
                } else {
                    async_output.flush_before_direct(async_output.send(limit_queued_record(output)))
                }
            } else {
                Some(output)
            };
            if let Some(output) = direct_output {
                self.write_direct(output);
            }

            // Move the already-formatted strings into the ring after the
            // output paths have borrowed them. This avoids formatting the
            // target and message a second time just for the in-memory log.
            RINGS.lock().unwrap().log(entry);
        }
    }
}

impl Logger {
    fn flush_direct(&self) {
        if let Some(file) = self.file.lock().unwrap().as_mut() {
            let _ = file.flush();
        }
        let _ = std::io::stderr().flush();
    }

    fn write_direct(&self, output: FormattedRecord) {
        // We use write_all rather than eprintln! so a redirected stderr or
        // file failure is ignored without panicking from inside log().
        let mut stderr = std::io::stderr();
        let _ = stderr.write_all(output.stderr.as_bytes());
        let _ = stderr.flush();
        let mut file = self.file.lock().unwrap();
        if file.is_none() {
            if let Ok(handle) = std::fs::OpenOptions::new()
                .append(true)
                .create(true)
                .open(&self.file_name)
            {
                file.replace(BufWriter::new(handle));
            }
        }
        if let Some(file) = file.as_mut() {
            let _ = file.write_all(output.file.as_bytes());
            let _ = file.flush();
        }
    }
}

fn limit_queued_record(mut record: FormattedRecord) -> FormattedRecord {
    truncate_utf8(&mut record.stderr, MAX_QUEUED_LINE_BYTES);
    truncate_utf8(&mut record.file, MAX_QUEUED_LINE_BYTES);
    record
}

fn truncate_utf8(value: &mut String, max_bytes: usize) {
    if value.len() > max_bytes {
        const SUFFIX: &str = "...[truncated]\n";
        let mut cut = max_bytes.saturating_sub(SUFFIX.len()).min(value.len());
        while cut > 0 && !value.is_char_boundary(cut) {
            cut -= 1;
        }
        value.truncate(cut);
        value.push_str(SUFFIX);
    }
}

/// Returns the current set of log information, sorted by time
pub fn get_entries() -> Vec<Entry> {
    let mut entries = RINGS.lock().unwrap().get_entries();
    entries.sort();
    entries
}

fn prune_old_logs() {
    let one_week = std::time::Duration::from_secs(86400 * 7);
    if let Ok(dir) = std::fs::read_dir(&*config::RUNTIME_DIR) {
        for entry in dir.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                if name.contains("-log-") {
                    if let Ok(meta) = entry.metadata() {
                        if let Ok(modified) = meta.modified() {
                            if let Ok(elapsed) = modified.elapsed() {
                                if elapsed > one_week {
                                    let _ = std::fs::remove_file(entry.path());
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn setup_pretty() -> (LevelFilter, Logger) {
    let base_name = std::env::current_exe()
        .ok()
        .and_then(|p| p.file_name().map(|s| s.to_string_lossy().to_string()))
        .unwrap_or_else(|| "onlyterm".to_string());

    if base_name.contains("gui") {
        // Only tidy up logs when the gui process is starting.
        // rationale: `onlyterm cli` commands should have as low startup
        // overhead as possible
        prune_old_logs();
    }

    let log_file_name = config::RUNTIME_DIR.join(format!(
        "{}-log-{}.txt",
        base_name,
        // SAFETY: `getpid` has no preconditions and no UB.
        unsafe { libc::getpid() }
    ));

    let mut filters = FilterBuilder::new();
    for (module, level) in [
        ("wgpu_core", LevelFilter::Error),
        ("wgpu_hal", LevelFilter::Error),
        ("gfx_backend_metal", LevelFilter::Error),
        ("tracing", LevelFilter::Error),
        ("zbus", LevelFilter::Error),
    ] {
        filters.filter_module(module, level);
    }

    if let Ok(s) = std::env::var("ONLYTERM_LOG") {
        filters.parse(&s);
    } else {
        filters.filter_level(LevelFilter::Info);
    }
    let filter = filters.build();
    let max_level = filter.filter();

    let async_output = if use_background_output(base_name.contains("gui"), max_level) {
        AsyncOutput::new(log_file_name.clone())
    } else {
        None
    };

    (
        max_level,
        Logger {
            file_name: log_file_name,
            file: Mutex::new(None),
            async_output,
            filter,
            padding: AtomicUsize::new(0),
            is_tty: std::io::stderr().is_tty(),
        },
    )
}

pub fn setup_logger() {
    let (max_level, logger) = setup_pretty();
    if log::set_boxed_logger(Box::new(logger)).is_ok() {
        log::set_max_level(max_level);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(index: usize) -> Entry {
        Entry {
            then: Local::now(),
            level: Level::Info,
            target: "test".to_string(),
            msg: index.to_string(),
        }
    }

    fn messages(ring: &LevelRing) -> Vec<String> {
        let mut entries = Vec::new();
        ring.append_to_vec(&mut entries);
        entries.into_iter().map(|entry| entry.msg).collect()
    }

    #[test]
    fn level_ring_keeps_exactly_sixteen_entries_before_wrapping() {
        let mut ring = LevelRing::new(Level::Info);
        for index in 0..16 {
            ring.push(entry(index));
        }

        assert_eq!(ring.len(), 16);
        assert_eq!(
            messages(&ring),
            (0..16).map(|i| i.to_string()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn level_ring_retains_the_latest_entries_after_wrapping() {
        let mut ring = LevelRing::new(Level::Info);
        for index in 0..17 {
            ring.push(entry(index));
        }

        assert_eq!(ring.len(), 16);
        assert_eq!(
            messages(&ring),
            (1..17).map(|i| i.to_string()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn level_ring_stays_bounded_for_long_runs() {
        let mut ring = LevelRing::new(Level::Info);
        for index in 0..128 {
            ring.push(entry(index));
        }

        assert_eq!(ring.len(), 16);
        assert_eq!(
            messages(&ring),
            (112..128).map(|i| i.to_string()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn queued_records_are_bounded_and_flushable() {
        let long = FormattedRecord {
            stderr: String::new(),
            file: "x".repeat(MAX_QUEUED_LINE_BYTES + 32),
        };
        let limited = limit_queued_record(long);
        assert!(limited.file.len() <= MAX_QUEUED_LINE_BYTES);
        assert!(limited.file.ends_with("...[truncated]\n"));
    }

    #[test]
    fn queued_utf8_records_truncate_at_a_character_boundary() {
        let mut value = "界".repeat(32);
        truncate_utf8(&mut value, 17);
        assert!(value.is_char_boundary(value.len()));
        assert!(value.ends_with("...[truncated]\n"));
        assert!(value.len() <= 17);
    }

    #[test]
    fn bounded_queue_reports_backpressure_without_blocking() {
        let (tx, _rx) = sync_channel(1);
        let record = || {
            AsyncMessage::Record(FormattedRecord {
                stderr: String::new(),
                file: "record\n".to_string(),
            })
        };
        assert!(tx.try_send(record()).is_ok());
        assert!(matches!(tx.try_send(record()), Err(TrySendError::Full(_))));
    }

    #[test]
    fn async_output_flush_barrier_writes_short_lived_logs() {
        let path =
            std::env::temp_dir().join(format!("onlyterm-ringlog-{}.log", std::process::id()));
        let output = AsyncOutput::new(path.clone()).unwrap();
        assert!(output
            .send(FormattedRecord {
                stderr: String::new(),
                file: "startup\n".to_string(),
            })
            .is_ok());
        output.flush();
        output.shutdown();
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "startup\n");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn explicit_diagnostics_are_synchronous() {
        assert!(use_background_output(true, LevelFilter::Info));
        for level in [
            LevelFilter::Off,
            LevelFilter::Error,
            LevelFilter::Warn,
            LevelFilter::Debug,
            LevelFilter::Trace,
        ] {
            assert!(!use_background_output(true, level));
        }
        assert!(!use_background_output(false, LevelFilter::Info));
    }

    #[test]
    fn queue_overflow_flushes_earlier_records_before_direct_fallback() {
        let (tx, rx) = sync_channel(1);
        let output = AsyncOutput {
            tx,
            join: Mutex::new(None),
        };
        assert!(output
            .send(FormattedRecord {
                stderr: String::new(),
                file: "first".into()
            })
            .is_ok());
        let unsent = output.send(FormattedRecord {
            stderr: String::new(),
            file: "second".into(),
        });
        assert!(unsent.is_err());
        let worker = std::thread::spawn(move || {
            match rx.recv().unwrap() {
                AsyncMessage::Record(record) => assert_eq!(record.file, "first"),
                _ => panic!("expected earlier record"),
            }
            match rx.recv().unwrap() {
                AsyncMessage::Flush(done) => done.send(()).unwrap(),
                _ => panic!("expected flush barrier before direct fallback"),
            }
        });
        assert_eq!(output.flush_before_direct(unsent).unwrap().file, "second");
        worker.join().unwrap();
    }

    #[test]
    fn async_output_ignores_file_open_failures_and_still_flushes() {
        let path = std::env::temp_dir()
            .join(format!("onlyterm-ringlog-missing-{}", std::process::id()))
            .join("log.txt");
        let output = AsyncOutput::new(path).unwrap();
        assert!(output
            .send(FormattedRecord {
                stderr: String::new(),
                file: "diagnostic\n".to_string(),
            })
            .is_ok());
        output.flush();
        output.shutdown();
    }
}
