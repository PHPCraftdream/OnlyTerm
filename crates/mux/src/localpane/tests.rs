mod process_info;
mod search;

use super::*;
use onlyterm_term::color::ColorPalette;
use onlyterm_term::{TerminalConfiguration, TerminalSize};
use portable_pty::{Child, ChildKiller, ExitStatus, MasterPty, PtySize};
use std::io::{Read, Result as IoResult};

/// A `Child` double that never exits on its own, mirroring
/// `test::terminal_lock_contention::NeverExitChild`: `LocalPane` only
/// needs something implementing the trait to track process state,
/// and these tests never let it actually run to completion.
#[derive(Debug)]
struct NeverExitChild;

impl Child for NeverExitChild {
    fn try_wait(&mut self) -> IoResult<Option<ExitStatus>> {
        Ok(None)
    }
    fn wait(&mut self) -> IoResult<ExitStatus> {
        loop {
            std::thread::sleep(Duration::from_secs(3600));
        }
    }
    fn process_id(&self) -> Option<u32> {
        None
    }
    #[cfg(windows)]
    fn as_raw_handle(&self) -> Option<std::os::windows::io::RawHandle> {
        None
    }
}

#[derive(Debug, Clone)]
struct NeverExitKiller;
impl ChildKiller for NeverExitKiller {
    fn kill(&mut self) -> IoResult<()> {
        Ok(())
    }
    fn clone_killer(&self) -> Box<dyn ChildKiller + Send + Sync> {
        Box::new(self.clone())
    }
}
impl ChildKiller for NeverExitChild {
    fn kill(&mut self) -> IoResult<()> {
        Ok(())
    }
    fn clone_killer(&self) -> Box<dyn ChildKiller + Send + Sync> {
        Box::new(NeverExitKiller)
    }
}

/// A `Child` double that, unlike `NeverExitChild`, reports the
/// *actual* pid of the test process itself via `process_id()`. Task
/// #247's `divine_process_list` background-refresh path needs a pid
/// that `LocalProcessInfo::with_root_pid` can genuinely resolve
/// against a live system process snapshot (there's no cheap way to
/// fake that snapshot), and the test process is guaranteed to exist
/// for as long as the test runs.
#[derive(Debug)]
struct RealPidChild;

impl Child for RealPidChild {
    fn try_wait(&mut self) -> IoResult<Option<ExitStatus>> {
        Ok(None)
    }
    fn wait(&mut self) -> IoResult<ExitStatus> {
        loop {
            std::thread::sleep(Duration::from_secs(3600));
        }
    }
    fn process_id(&self) -> Option<u32> {
        Some(std::process::id())
    }
    #[cfg(windows)]
    fn as_raw_handle(&self) -> Option<std::os::windows::io::RawHandle> {
        None
    }
}

impl ChildKiller for RealPidChild {
    fn kill(&mut self) -> IoResult<()> {
        Ok(())
    }
    fn clone_killer(&self) -> Box<dyn ChildKiller + Send + Sync> {
        Box::new(NeverExitKiller)
    }
}

/// A `MasterPty` double that records whether it has been dropped, so
/// tests can confirm `kill()` doesn't drop it inline (the whole point
/// of the deferred-drop mechanism).
struct FakeMasterPty {
    size: Mutex<PtySize>,
    dropped: Arc<std::sync::atomic::AtomicBool>,
}

