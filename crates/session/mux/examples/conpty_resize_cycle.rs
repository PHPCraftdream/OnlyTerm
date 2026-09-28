//! Investigation harness for "after returning to a tab, input lands N rows
//! away from the CMD prompt". Display power cycles and similar events resize
//! every OnlyTerm window; with cmd's `color` fill in the buffer, ConPTY's own
//! reflow moves rows that the terminal model must move identically, because
//! ConPTY never repaints on resize and places later edits absolutely.
//!
//! This replays resize sequences against a real ConPTY + cmd.exe, feeding
//! its output through `onlyterm_term::Terminal` exactly like `LocalPane`
//! (pty resize first, then model resize, no output applied in between),
//! then types a marker and reports whether it landed on the prompt row.
//!
//! Needs the sideloaded `conpty.dll` and `OpenConsole.exe` (from
//! `assets/windows/conhost`) next to the example binary. Not a test: it
//! drives a real shell. Environment knobs:
//!
//! * `COLOR_F8=1` starts cmd like the default tabs: `chcp 65001>nul & color f8`.
//! * `NATIVE_DUMP=<path to conpty-native-dump.ps1>` reads the native console
//!   buffer of the child (read-only `AttachConsole`); with `STEP_NATIVE=1`
//!   after every resize, reporting `MISMATCH` against the model cursor.
//! * `FUZZ=seed:steps` replaces the size sequence with random sizes.
//! * `PENDING=text` leaves input pending in cmd's cooked read while resizing.
//! * `EXTRA_CMD=cmd` runs one more command (e.g. `cls`) before resizing.
//!
//! ```text
//! cargo run -p onlyterm-mux --example conpty_resize_cycle -- 120 48 5 10 120x49,120x48
//! ```
//! Arguments: cols, base rows, cycles, prompt lines to emit, sizes of one cycle.

use onlyterm_term::{Terminal, TerminalSize};
use portable_pty::{CommandBuilder, NativePtySystem, PtySize, PtySystem};
use std::io::Write;
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone)]
struct SharedWriter(Arc<Mutex<Box<dyn Write + Send>>>);

impl Write for SharedWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().write(buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.0.lock().unwrap().flush()
    }
}

fn size(cols: usize, rows: usize) -> TerminalSize {
    TerminalSize {
        rows,
        cols,
        pixel_width: cols * 8,
        pixel_height: rows * 16,
        dpi: 96,
    }
}

fn escaped(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .chars()
        .flat_map(|c| c.escape_debug())
        .collect()
}

/// Applies pty output to the model until `until` returns true or `limit` elapses.
fn pump(
    terminal: &mut Terminal,
    rx: &Receiver<Vec<u8>>,
    limit: Duration,
    log: &mut Vec<u8>,
    until: &dyn Fn(&Terminal) -> bool,
) -> bool {
    let deadline = Instant::now() + limit;
    loop {
        if until(terminal) {
            return true;
        }
        let now = Instant::now();
        if now >= deadline {
            return false;
        }
        if let Ok(bytes) = rx.recv_timeout(deadline - now) {
            terminal.advance_bytes(&bytes);
            log.extend_from_slice(&bytes);
        }
    }
}

fn visible_rows(terminal: &Terminal) -> Vec<String> {
    let screen = terminal.screen();
    let first = screen.phys_row(0);
    screen
        .lines_in_phys_range(first..first + screen.physical_rows)
        .iter()
        .map(|l| l.as_str().trim_end().to_string())
        .collect()
}

fn contains(terminal: &Terminal, needle: &str) -> bool {
    visible_rows(terminal).iter().any(|r| r.contains(needle))
}

fn dump(terminal: &Terminal, label: &str) {
    let screen = terminal.screen();
    let cursor = terminal.cursor_pos();
    println!(
        "--- {label}: cursor=({}, {}) rows={} scrollback_rows={}",
        cursor.x,
        cursor.y,
        screen.physical_rows,
        screen.scrollback_rows()
    );
    for (row, text) in visible_rows(terminal).iter().enumerate() {
        if !text.is_empty() || row as i64 == cursor.y {
            println!("  row {:>2}: {:?}", row, text);
        }
    }
}

