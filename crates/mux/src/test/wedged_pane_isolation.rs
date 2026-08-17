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
//! `wezterm-gui`'s `TermWindow::pos_pane_to_pane_info`
//! (`crates/wezterm-gui/src/termwindow/mod.rs`) performs against every
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
use parking_lot::Mutex;
use portable_pty::{Child, ChildKiller, ExitStatus, MasterPty, PtySize};
use std::io::{Read, Result as IoResult, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use wezterm_term::color::ColorPalette;
use wezterm_term::{Terminal, TerminalConfiguration, TerminalSize};

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
/// call sites in `wezterm-gui` use (paste, `SendString`, ...).
struct BlockingWriter {
    gate: Arc<Mutex<()>>,
    wrote: Arc<AtomicBool>,
}

impl Write for BlockingWriter {
    fn write(&mut self, buf: &[u8]) -> IoResult<usize> {
        let _guard = self.gate.lock();
        self.wrote.store(true, Ordering::SeqCst);
        Ok(buf.len())
    }
    fn flush(&mut self) -> IoResult<()> {
        Ok(())
    }
}

#[derive(Debug)]
struct TestConfig;
impl TerminalConfiguration for TestConfig {
    fn color_palette(&self) -> ColorPalette {
        ColorPalette::default()
    }
}

const ROWS: usize = 24;
const COLS: usize = 80;

/// Builds a real `LocalPane` with a real `wezterm_term::Terminal` behind
/// the exact same `Mutex` used in production, and a real
/// `crate::domain::WriterWrapper` (task #245) in front of a
/// `BlockingWriter`, so `pane.writer().write_all()` exercises the actual
/// non-blocking machinery rather than a plain `Vec` sink.
fn make_pane(name: &str) -> (Arc<LocalPane>, Arc<Mutex<()>>, Arc<AtomicBool>) {
    let size = TerminalSize {
        rows: ROWS,
        cols: COLS,
        pixel_width: COLS * 8,
        pixel_height: ROWS * 16,
        dpi: 0,
    };
    let gate = Arc::new(Mutex::new(()));
    let wrote = Arc::new(AtomicBool::new(false));
    let writer = crate::domain::WriterWrapper::new(Box::new(BlockingWriter {
        gate: Arc::clone(&gate),
        wrote: Arc::clone(&wrote),
    }));
    let terminal = Terminal::new_with_nonblocking_writer(
        size,
        Arc::new(TestConfig),
        "WezTerm",
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
    (pane, gate, wrote)
}

/// Snapshot of everything `TermWindow::pos_pane_to_pane_info`
/// (`crates/wezterm-gui/src/termwindow/mod.rs`) reads off a pane on the
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

/// Generous bound for "promptly" in this test: well under a second, in
/// line with the existing per-accessor tests
/// (`has_unseen_output_does_not_block_on_a_locked_terminal` et al. all use
/// a 1s ceiling), but loose enough not to be scheduler-jitter-flaky on a
/// loaded CI/dev machine.
const PROMPT_BOUND: Duration = Duration::from_secs(1);

/// The end-to-end claim this task exists to prove: with the VICTIM pane's
/// `terminal.lock()` held by another thread for the whole test (simulating
/// a wedged terminal -- see the module doc comment for why this technique
/// is a faithful, deterministic stand-in for a real hung pty/process), (1)
/// every GUI-thread-reachable accessor on the victim itself still returns
/// promptly (bounded fallback behavior, tasks #244/#246/#248), (2) the
/// SAME accessors on a HEALTHY, unrelated pane in the "same window" are
/// completely unaffected -- correct data, no slowdown at all -- and (3) a
/// paste/`SendString`-style `pane.writer().write_all()` against the victim
/// also does not block (task #245's writer thread is independent of
/// `terminal.lock()`). Finally, once the wedge is released, both panes are
/// confirmed to still work correctly, proving the timeout/fallback paths
/// didn't leave anything corrupted.
#[test]
fn wedged_pane_does_not_block_healthy_pane_or_its_own_recovery() {
    let _mux_guard = super::MUX_TEST_GUARD.lock();
    let mux = Arc::new(crate::Mux::new(None));
    crate::Mux::set_mux(&mux);

    let (victim, victim_gate, victim_wrote) = make_pane("victim");
    let (healthy, _healthy_gate, _healthy_wrote) = make_pane("healthy");

    // Give each pane a known-good, distinguishable title and a user var,
    // exactly like `get_title_does_not_block_on_a_locked_terminal` does,
    // so this test can assert on *correct*, pane-specific data -- not
    // just "didn't block" -- both during contention and after recovery.
    for (pane, title) in [(&victim, "victim-title"), (&healthy, "healthy-title")] {
        pane.set_title_for_test(title);
        assert_eq!(pane.get_title(), title);
    }

    // Bump the victim's seqno so it has genuine unseen output once it
    // loses focus, mirroring `has_unseen_output_does_not_block_on_a_locked_terminal`'s
    // setup: this is the real signal `has_unseen_output()` reads, not
    // just a hardcoded expectation.
    victim.focus_changed(false);
    victim.increment_seqno_for_test();
    assert!(victim.has_unseen_output());
    // The healthy pane stays focused throughout: no unseen output.
    assert!(!healthy.has_unseen_output());

    // --- Wedge the victim's terminal: a real thread holds terminal.lock()
    // indefinitely, exactly the technique used by
    // `localpane::tests::has_unseen_output_does_not_block_on_a_locked_terminal`
    // / `get_title_does_not_block_on_a_locked_terminal`. ---
    let blocker_release = Arc::new(AtomicBool::new(false));
    let blocker_handle = victim.spawn_terminal_lock_blocker(Arc::clone(&blocker_release));

    // --- Also wedge the victim's writer, standing in for a full/unread
    // stdin pipe (task #237's original manual scenario): any write into
    // `BlockingWriter` blocks until the test releases `victim_gate`. ---
    let writer_guard = victim_gate.lock();

    // (1) Victim's own GUI-thread accessors must return promptly, serving
    // bounded-fallback / last-known-good data instead of blocking.
    let victim_snapshot = snapshot_pane_info(&victim);
    assert!(
        victim_snapshot.elapsed < PROMPT_BOUND,
        "victim pane's pos_pane_to_pane_info-equivalent read batch took {:?} \
         while its terminal.lock() was held by another thread -- it must \
         return within the bounded-fallback window instead of blocking",
        victim_snapshot.elapsed,
    );
    assert!(
        victim_snapshot.has_unseen_output,
        "has_unseen_output() is lock-free (task #244) and must still \
         report the real, already-published state while the lock is held"
    );
    assert_eq!(
        victim_snapshot.title, "victim-title",
        "get_title() must fall back to the last known-good cached title \
         (task #246) rather than blocking or returning garbage"
    );
    assert!(
        victim_snapshot.is_unresponsive_after,
        "is_unresponsive() must flip true (task #248) once the bounded \
         terminal.lock() attempt inside get_title()/copy_user_vars()/etc. \
         above timed out"
    );

    // (2) THE CORE CLAIM: the healthy, unrelated pane's own accessors are
    // completely unaffected -- correct data, and no slowdown at all, even
    // though the victim's terminal is wedged concurrently. This is the
    // property no single existing per-accessor test proves on its own,
    // since each of those drives only one pane.
    let healthy_snapshot = snapshot_pane_info(&healthy);
    assert!(
        healthy_snapshot.elapsed < PROMPT_BOUND,
        "healthy pane's read batch took {:?} even though only the OTHER \
         (victim) pane's terminal was wedged -- panes must not contend \
         with each other's terminal locks",
        healthy_snapshot.elapsed,
    );
    assert!(
        !healthy_snapshot.has_unseen_output,
        "healthy pane's has_unseen_output() must reflect its own real \
         state (still focused, no new output), unaffected by the victim"
    );
    assert_eq!(
        healthy_snapshot.title, "healthy-title",
        "healthy pane's get_title() must return its own live title, not \
         be corrupted or delayed by the victim pane's wedged lock"
    );
    assert!(
        !healthy_snapshot.is_unresponsive_after,
        "the healthy pane's own terminal.lock() was never contended, so \
         it must never have been marked unresponsive"
    );
    assert!(healthy_snapshot.user_vars.is_empty());

    // (3) A paste/SendString-style write against the VICTIM must also not
    // block, even though both its terminal.lock() AND its writer's
    // underlying pipe are currently wedged. Task #245's WriterWrapper
    // background thread is independent of terminal.lock(), so this
    // exercises that independence explicitly as part of the combined
    // scenario rather than trusting it from #245's own separate test.
    let write_start = Instant::now();
    victim
        .writer()
        .write_all(b"echo hello\n")
        .expect("write_all must succeed (it only enqueues onto WriterWrapper's background thread)");
    let write_elapsed = write_start.elapsed();
    assert!(
        write_elapsed < PROMPT_BOUND,
        "pane.writer().write_all() on the victim pane took {:?} while both \
         its terminal.lock() and its underlying writer were wedged -- the \
         writer path (task #245) must be fully independent of terminal.lock()",
        write_elapsed,
    );
    assert!(
        !victim_wrote.load(Ordering::SeqCst),
        "the write is still blocked behind the writer gate, so it must \
         not have reached the underlying BlockingWriter yet"
    );

    // --- Release both wedges. ---
    drop(writer_guard);
    blocker_release.store(true, Ordering::SeqCst);
    blocker_handle.join().expect("blocker thread panicked");

    // Give the deferred write a bounded window to actually land on the
    // real (now-unblocked) writer.
    let deadline = Instant::now() + Duration::from_secs(5);
    while !victim_wrote.load(Ordering::SeqCst) {
        assert!(
            Instant::now() < deadline,
            "the victim pane's deferred write never reached the \
             underlying writer after the gate was released"
        );
        std::thread::sleep(Duration::from_millis(10));
    }

    // (4) Post-recovery correctness: both panes must still report
    // accurate, un-corrupted data, and the victim must clear its
    // unresponsive flag now that its terminal.lock() is free again --
    // nothing was silently left in a bad state by the timeout/fallback
    // paths exercised above.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if !victim.is_unresponsive() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "victim.is_unresponsive() never cleared after the wedge was released"
        );
        let _ = victim.get_title();
        std::thread::sleep(Duration::from_millis(10));
    }

    let victim_after = snapshot_pane_info(&victim);
    assert!(victim_after.elapsed < PROMPT_BOUND);
    assert_eq!(victim_after.title, "victim-title");
    assert!(
        !victim_after.is_unresponsive_after,
        "victim pane must report responsive again once its terminal.lock() \
         is free"
    );

    let healthy_after = snapshot_pane_info(&healthy);
    assert!(healthy_after.elapsed < PROMPT_BOUND);
    assert_eq!(healthy_after.title, "healthy-title");
    assert!(!healthy_after.is_unresponsive_after);

    crate::Mux::shutdown();
}