impl Drop for FakeMasterPty {
    fn drop(&mut self) {
        self.dropped
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

impl MasterPty for FakeMasterPty {
    fn resize(&self, size: PtySize) -> anyhow::Result<()> {
        *self.size.lock() = size;
        Ok(())
    }
    fn get_size(&self) -> anyhow::Result<PtySize> {
        Ok(*self.size.lock())
    }
    fn try_clone_reader(&self) -> anyhow::Result<Box<dyn Read + Send>> {
        Ok(Box::new(std::io::empty()))
    }
    fn take_writer(&self) -> anyhow::Result<Box<dyn Write + Send>> {
        Ok(Box::new(Vec::new()))
    }
}

/// A `Write` double that records whether it has been dropped, so
/// tests can confirm `kill()` defers dropping the *real* writer (not
/// just the pty) rather than letting it drop inline along with the
/// rest of `LocalPane` -- see `kill()`'s doc comment for why that
/// matters (`ConPtyMasterPty::take_writer()` hands out the pty's only
/// reference to its stdin pipe).
struct TrackedWriter {
    dropped: Arc<std::sync::atomic::AtomicBool>,
}

impl Write for TrackedWriter {
    fn write(&mut self, buf: &[u8]) -> IoResult<usize> {
        Ok(buf.len())
    }
    fn flush(&mut self) -> IoResult<()> {
        Ok(())
    }
}

impl Drop for TrackedWriter {
    fn drop(&mut self) {
        self.dropped
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

/// A `Write` double whose `write` call blocks until the test
/// explicitly releases it, standing in for a real ConPTY stdin pipe
/// whose reader (the child process) has stopped reading -- the
/// scenario that used to make `kill()` hang the GUI thread forever
/// (see the regression this test guards against, below). Records
/// whether a write was ever observed, so the test can confirm the
/// soft `\x03` byte does eventually get written on the background
/// thread once released.
struct BlockingWriter {
    /// Held locked by the test until it wants to let a pending
    /// `write` call through.
    gate: Arc<Mutex<()>>,
    wrote: Arc<std::sync::atomic::AtomicBool>,
}

impl Write for BlockingWriter {
    fn write(&mut self, buf: &[u8]) -> IoResult<usize> {
        // Blocks for as long as the test is holding `gate` locked,
        // mirroring a real write blocking on a full, unread pipe.
        let _guard = self.gate.lock();
        self.wrote.store(true, std::sync::atomic::Ordering::SeqCst);
        Ok(buf.len())
    }
    fn flush(&mut self) -> IoResult<()> {
        Ok(())
    }
}

#[derive(Debug)]
struct TestConfig;
impl TerminalConfiguration for TestConfig {
    fn allow_process_title_updates(&self) -> bool {
        true
    }

    fn color_palette(&self) -> ColorPalette {
        ColorPalette::default()
    }
}

const ROWS: usize = 24;
const COLS: usize = 80;

fn make_pane() -> (Arc<LocalPane>, Arc<std::sync::atomic::AtomicBool>) {
    let size = TerminalSize {
        rows: ROWS,
        cols: COLS,
        pixel_width: COLS * 8,
        pixel_height: ROWS * 16,
        dpi: 0,
    };
    let terminal = Terminal::new(
        size,
        Arc::new(TestConfig),
        "OnlyTerm",
        "0.0.0",
        Box::new(Vec::new()),
    );
    let dropped = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let pty = Box::new(FakeMasterPty {
        size: Mutex::new(PtySize {
            rows: ROWS as u16,
            cols: COLS as u16,
            pixel_width: 0,
            pixel_height: 0,
        }),
        dropped: Arc::clone(&dropped),
    });
    let writer = Box::new(Vec::new());
    let pane = Arc::new(LocalPane::new(
        1,
        terminal,
        Box::new(NeverExitChild),
        pty,
        writer,
        1,
        "test".to_string(),
        None,
    ));
    (pane, dropped)
}

/// Like `make_pane`, but backed by `RealPidChild` instead of
/// `NeverExitChild`, so `ProcessState::Running.pid` is
/// `Some(std::process::id())` -- a pid that
/// `LocalProcessInfo::with_root_pid` can genuinely resolve. Needed by
/// `divine_process_list`/`CachedProcInfo` tests (task #247): with
/// `NeverExitChild`'s `pid: None`, `divine_process_list` bails out
/// before ever touching the cache at all.
fn make_pane_with_real_pid() -> Arc<LocalPane> {
    let size = TerminalSize {
        rows: ROWS,
        cols: COLS,
        pixel_width: COLS * 8,
        pixel_height: ROWS * 16,
        dpi: 0,
    };
    let terminal = Terminal::new(
        size,
        Arc::new(TestConfig),
        "OnlyTerm",
        "0.0.0",
        Box::new(Vec::new()),
    );
    let dropped = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let pty = Box::new(FakeMasterPty {
        size: Mutex::new(PtySize {
            rows: ROWS as u16,
            cols: COLS as u16,
            pixel_width: 0,
            pixel_height: 0,
        }),
        dropped,
    });
    let writer = Box::new(Vec::new());
    Arc::new(LocalPane::new(
        1,
        terminal,
        Box::new(RealPidChild),
        pty,
        writer,
        1,
        "test".to_string(),
        None,
    ))
}

/// Like `make_pane`, but with a `TrackedWriter` in place of the plain
/// `Vec::new()` writer, so tests can also observe whether the *real*
/// writer's drop was deferred alongside the pty's.
fn make_pane_with_tracked_writer() -> (
    Arc<LocalPane>,
    Arc<std::sync::atomic::AtomicBool>,
    Arc<std::sync::atomic::AtomicBool>,
) {
    let size = TerminalSize {
        rows: ROWS,
        cols: COLS,
        pixel_width: COLS * 8,
        pixel_height: ROWS * 16,
        dpi: 0,
    };
    let terminal = Terminal::new(
        size,
        Arc::new(TestConfig),
        "OnlyTerm",
        "0.0.0",
        Box::new(Vec::new()),
    );
    let pty_dropped = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let pty = Box::new(FakeMasterPty {
        size: Mutex::new(PtySize {
            rows: ROWS as u16,
            cols: COLS as u16,
            pixel_width: 0,
            pixel_height: 0,
        }),
        dropped: Arc::clone(&pty_dropped),
    });
    let writer_dropped = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let writer = Box::new(TrackedWriter {
        dropped: Arc::clone(&writer_dropped),
    });
    let pane = Arc::new(LocalPane::new(
        1,
        terminal,
        Box::new(NeverExitChild),
        pty,
        writer,
        1,
        "test".to_string(),
        None,
    ));
    (pane, pty_dropped, writer_dropped)
}

/// Like `make_pane`, but with a `BlockingWriter` in place of the
/// plain `Vec::new()` writer, so tests can prove `kill()` never
/// blocks on the caller's thread even when writing the soft-signal
/// byte would block forever.
fn make_pane_with_blocking_writer() -> (
    Arc<LocalPane>,
    Arc<Mutex<()>>,
    Arc<std::sync::atomic::AtomicBool>,
) {
    let size = TerminalSize {
        rows: ROWS,
        cols: COLS,
        pixel_width: COLS * 8,
        pixel_height: ROWS * 16,
        dpi: 0,
    };
    let terminal = Terminal::new(
        size,
        Arc::new(TestConfig),
        "OnlyTerm",
        "0.0.0",
        Box::new(Vec::new()),
    );
    let pty = Box::new(FakeMasterPty {
        size: Mutex::new(PtySize {
            rows: ROWS as u16,
            cols: COLS as u16,
            pixel_width: 0,
            pixel_height: 0,
        }),
        dropped: Arc::new(std::sync::atomic::AtomicBool::new(false)),
    });
    let gate = Arc::new(Mutex::new(()));
    let wrote = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let writer = Box::new(BlockingWriter {
        gate: Arc::clone(&gate),
        wrote: Arc::clone(&wrote),
    });
    let pane = Arc::new(LocalPane::new(
        1,
        terminal,
        Box::new(NeverExitChild),
        pty,
        writer,
        1,
        "test".to_string(),
        None,
    ));
    (pane, gate, wrote)
}

/// Regression test for the bug fixed alongside this test: `kill()`
/// used to write the soft `\x03` signal synchronously on the calling
/// thread, via `self.writer` -- a thin, unbuffered pass-through over
/// the real ConPTY stdin pipe. Since `kill()` is called from the GUI
/// thread in several real paths, a blocked write (e.g. the child's
/// stdin pipe is full and it isn't reading) used to freeze every
/// window in the process. Confirms `kill()` now returns promptly even
/// when the write would block forever, and that the byte still gets
/// written once the write is able to proceed, proving the write moved
/// to the background `pty-drop-grace` thread instead of being skipped
/// outright.
#[test]
fn kill_does_not_block_on_a_stuck_writer() {
    let (pane, gate, wrote) = make_pane_with_blocking_writer();

    // Hold the gate so any write into `BlockingWriter` blocks, as it
    // would on a real pipe whose reader has stopped reading.
    let guard = gate.lock();

    let start = Instant::now();
    pane.kill();
    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_secs(1),
        "kill() must not block on a stuck writer; took {:?}",
        elapsed
    );
    assert!(
        !wrote.load(std::sync::atomic::Ordering::SeqCst),
        "the write is still blocked behind the gate, so it must not \
             have completed yet"
    );

    // Release the gate so the background thread's blocked write can
    // complete, then give it a moment to actually run.
    drop(guard);
    let mut waited = Duration::ZERO;
    while !wrote.load(std::sync::atomic::Ordering::SeqCst) && waited < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(10));
        waited += Duration::from_millis(10);
    }
    assert!(
        wrote.load(std::sync::atomic::Ordering::SeqCst),
        "the soft-signal write must still happen on the background \
             thread once it's able to proceed"
    );
}

/// Like `make_pane_with_blocking_writer`, but wraps the `BlockingWriter`
/// in a real `crate::domain::WriterWrapper` -- the type actually
/// returned by `Pane::writer()` in production (see
/// `LocalDomain::spawn_pane` / `TmuxDomain`'s pane spawn, both of which
/// hand a `WriterWrapper` clone into `LocalPane::new`). The other
/// `make_pane_*` helpers above put a plain, un-wrapped writer straight
/// into `self.writer`, which is fine for exercising `kill()`'s own
/// deferred-write mechanism, but doesn't exercise `WriterWrapper`
/// itself, which is the type this test's regression is actually about.
fn make_pane_with_writer_wrapper() -> (
    Arc<LocalPane>,
    Arc<Mutex<()>>,
    Arc<std::sync::atomic::AtomicBool>,
) {
    let size = TerminalSize {
        rows: ROWS,
        cols: COLS,
        pixel_width: COLS * 8,
        pixel_height: ROWS * 16,
        dpi: 0,
    };
    let gate = Arc::new(Mutex::new(()));
    let wrote = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let writer = crate::domain::WriterWrapper::new(Box::new(BlockingWriter {
        gate: Arc::clone(&gate),
        wrote: Arc::clone(&wrote),
    }));
    let terminal = Terminal::new_with_nonblocking_writer(
        size,
        Arc::new(TestConfig),
        "OnlyTerm",
        "0.0.0",
        Box::new(writer.clone()),
    );
    let pty = Box::new(FakeMasterPty {
        size: Mutex::new(PtySize {
            rows: ROWS as u16,
            cols: COLS as u16,
            pixel_width: 0,
            pixel_height: 0,
        }),
        dropped: Arc::new(std::sync::atomic::AtomicBool::new(false)),
    });
    let pane = Arc::new(LocalPane::new(
        1,
        terminal,
        Box::new(NeverExitChild),
        pty,
        Box::new(writer),
        1,
        "test".to_string(),
        None,
    ));
    (pane, gate, wrote)
}

/// Regression test for the bug this task fixes: `WriterWrapper`
/// (`crate::domain::WriterWrapper`, the concrete type behind
/// `Pane::writer()`) used to be a direct, blocking pass-through over
/// the real pty writer (`self.writer.lock().write(buf)`). Roughly a
/// dozen call sites in `onlyterm-gui` call
/// `pane.writer().write_all(...)` synchronously from the GUI thread
/// (paste, `SendString`, IME composition, character-picker insertion,
/// ...); if the target process wasn't reading its stdin, any of those
/// calls could block the GUI thread -- and with it every window in
/// the process -- forever. Confirms `pane.writer().write_all()` (the
/// exact call shape used by those call sites) now returns promptly
/// even when the real underlying write would block forever, and that
/// the bytes still reach the real writer once it's able to proceed,
/// proving the write moved to `WriterWrapper`'s own background thread
/// instead of being silently dropped.
#[test]
fn pane_writer_does_not_block_on_a_stuck_underlying_writer() {
    // `Pane::writer()` calls `Mux::get().record_input_for_current_identity()`,
    // so this test needs the process-global `Mux` singleton installed
    // (see the doc comment on `crate::test::MUX_TEST_GUARD` for why
    // this has to be serialized against every other test that also
    // installs one).
    let _mux_guard = crate::test::MUX_TEST_GUARD.lock();
    let mux = Arc::new(Mux::new(None));
    Mux::set_mux(&mux);

    let (pane, gate, wrote) = make_pane_with_writer_wrapper();

    // Hold the gate so any write performed by the real background
    // thread blocks, as it would on a real pipe whose reader has
    // stopped reading.
    let guard = gate.lock();

    let start = Instant::now();
    pane.writer()
        .write_all(b"hello")
        .expect("write_all must succeed (it only enqueues)");
    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_secs(1),
        "pane.writer().write_all() must not block on a stuck \
             underlying writer; took {:?}",
        elapsed
    );
    assert!(
        !wrote.load(std::sync::atomic::Ordering::SeqCst),
        "the write is still blocked behind the gate, so it must not \
             have reached the underlying writer yet"
    );