/// Optional ground truth: the native console buffer of our own child, read
/// through `.scratch/conpty-native-dump.ps1` (path in `NATIVE_DUMP`).
fn native_dump(pid: u32, full: bool) -> String {
    let script = match std::env::var_os("NATIVE_DUMP") {
        Some(script) => script,
        None => return String::new(),
    };
    let out = std::path::PathBuf::from(format!(".scratch/conpty-native-{pid}.txt"));
    let status = std::process::Command::new("powershell")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(&script)
        .arg("-SnapshotPid")
        .arg(pid.to_string())
        .arg("-Out")
        .arg(&out)
        .status();
    let text = std::fs::read_to_string(&out).unwrap_or_else(|_| format!("{status:?}"));
    let _ = std::fs::remove_file(&out);
    let text = text.trim_start_matches('\u{feff}');
    if full {
        text.to_string()
    } else {
        text.lines().next().unwrap_or_default().to_string()
    }
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let num = |i: usize, default: usize| {
        args.get(i)
            .map(|a| a.parse().expect("numeric argument"))
            .unwrap_or(default)
    };
    let cols = num(0, 120);
    let base_rows = num(1, 48);
    let cycles = num(2, 5);
    let prompt_lines = num(3, 10);
    // Sizes visited by one cycle, e.g. "120x49,120x48"; the last should be the base.
    let mut sequence: Vec<TerminalSize> = args
        .get(4)
        .map(String::as_str)
        .unwrap_or("120x49,120x48")
        .split(',')
        .map(|s| {
            let (c, r) = s.split_once('x').expect("COLSxROWS");
            size(c.parse().unwrap(), r.parse().unwrap())
        })
        .collect();
    // FUZZ=seed:steps replaces the sequence with random sizes, then the base.
    if let Ok(spec) = std::env::var("FUZZ") {
        let (seed, steps) = spec.split_once(':').expect("seed:steps");
        let mut state: u64 = seed
            .parse::<u64>()
            .unwrap()
            .wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut next = |lo: usize, hi: usize| {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            lo + ((state >> 33) as usize) % (hi - lo + 1)
        };
        sequence = (0..steps.parse::<usize>().unwrap())
            .map(|_| size(next(60, 170), next(12, 55)))
            .collect();
        sequence.push(size(cols, base_rows));
        let text: Vec<String> = sequence
            .iter()
            .map(|s| format!("{}x{}", s.cols, s.rows))
            .collect();
        println!("fuzz sequence: {}", text.join(","));
    }
    // Input left pending in cmd's cooked read while resizing.
    let pending = std::env::var("PENDING").unwrap_or_default();
    let settle = Duration::from_millis(
        std::env::var("SETTLE_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(400),
    );

    let base = size(cols, base_rows);

    let pair = NativePtySystem::default().openpty(PtySize {
        rows: base.rows as u16,
        cols: base.cols as u16,
        pixel_width: base.pixel_width as u16,
        pixel_height: base.pixel_height as u16,
    })?;
    let (master, slave) = (pair.master, pair.slave);
    let mut cmd = CommandBuilder::new("cmd.exe");
    cmd.args(["/d", "/q", "/k"]);
    // The user's tabs start with `cmd /k "chcp 65001>nul & color f8"`.
    if std::env::var_os("COLOR_F8").is_some() {
        cmd.arg("chcp 65001>nul & color f8");
    }
    cmd.env("PROMPT", "PROBE$G");
    cmd.cwd(std::env::current_dir()?);
    let child = slave.spawn_command(cmd)?;
    drop(slave);
    println!("child cmd.exe pid={:?}", child.process_id());

    let writer = SharedWriter(Arc::new(Mutex::new(master.take_writer()?)));
    let mut terminal = Terminal::new(
        base,
        Arc::new(onlyterm_config::TermConfig::new()),
        "OnlyTerm",
        onlyterm_config::onlyterm_version(),
        Box::new(writer.clone()),
    );
    terminal.enable_conpty_quirks();

    let (tx, rx) = channel::<Vec<u8>>();
    let mut reader = master.try_clone_reader()?;
    std::thread::spawn(move || {
        let mut buf = vec![0u8; 64 * 1024];
        while let Ok(n) = std::io::Read::read(&mut reader, &mut buf) {
            if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                break;
            }
        }
    });

    let mut log = vec![];
    let mut w = writer.clone();
    anyhow::ensure!(
        pump(
            &mut terminal,
            &rx,
            Duration::from_secs(10),
            &mut log,
            &|t| { contains(t, "PROBE>") }
        ),
        "no prompt"
    );
    for i in 0..prompt_lines {
        let marker = format!("line{i:02}");
        write!(w, "echo {marker}\r")?;
        w.flush()?;
        anyhow::ensure!(
            pump(&mut terminal, &rx, Duration::from_secs(5), &mut log, &|t| {
                visible_rows(t).iter().any(|r| r == &marker)
            }),
            "echo {marker} did not appear"
        );
    }
    pump(&mut terminal, &rx, settle, &mut log, &|_| false);
    // Optional extra command before the cycles, e.g. `cls` or a long echo.
    if let Ok(extra) = std::env::var("EXTRA_CMD") {
        log.clear();
        write!(w, "{extra}\r")?;
        w.flush()?;
        pump(&mut terminal, &rx, settle * 3, &mut log, &|_| false);
    }
    if std::env::var_os("DUMP_START").is_some() {
        println!("startup output: {}", escaped(&log));
    }
    if !pending.is_empty() {
        write!(w, "{pending}")?;
        w.flush()?;
        let expect = format!("PROBE>{pending}");
        anyhow::ensure!(
            pump(&mut terminal, &rx, Duration::from_secs(5), &mut log, &|t| {
                visible_rows(t).iter().any(|r| r.ends_with(&expect))
            }),
            "pending input did not echo"
        );
    }
    dump(&terminal, "before cycles");
    if let (Some(pid), true) = (
        child.process_id(),
        std::env::var_os("STEP_NATIVE").is_some(),
    ) {
        println!("native before cycles:\n{}", native_dump(pid, true));
    }

    let pid = child.process_id();
    let step_native = std::env::var_os("STEP_NATIVE").is_some();
    let resize = |terminal: &mut Terminal, to: TerminalSize, log: &mut Vec<u8>| {
        log.clear();
        master
            .resize(PtySize {
                rows: to.rows as u16,
                cols: to.cols as u16,
                pixel_width: to.pixel_width as u16,
                pixel_height: to.pixel_height as u16,
            })
            .unwrap();
        terminal.resize(to);
        pump(terminal, &rx, settle, log, &|_| false);
        println!(
            "resize -> {}x{}: model cursor={:?} emitted: {}",
            to.cols,
            to.rows,
            (terminal.cursor_pos().x, terminal.cursor_pos().y),
            escaped(log)
        );
        if let (Some(pid), true) = (pid, step_native) {
            let full = std::env::var("STEP_NATIVE").is_ok_and(|v| v == "full");
            if full {
                dump(terminal, "model");
            }
            let native = native_dump(pid, full);
            println!("    {}", native);
            // "native cursor=(x,y) window=top..bottom ..."
            let parsed = (|| {
                let rest = native.split("cursor=(").nth(1)?;
                let (xy, rest) = rest.split_once(')')?;
                let (x, y) = xy.split_once(',')?;
                let top = rest.split("window=").nth(1)?.split("..").next()?;
                Some((
                    x.parse::<i64>().ok()?,
                    y.parse::<i64>().ok()? - top.parse::<i64>().ok()?,
                ))
            })();
            let model = (terminal.cursor_pos().x as i64, terminal.cursor_pos().y);
            if parsed != Some(model) {
                println!("    MISMATCH model={model:?} native={parsed:?}");
            }
        }
    };
    for _ in 0..cycles {
        for &to in &sequence {
            resize(&mut terminal, to, &mut log);
        }
    }
    dump(&terminal, "after cycles");

    log.clear();
    write!(w, "xyz")?;
    w.flush()?;
    let seen = pump(&mut terminal, &rx, Duration::from_secs(5), &mut log, &|t| {
        contains(t, "xyz")
    });
    pump(&mut terminal, &rx, settle, &mut log, &|_| false);
    println!("typed xyz (seen={seen}), emitted: {}", escaped(&log));
    dump(&terminal, "after typing");
    let rows = visible_rows(&terminal);
    let expect = format!("PROBE>{pending}xyz");
    let attached = rows.iter().any(|r| r.ends_with(&expect));
    println!(
        "RESULT: {}",
        if attached {
            "input attached to prompt"
        } else {
            "INPUT DETACHED FROM PROMPT"
        }
    );

    if let Some(pid) = pid {
        println!("native after typing:\n{}", native_dump(pid, true));
    }
    write!(w, "\x08\x08\x08exit\r")?;
    w.flush()?;
    pump(
        &mut terminal,
        &rx,
        Duration::from_secs(3),
        &mut log,
        &|_| false,
    );
    drop(master);
    Ok(())
}
