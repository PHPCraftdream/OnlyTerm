//! End-to-end regression test for task #256 ("244.t1"): proves the
//! COMBINED, user-visible effect of tasks #244-#251 -- a background pane
//! whose terminal is wedged (here: a real thread holding `terminal.lock()`
//! indefinitely, the same faithful stand-in used by
//! `localpane::tests::has_unseen_output_does_not_block_on_a_locked_terminal`
//! / `get_title_does_not_block_on_a_locked_terminal`) must not be able to
//! block the GUI thread, and by extension must not block input/rendering
//! for OTHER, unaffected panes in the same window.
//!
//! Each of the properties exercised here already has its own isolated
//! unit test living next to the production code in
//! `crates/mux/src/localpane.rs`'s `tests` module (see the list in that
//! module: `has_unseen_output_does_not_block_on_a_locked_terminal`,
//! `get_title_does_not_block_on_a_locked_terminal`,
//! `pane_writer_does_not_block_on_a_stuck_underlying_writer`,
//! `is_unresponsive_flips_on_timeout_and_clears_on_success`,
//! `divine_process_list_returns_stale_data_and_backgrounds_refresh`). This
//! test is deliberately NOT about re-proving any one of those in
//! isolation again -- it drives TWO real `LocalPane`s side by side (a
//! "victim" with its terminal wedged, and a "healthy" pane with no
//! contention at all) and exercises exactly the operation set
//! `onlyterm-gui`'s `TermWindow::pos_pane_to_pane_info`
//! (`crates/onlyterm-gui/src/termwindow/mod.rs`) performs against every
//! pane on essentially every key/mouse event via `get_tab_information()`:
//! `has_unseen_output()`, `is_unresponsive()`, `get_title()`,
//! `copy_user_vars()`, `get_progress()`,
//! `get_current_working_dir(CachePolicy::AllowStale)` -- plus a
//! `pane.writer().write_all()` call standing in for a paste/`SendString`
//! from the GUI thread. The property this test is the first to pin is
//! cross-pane isolation: that the healthy pane's own accessors are
//! unaffected by the victim's wedged lock, not just that the victim's own
//! accessors individually recover (which the per-accessor tests already
//! cover).
use crate::localpane::LocalPane;
use crate::pane::{CachePolicy, Pane};
use onlyterm_term::color::ColorPalette;
use onlyterm_term::{Terminal, TerminalConfiguration, TerminalSize};
use parking_lot::Mutex;
use portable_pty::{Child, ChildKiller, ExitStatus, MasterPty, PtySize};
use std::io::{Read, Result as IoResult, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// A `Child` double that never exits on its own, mirroring
/// `localpane::tests::NeverExitChild` / `terminal_lock_contention::NeverExitChild`:
/// `LocalPane` only needs something implementing the trait to track
/// process state, and this test never lets it actually run to completion.
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

struct FakeMasterPty {
    size: Mutex<PtySize>,
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

/// A `Write` double whose `write` call blocks until the test explicitly
/// releases it, standing in for a real pty stdin pipe whose reader has
/// stopped reading. Wrapped in a real `crate::domain::WriterWrapper` below
/// (the type actually returned by `Pane::writer()` in production) so this
/// test exercises the exact call shape `pane.writer().write_all(...)`
/// call sites in `onlyterm-gui` use (paste, `SendString`, ...).
struct BlockingWriter {
    gate: Arc<Mutex<()>>,
    wrote: Arc<AtomicBool>,
    completed: std::sync::mpsc::SyncSender<()>,
}

impl Write for BlockingWriter {
    fn write(&mut self, buf: &[u8]) -> IoResult<usize> {
        let _guard = self.gate.lock();
        self.wrote.store(true, Ordering::SeqCst);
        let _ = self.completed.try_send(());
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

/// Builds a real `LocalPane` with a real `onlyterm_term::Terminal` behind
/// the exact same `Mutex` used in production, and a real
/// `crate::domain::WriterWrapper` (task #245) in front of a
/// `BlockingWriter`, so `pane.writer().write_all()` exercises the actual
/// non-blocking machinery rather than a plain `Vec` sink.
fn make_pane(
    name: &str,
) -> (
    Arc<LocalPane>,
    Arc<Mutex<()>>,
    Arc<AtomicBool>,
    std::sync::mpsc::Receiver<()>,
) {
    let size = TerminalSize {
        rows: ROWS,
        cols: COLS,
        pixel_width: COLS * 8,
        pixel_height: ROWS * 16,
        dpi: 0,
    };
    let gate = Arc::new(Mutex::new(()));
    let wrote = Arc::new(AtomicBool::new(false));
    let (completed, completion) = std::sync::mpsc::sync_channel(1);
    let writer = crate::domain::WriterWrapper::new(Box::new(BlockingWriter {
        gate: Arc::clone(&gate),
        wrote: Arc::clone(&wrote),
        completed,
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
    });
    let pane = Arc::new(LocalPane::new(
        1,
        terminal,
        Box::new(NeverExitChild),
        pty,
        Box::new(writer),
        1,
        name.to_string(),
        None,
    ));
    (pane, gate, wrote, completion)
}

/// Snapshot of everything `TermWindow::pos_pane_to_pane_info`
/// (`crates/onlyterm-gui/src/termwindow/mod.rs`) reads off a pane on the
/// GUI thread, plus the wall-clock time the whole batch took.
struct PaneInfoSnapshot {
    has_unseen_output: bool,
    /// `is_unresponsive()` read AFTER the rest of the batch below, not
    /// before: `pos_pane_to_pane_info` itself reads it first (before
    /// `get_title()`/`copy_user_vars()`/etc, which are the calls that can
    /// actually flip it via a timed-out `try_lock_terminal_for`), so a
    /// "before" read only ever reflects the *previous* batch's outcome.
    /// This field captures the flag's state once this batch's own lock
    /// attempts have had a chance to set it, which is what this test
    /// actually needs to assert on.
    is_unresponsive_after: bool,
    title: String,
    user_vars: std::collections::HashMap<String, String>,
    elapsed: Duration,
}

/// Reproduces exactly the read sequence `pos_pane_to_pane_info` performs,
/// timing the whole batch the way a single GUI-thread frame would
/// experience it.
fn snapshot_pane_info(pane: &LocalPane) -> PaneInfoSnapshot {
    let start = Instant::now();
    let has_unseen_output = pane.has_unseen_output();
    let _is_unresponsive_before = pane.is_unresponsive();
    let title = pane.get_title();
    let user_vars = pane.copy_user_vars();
    let _progress = pane.get_progress();
    let _cwd = pane.get_current_working_dir(CachePolicy::AllowStale);
    let elapsed = start.elapsed();
    let is_unresponsive_after = pane.is_unresponsive();
    PaneInfoSnapshot {
        has_unseen_output,
        is_unresponsive_after,
        title,
        user_vars,
        elapsed,
    }
}

// A watchdog bounds a broken implementation, not a performance assertion.
const DEADLOCK_WATCHDOG: Duration = Duration::from_secs(30);

#[test]
fn wedged_pane_does_not_block_healthy_pane_or_its_own_recovery() {
    let _mux_guard = super::MUX_TEST_GUARD.lock();
    let mux = Arc::new(crate::Mux::new(None));
    crate::Mux::set_mux(&mux);
    let (victim, victim_gate, victim_wrote, write_completed) = make_pane("victim");
    let (healthy, _, _, _) = make_pane("healthy");
    for (pane, title) in [(&victim, "victim-title"), (&healthy, "healthy-title")] {
        pane.set_title_for_test(title);
        assert_eq!(pane.get_title(), title);
    }
    victim.focus_changed(false);
    victim.increment_seqno_for_test();
    assert!(victim.has_unseen_output());
    assert!(!healthy.has_unseen_output());

    let (release, released) = std::sync::mpsc::sync_channel(1);
    let blocker = victim.spawn_terminal_lock_blocker(released);
    let writer_guard = victim_gate.lock();
    let (done_tx, done_rx) = std::sync::mpsc::sync_channel(1);
    let reader = {
        let victim = Arc::clone(&victim);
        let healthy = Arc::clone(&healthy);
        let victim_wrote = Arc::clone(&victim_wrote);
        std::thread::spawn(move || {
            let victim_snapshot = snapshot_pane_info(&victim);
            let healthy_snapshot = snapshot_pane_info(&healthy);
            let write = victim.writer().write_all(b"echo hello\n");
            let reached_writer = victim_wrote.load(Ordering::SeqCst);
            let _ = done_tx.send((victim_snapshot, healthy_snapshot, write, reached_writer));
        })
    };

    // Causal oracle: accessors and enqueue must finish BEFORE either lock is released.
    let before_release = done_rx.recv_timeout(DEADLOCK_WATCHDOG);
    drop(writer_guard);
    drop(release);
    blocker.join().expect("blocker thread panicked");
    reader.join().expect("pane read thread panicked");
    let (victim_snapshot, healthy_snapshot, write, reached_writer) =
        before_release.expect("pane accessors depended on releasing another thread's lock");

    eprintln!(
        "pane snapshots: victim={:?}, healthy={:?}",
        victim_snapshot.elapsed, healthy_snapshot.elapsed
    );
    assert!(victim_snapshot.has_unseen_output);
    assert_eq!(victim_snapshot.title, "victim-title");
    assert!(victim_snapshot.is_unresponsive_after);
    assert!(!healthy_snapshot.has_unseen_output);
    assert_eq!(healthy_snapshot.title, "healthy-title");
    assert!(!healthy_snapshot.is_unresponsive_after);
    assert!(healthy_snapshot.user_vars.is_empty());
    write.expect("write only enqueues onto WriterWrapper");
    assert!(
        !reached_writer,
        "underlying writer must still be blocked before release"
    );
    write_completed
        .recv_timeout(DEADLOCK_WATCHDOG)
        .expect("deferred write after gate release");

    let victim_after = snapshot_pane_info(&victim);
    assert_eq!(victim_after.title, "victim-title");
    assert!(!victim_after.is_unresponsive_after);
    let healthy_after = snapshot_pane_info(&healthy);
    assert_eq!(healthy_after.title, "healthy-title");
    assert!(!healthy_after.is_unresponsive_after);
    crate::Mux::shutdown();
}