    // Release the gate so the background thread's blocked write can
    // complete, then give it a moment to actually run.
    drop(guard);
    let mut waited = Duration::ZERO;
    while !wrote.load(std::sync::atomic::Ordering::SeqCst) && waited < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(10));
        waited += Duration::from_millis(10);
    }
    assert!(
        wrote.load(std::sync::atomic::Ordering::SeqCst),
        "the write must still reach the underlying writer on \
             WriterWrapper's background thread once it's able to proceed"
    );

    Mux::shutdown();
}

/// Regression test for the GUI-freeze bug fixed alongside this test:
/// `has_unseen_output()` used to take `terminal.lock()` with no
/// timeout. The GUI title-refresh path (`update_title_impl`) polls
/// every pane's `has_unseen_output()` on essentially every key/mouse
/// event, so a single background pane whose terminal mutex was held
/// (or wedged) blocked the GUI thread -- and with it the process's
/// single message loop -- forever. Confirms the read path is now
/// lock-free: it returns promptly even while another thread holds
/// `terminal.lock()`, and that the published flag still tracks real
/// focus/output state changes.
#[test]
fn has_unseen_output_does_not_block_on_a_locked_terminal() {
    let (pane, _dropped) = make_pane();

    // --- Correctness of the published flag (no contention) ---
    // A fresh pane is focused, so there is no unseen output.
    assert!(!pane.has_unseen_output());

    // Losing focus snapshots `lost_focus_seqno = seqno`, so the two
    // are still equal: still no unseen output.
    pane.focus_changed(false);
    assert!(!pane.has_unseen_output());

    // New output bumps `seqno` ahead of `lost_focus_seqno`: now there
    // is unseen output. This also exercises the publication path in
    // `increment_seqno`, the other chokepoint alongside `focus_changed`.
    pane.terminal.lock().increment_seqno();
    assert!(pane.has_unseen_output());

    // --- Non-blocking under contention ---
    // Hold `terminal.lock()` on another thread, simulating a
    // wedged/held mutex. Under the old code the `has_unseen_output()`
    // call below would block forever here.
    let started = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let release = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let blocker_pane = Arc::clone(&pane);
    let blocker_started = Arc::clone(&started);
    let blocker_release = Arc::clone(&release);
    let handle = std::thread::spawn(move || {
        // Hold the terminal mutex for the whole lifetime of this
        // guard, releasing it only once the main thread is done.
        let _guard = blocker_pane.terminal.lock();
        blocker_started.store(true, std::sync::atomic::Ordering::SeqCst);
        while !blocker_release.load(std::sync::atomic::Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(5));
        }
    });

    // Wait until the blocker has actually acquired the lock.
    while !started.load(std::sync::atomic::Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(5));
    }

    // This call must return immediately despite the lock being held.
    let call_start = Instant::now();
    let _ = pane.has_unseen_output();
    let elapsed = call_start.elapsed();
    assert!(
        elapsed < Duration::from_secs(1),
        "has_unseen_output() must not block on a locked terminal; took {:?}",
        elapsed
    );

    // Let the blocker release the lock and join cleanly.
    release.store(true, std::sync::atomic::Ordering::SeqCst);
    handle.join().expect("blocker thread panicked");
}

/// Regression test for task #246: `get_title()` used to take
/// `terminal.lock()` unconditionally, with no timeout. Like
/// `has_unseen_output()` before it (task #244), `get_title()` is
/// called for the active pane of *every* tab from
/// `get_tab_information()`/`update_title_impl` on the GUI thread on
/// essentially every key/mouse event, so a single background pane
/// whose terminal mutex is wedged (held by another thread and never
/// released) could block the whole window's message loop forever.
/// Confirms that `get_title()` now gives up after a bounded wait
/// (`TERMINAL_ACCESSOR_LOCK_TIMEOUT`) and returns the last
/// known-good cached title instead of blocking, while still
/// reflecting real title changes once the lock is free.
#[test]
fn get_title_does_not_block_on_a_locked_terminal() {
    let (pane, _dropped) = make_pane();

    // `Terminal::perform_actions` on a `SetWindowTitle` OSC would
    // otherwise fire `LocalPaneNotifHandler::alert`, which calls
    // `promise::spawn::spawn_into_main_thread` -- that needs a
    // process-global scheduler installed (see
    // `promise::spawn::set_schedulers`), which is orthogonal to what
    // this test is about. Swap in a no-op handler so the title change
    // below is observable through `get_title()` without dragging in
    // the whole scheduler/Mux-singleton setup.
    struct NoopAlertHandler;
    impl AlertHandler for NoopAlertHandler {
        fn alert(&mut self, _alert: Alert) {}
    }
    pane.terminal
        .lock()
        .set_notification_handler(Box::new(NoopAlertHandler));

    // --- Correctness (no contention): a real title change is
    // observed once the lock is available. ---
    pane.terminal
        .lock()
        .perform_actions(vec![Action::OperatingSystemCommand(Box::new(
            termwiz::escape::OperatingSystemCommand::SetWindowTitle("known-good-title".to_string()),
        ))]);
    assert_eq!(pane.get_title(), "known-good-title");

    // --- Non-blocking under contention ---
    // Hold `terminal.lock()` on another thread, simulating a
    // wedged/held mutex. Under the old code the `get_title()` call
    // below would block for as long as the lock was held.
    let started = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let release = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let blocker_pane = Arc::clone(&pane);
    let blocker_started = Arc::clone(&started);
    let blocker_release = Arc::clone(&release);
    let handle = std::thread::spawn(move || {
        // Hold the terminal mutex for the whole lifetime of this
        // guard, releasing it only once the main thread is done.
        let _guard = blocker_pane.terminal.lock();
        blocker_started.store(true, std::sync::atomic::Ordering::SeqCst);
        while !blocker_release.load(std::sync::atomic::Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(5));
        }
    });

    // Wait until the blocker has actually acquired the lock.
    while !started.load(std::sync::atomic::Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(5));
    }

    // This call must return promptly (bounded by
    // `TERMINAL_ACCESSOR_LOCK_TIMEOUT`) despite the lock being held,
    // and must serve the cached title from before contention began
    // rather than blocking until the lock is released.
    let call_start = Instant::now();
    let title = pane.get_title();
    let elapsed = call_start.elapsed();
    assert!(
        elapsed < Duration::from_secs(1),
        "get_title() must not block on a locked terminal; took {:?}",
        elapsed
    );
    assert_eq!(
        title, "known-good-title",
        "get_title() must fall back to the last known-good cached \
             title when the terminal lock can't be acquired in time"
    );

    // Let the blocker release the lock and join cleanly.
    release.store(true, std::sync::atomic::Ordering::SeqCst);
    handle.join().expect("blocker thread panicked");
}

/// Task #248: `is_unresponsive()` should flip to `true` the moment a
/// bounded `terminal.lock()` attempt (`try_lock_terminal_for`, task
/// #246) times out on any of the four GUI-thread-reachable accessors,
/// and flip back to `false` as soon as a subsequent bounded attempt
/// succeeds. This is what makes a wedged pane's terminal lock
/// *observable* at the pane level, rather than only showing up as a
/// silent `metrics::counter!` increment. Mirrors the
/// real-blocker-thread contention setup used by
/// `get_title_does_not_block_on_a_locked_terminal` (task #246).
#[test]
fn is_unresponsive_flips_on_timeout_and_clears_on_success() {
    let (pane, _dropped) = make_pane();

    // A fresh pane has never timed out: not unresponsive.
    assert!(!pane.is_unresponsive());

    // Hold `terminal.lock()` on another thread for longer than
    // `TERMINAL_ACCESSOR_LOCK_TIMEOUT`, simulating a wedged mutex.
    let started = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let release = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let blocker_pane = Arc::clone(&pane);
    let blocker_started = Arc::clone(&started);
    let blocker_release = Arc::clone(&release);
    let handle = std::thread::spawn(move || {
        let _guard = blocker_pane.terminal.lock();
        blocker_started.store(true, std::sync::atomic::Ordering::SeqCst);
        while !blocker_release.load(std::sync::atomic::Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(5));
        }
    });

    // Wait until the blocker has actually acquired the lock.
    while !started.load(std::sync::atomic::Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(5));
    }

    // Any of the four bounded accessors will do; `get_title()` is
    // enough to exercise `try_lock_terminal_for`'s timeout path and
    // set the flag.
    let _ = pane.get_title();
    assert!(
        pane.is_unresponsive(),
        "is_unresponsive() must become true after a bounded \
             terminal.lock() attempt times out"
    );

    // Let the blocker release the lock and join cleanly.
    release.store(true, std::sync::atomic::Ordering::SeqCst);
    handle.join().expect("blocker thread panicked");

    // Now that the lock is free, the next bounded access should
    // succeed and clear the flag again.
    let _ = pane.get_title();
    assert!(
        !pane.is_unresponsive(),
        "is_unresponsive() must clear back to false once a bounded \
             terminal.lock() attempt succeeds again"
    );
}

/// Task #273 regression test: a pane that trips the render-budget
/// signal and then simply stops being painted (e.g. its tab is
/// switched away from) must eventually stop reporting
/// `is_unresponsive() == true` on its own, rather than latching
/// `true` forever. Before this fix, `render_budget_exceeded` was a
/// plain sticky `bool` that only `set_render_budget_exceeded(false)`
/// -- called exclusively from the per-frame render loop, which never
/// runs again for an unpainted pane -- could clear.
///
/// This test doesn't wait for the real `RENDER_BUDGET_EXCEEDED_EXPIRY`
/// (1 second) in real time; instead it reaches into the pane's
/// internal `render_budget_exceeded` cell and backdates the stored
/// `Instant` past the expiry window, which is equivalent to letting
/// that much wall-clock time actually pass without depending on a
/// slow, real-time sleep in the test suite.
#[test]
fn render_budget_exceeded_expires_once_painting_stops() {
    let (pane, _dropped) = make_pane();

    // A fresh pane has never tripped the render budget: not
    // unresponsive.
    assert!(!pane.is_unresponsive());

    // Simulate a frame that exceeded the render budget for this pane
    // (what `crates/onlyterm-gui/src/termwindow/render/pane.rs` does
    // once per painted pane per frame).
    pane.set_render_budget_exceeded(true);
    assert!(
        pane.is_unresponsive(),
        "is_unresponsive() must become true immediately after \
             set_render_budget_exceeded(true), mirroring a real, \
             currently-ongoing budget-exceeded frame on the active tab"
    );

    // Backdate the observation past the expiry window, simulating
    // the pane's tab having been switched away from (and thus never
    // painted, and never getting another set_render_budget_exceeded
    // call at all) for at least that long.
    *pane.render_budget_exceeded.lock() =
        Some(Instant::now() - RENDER_BUDGET_EXCEEDED_EXPIRY - Duration::from_millis(1));

    assert!(
        !pane.is_unresponsive(),
        "a render-budget-exceeded observation older than \
             RENDER_BUDGET_EXCEEDED_EXPIRY must not keep is_unresponsive() \
             true forever for a pane that has stopped being painted"
    );
}

/// After `kill()`, `self.pty` must be `None` (taken, not yet
/// dropped): this is the mechanism that lets `LocalPane::drop()` run
/// moments later without tearing down the pty inline.
#[test]
fn kill_takes_pty_leaving_none() {
    let (pane, dropped) = make_pane();
    assert!(pane.pty.lock().is_some());
    pane.kill();
    assert!(pane.pty.lock().is_none());
    // The deferred-drop thread hasn't had time to run yet (it sleeps
    // for PTY_DROP_GRACE_MS before dropping), so the pty is still
    // alive, just no longer reachable through `self.pty`.
    assert!(!dropped.load(std::sync::atomic::Ordering::SeqCst));
}

/// Regression test for the bug fixed alongside this test: `kill()`
/// used to defer dropping `self.pty` but left `self.writer` dropping
/// immediately, which closed the pty's underlying pipe right away
/// regardless -- because `ConPtyMasterPty::take_writer()`
/// (`crates/pty/src/win/conpty.rs`) hands the pty's stdin
/// `FileDescriptor` to whoever calls it and keeps no reference of its
/// own, `self.writer` is the *only* thing keeping that pipe open.
/// Confirms the *real* writer's drop is now deferred the same way the
/// pty's is, and that `self.writer` is left holding a harmless sink in
/// the meantime rather than `None` (avoiding a wider `Option`-ifying
/// change to the `writer()` accessor, part of the `Pane` trait's
/// public surface).
#[test]
fn kill_defers_the_real_writer_too() {
    let (pane, pty_dropped, writer_dropped) = make_pane_with_tracked_writer();
    pane.kill();

    // Neither the pty nor the real writer have been dropped yet (the
    // deferred-drop thread sleeps for PTY_DROP_GRACE_MS first).
    assert!(!pty_dropped.load(std::sync::atomic::Ordering::SeqCst));
    assert!(
        !writer_dropped.load(std::sync::atomic::Ordering::SeqCst),
        "the real writer must not drop immediately, or the deferred \
             pty drop accomplishes nothing"
    );

    // `self.writer` still holds *something* usable -- the sink
    // swapped in by `kill()` -- rather than being left in a state
    // that would panic a caller still holding a `writer()` guard.
    // Goes at the field directly (not the public `writer()`
    // accessor, which also calls `Mux::get()` for unrelated
    // bookkeeping that isn't set up in this unit test).
    let mut writer = pane.writer.lock();
    writer
        .write_all(b"late write after kill")
        .expect("writing to the post-kill sink must not error");
}

/// Calling `kill()` twice must not panic or double-fire the
/// signal-and-defer sequence; the second call should see `killed ==
/// true` and skip straight past it.
#[test]
fn kill_is_idempotent() {
    let (pane, _dropped) = make_pane();
    pane.kill();
    pane.kill();
    assert!(pane.pty.lock().is_none());
}

/// Every `Option`-aware call site added for task #237 must degrade
/// gracefully (no panic, sensible default) once `self.pty` is `None`
/// after `kill()`.
#[test]
fn pty_dependent_calls_dont_panic_after_kill() {
    let (pane, _dropped) = make_pane();
    pane.kill();

    // exit_behavior(): falls through to `None` rather than treating
    // a gone pty as `FailedSpawnPty`.
    assert_eq!(pane.exit_behavior(), None);

    // resize(): silently skips the pty resize, still updates the
    // terminal model, and must not error.
    let new_size = TerminalSize {
        rows: ROWS + 1,
        cols: COLS + 1,
        pixel_width: (COLS + 1) * 8,
        pixel_height: (ROWS + 1) * 16,
        dpi: 0,
    };
    assert!(pane.resize(new_size).is_ok());

    // reader(): nothing left to read from.
    assert!(pane.reader().unwrap().is_none());

    // get_metadata(): must not panic digging for termios on a gone pty.
    let _ = pane.get_metadata();

    assert_eq!(pane.tty_name(), None);
    assert!(pane
        .get_foreground_process_info(CachePolicy::FetchImmediate)
        .is_none());

    // `can_close_without_prompting` has no pty-derived fallback (it
    // falls through to `false` regardless of pty state once
    // `divine_process_list` finds no pid), so the only thing this call
    // must do post-kill is *not panic* digging through a gone pty.
    let _ = pane.can_close_without_prompting(CloseReason::Tab);
}
