use super::*;
use crate::colorease::ColorEase;
use crate::frontend::front_end;
use crate::gui_api::guiwin::GuiWin;
use crate::inputmap::InputMap;
use crate::overlay::{
    launcher, start_overlay, CopyModeParams, CopyOverlay, LauncherArgs, LauncherFlags,
    QuickSelectOverlay,
};
use crate::spawn::SpawnWhere;
use crate::tabbar::TabBarState;
use crate::termwindow::background::reload_background_image;
use crate::termwindow::keyevent::{KeyTableArgs, KeyTableState};
use crate::termwindow::modal::Modal;
use anyhow::{anyhow, ensure};
use onlyterm_config::keyassignment::{
    ClipboardCopyDestination, Confirmation, KeyAssignment, LauncherActionArgs, PaneDirection,
    Pattern, PromptInputLine, QuickSelectArguments, SpawnCommand, SplitSize,
};
use onlyterm_config::window::WindowLevel;
use onlyterm_config::{configuration, TermConfig};
use onlyterm_mux::pane::{
    CachePolicy, Pane, PaneId, Pattern as MuxPattern, PerformAssignmentResult,
};
use onlyterm_mux::renderable::RenderableDimensions;
use onlyterm_mux::tab::{
    PositionedPane, PositionedSplit, SplitDirection, SplitRequest, SplitSize as MuxSplitSize, Tab,
    TabId,
};
use onlyterm_mux::Mux;
use onlyterm_mux_funcs::MuxPane;
use onlyterm_term::color::ColorPalette;
use onlyterm_term::{StableRowIndex, TerminalConfiguration, TerminalSize};
use smol::Timer;
use std::cell::RefMut;
use std::ops::Add;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Builds a key-down event that looks like a real hardware press of `key`
/// with `mods` held, for the key *assignments* that need to feed a keypress
/// to the pane's application (`CopySelectionOrInterrupt`,
/// `SendEnterOrNewline`, `SendChar`) rather than write a fixed byte.
///
/// Filling in the physical identity of the key -- `phys_code`, and the
/// Windows virtual-key and scan codes -- is load-bearing, not cosmetic:
/// `KeyEvent::encode_win32_input_mode` copies the latter two straight into
/// the `Vk`/`Sc` fields of the sequence it emits, and a record carrying
/// `Vk = 0` cannot be resolved back to a key by the receiver. ConPTY and
/// crossterm both recover the character by calling `ToUnicode(vk, sc, ..)`,
/// which fails outright for `vk == 0`, so such a keypress is dropped
/// silently and the application never sees it at all. That is precisely why
/// Ctrl+C (which hardcoded the real VK_C/scan pair) worked while Ctrl+J
/// (which passed zeros) did nothing whatsoever.
pub(crate) fn synthetic_key_down(key: KeyCode, mods: Modifiers) -> KeyEvent {
    let phys_code = key.to_phys();
    let (raw_code, _scan_code) = phys_code
        .and_then(|phys| phys.to_win32_key_codes())
        .unwrap_or((0, 0));

    KeyEvent {
        key: key.clone(),
        modifiers: mods,
        leds: KeyboardLedStatus::empty(),
        repeat_count: 1,
        key_is_down: true,
        raw: Some(RawKeyEvent {
            key,
            modifiers: mods,
            leds: KeyboardLedStatus::empty(),
            phys_code,
            raw_code,
            #[cfg(windows)]
            scan_code: _scan_code,
            repeat_count: 1,
            key_is_down: true,
            handled: Handled::new(),
        }),
        #[cfg(windows)]
        win32_uni_char: None,
    }
}

/// Minimum spacing between actual tab-bar/window-title rebuilds on the
/// coalesced paths.
///
/// A rebuild costs three bounded terminal.lock() acquisitions per pane
/// (see pos_pane_to_pane_info), each up to ~8ms on a pane whose pty
/// reader thread keeps the lock busy -- measured ~47ms per rebuild on a
/// contended pane.
///
/// Only the mux-notification paths are rate-limited, because only they
/// can storm: a full-screen TUI emits Alert::WindowTitleChanged /
/// TabTitleChanged / CurrentWorkingDirectoryChanged / Progress on every
/// repaint, and each one used to force a synchronous rebuild. Rebuilds
/// driven by a direct user action (keypress, focus change, hover, resize,
/// opening an overlay) stay immediate: they arrive at human speed, so
/// deferring them buys nothing and a deferred hover highlight or a tab
/// bar lagging the window edge during a resize drag is itself a visible
/// regression.
///
/// On the coalesced paths the first request after a quiet period applies
/// immediately (leading edge) and everything else within the interval
/// collapses into one trailing-edge rebuild, so a burst costs at most one
/// rebuild per interval plus exactly one final rebuild after it ends.
const TITLE_UPDATE_MIN_INTERVAL: Duration = Duration::from_millis(100);

/// Pure state machine for the title-rebuild rate limit, kept free of any
/// window/IO so the coalescing logic is unit-testable without a TermWindow.
#[derive(Default)]
pub(super) struct TitleUpdateCoalescer {
    /// When the last actual rebuild ran.
    last_run: Option<Instant>,
    /// Whether the single trailing-edge rebuild is armed. At most one is
    /// ever in flight: it observes the latest state when it fires, so
    /// later requests inside the same interval need no second timer.
    trailing_armed: bool,
}

impl TitleUpdateCoalescer {
    /// Record a request at `now`. Returns true when the caller should
    /// rebuild immediately (leading edge; also stamps last_run).
    fn should_run_now(&mut self, now: Instant, interval: Duration) -> bool {
        match self.last_run {
            Some(last) if now.duration_since(last) < interval => false,
            _ => {
                self.last_run = Some(now);
                true
            }
        }
    }

    /// Whether the caller must (re)arm the trailing-edge rebuild.
    /// True only for the first suppressed request since the last run.
    fn needs_trailing_arm(&mut self) -> bool {
        if self.trailing_armed {
            false
        } else {
            self.trailing_armed = true;
            true
        }
    }

    /// When the armed trailing rebuild is due.
    fn trailing_deadline(&self, interval: Duration) -> Instant {
        self.last_run
            .unwrap_or_else(Instant::now)
            .checked_add(interval)
            .unwrap_or_else(Instant::now)
    }

    /// The trailing rebuild fired: clears the armed flag and stamps
    /// last_run. The caller rebuilds unconditionally: some request was
    /// suppressed since the last run, and the guarantee being made is
    /// that the LAST request in a burst is always applied.
    fn trailing_fired(&mut self, now: Instant) {
        self.trailing_armed = false;
        self.last_run = Some(now);
    }
}

#[path = "actions/key_assignment.rs"]
mod key_assignment;
#[path = "actions/overlays.rs"]
mod overlays;
#[path = "actions/pane_actions.rs"]
mod pane_actions;
#[path = "actions/pane_layout.rs"]
mod pane_layout;
#[path = "actions/split_plan.rs"]
mod split_plan;
#[path = "actions/tabs.rs"]
mod tabs;
#[path = "actions/window_state.rs"]
mod window_state;

#[cfg(test)]
mod title_update_tests {
    use super::*;

    /// First request after startup applies immediately (should_run_now true;
    /// second call within interval false).
    #[test]
    fn test_first_request_immediate() {
        let mut coalescer = TitleUpdateCoalescer::default();
        let t0 = Instant::now();
        assert!(
            coalescer.should_run_now(t0, TITLE_UPDATE_MIN_INTERVAL),
            "first request should run immediately"
        );
        assert!(
            !coalescer.should_run_now(t0, TITLE_UPDATE_MIN_INTERVAL),
            "second request within interval should be suppressed"
        );
    }

    /// A burst coalesces: after a leading run, N suppressed requests ->
    /// needs_trailing_arm() returns true exactly once (first suppressed),
    /// false afterwards.
    #[test]
    fn test_burst_coalesces() {
        let mut coalescer = TitleUpdateCoalescer::default();
        let t0 = Instant::now();
        coalescer.should_run_now(t0, TITLE_UPDATE_MIN_INTERVAL);

        // First suppressed request arms trailing
        assert!(
            coalescer.needs_trailing_arm(),
            "first suppressed request should arm trailing"
        );
        // Subsequent suppressed requests don't re-arm
        assert!(
            !coalescer.needs_trailing_arm(),
            "second suppressed request should not re-arm"
        );
        assert!(
            !coalescer.needs_trailing_arm(),
            "third suppressed request should not re-arm"
        );
    }

    /// Trailing edge applies the last request: arm, then trailing_fired at
    /// t0+interval -> state allows an immediate rebuild again afterwards
    /// (next request after the interval applies immediately) and re-arms
    /// correctly for a new burst.
    #[test]
    fn test_trailing_edge_applies_last() {
        let mut coalescer = TitleUpdateCoalescer::default();
        let t0 = Instant::now();
        coalescer.should_run_now(t0, TITLE_UPDATE_MIN_INTERVAL);
        coalescer.needs_trailing_arm();

        let t1 = t0 + TITLE_UPDATE_MIN_INTERVAL;
        coalescer.trailing_fired(t1);

        // After trailing fired at t1, a request at t2 where t2 - t1 >= interval runs immediately
        let t2 = t1 + TITLE_UPDATE_MIN_INTERVAL;
        assert!(
            coalescer.should_run_now(t2, TITLE_UPDATE_MIN_INTERVAL),
            "request after trailing fired should run immediately"
        );
        // And can arm a new trailing
        assert!(
            coalescer.needs_trailing_arm(),
            "can re-arm after trailing fired and immediate run"
        );
    }

    /// After TITLE_UPDATE_MIN_INTERVAL has elapsed, a new request applies
    /// immediately.
    #[test]
    fn test_interval_expires() {
        let mut coalescer = TitleUpdateCoalescer::default();
        let t0 = Instant::now();
        coalescer.should_run_now(t0, TITLE_UPDATE_MIN_INTERVAL);
        coalescer.needs_trailing_arm();

        // Within interval, suppressed
        assert!(
            !coalescer.should_run_now(t0 + Duration::from_millis(50), TITLE_UPDATE_MIN_INTERVAL),
            "request within interval should be suppressed"
        );

        // After interval, immediate
        let t1 = t0 + TITLE_UPDATE_MIN_INTERVAL + Duration::from_millis(1);
        assert!(
            coalescer.should_run_now(t1, TITLE_UPDATE_MIN_INTERVAL),
            "request after interval should run immediately"
        );
    }

    /// trailing_deadline is >= the suppressed request's time (i.e. strictly in
    /// the future) and <= request_time + interval.
    #[test]
    fn test_trailing_deadline_bounds() {
        let mut coalescer = TitleUpdateCoalescer::default();
        let t0 = Instant::now();
        coalescer.should_run_now(t0, TITLE_UPDATE_MIN_INTERVAL);

        let request_time = t0 + Duration::from_millis(50);
        let deadline = coalescer.trailing_deadline(TITLE_UPDATE_MIN_INTERVAL);

        assert!(
            deadline > request_time,
            "deadline {} should be > request time {}",
            deadline.duration_since(Instant::now()).as_millis(),
            request_time.duration_since(Instant::now()).as_millis()
        );
        assert!(
            deadline <= t0 + TITLE_UPDATE_MIN_INTERVAL,
            "deadline should be <= t0 + interval"
        );
    }
}

#[cfg(test)]
mod title_latency_probe {
    /// Performance probe: measures the title/pane-info lock path of
    /// `update_title_impl` under contention, as a baseline for fixing T594.
    ///
    /// The test simulates a busy TUI by having a feeder thread apply a large
    /// `Action::PrintString` payload under `terminal.lock()` while the main
    /// thread calls the same bounded-lock methods used by title updates:
    /// `get_title()`, `get_progress()`, `get_current_working_dir()`, and
    /// `pos_pane_to_pane_info()`. Each accessor uses `try_lock_for(8ms)` with
    /// fallback to cached values when the terminal is contended; we measure
    /// how often and how long this timeout is hit.
    ///
    /// Read the "before"/"after" figures for what they are: this probe does
    /// NOT call `update_title_impl` and does NOT know which revision of the
    /// tree it was built from -- it replays the SHAPE of the two call
    /// sequences (active pane resolved twice per update, versus once and
    /// shared) against the same real contended pane, and prints both in the
    /// same run. Running it before and after the fix therefore yields the
    /// same numbers by construction. What ties the numbers to the fix is the
    /// code, not this test: `update_title_impl` now resolves the active
    /// pane once and hands it to `get_tab_information`.
    use super::*;
    use onlyterm_mux::localpane::LocalPane;
    use onlyterm_mux::pane::{CachePolicy, Pane, PaneId};
    use onlyterm_mux::tab::PositionedPane;
    use onlyterm_term::color::ColorPalette;
    use onlyterm_term::{TerminalConfiguration, TerminalSize};
    use portable_pty::{Child, ChildKiller, ExitStatus, MasterPty, PtySize};
    use std::io::{Read, Result as IoResult, Write};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    // === Test doubles (mirroring onlyterm_mux::localpane::tests) ===

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
        size: parking_lot::Mutex<PtySize>,
    }

    impl MasterPty for FakeMasterPty {
        fn resize(&self, new_size: PtySize) -> anyhow::Result<()> {
            *self.size.lock() = new_size;
            Ok(())
        }
        fn get_size(&self) -> anyhow::Result<PtySize> {
            Ok(*self.size.lock())
        }
        fn try_clone_reader(&self) -> anyhow::Result<Box<dyn Read + Send + 'static>> {
            Ok(Box::new(std::io::empty()))
        }
        fn take_writer(&self) -> anyhow::Result<Box<dyn Write + Send + 'static>> {
            Ok(Box::new(Vec::new()))
        }
    }

    #[derive(Debug)]
    struct TestTermConfig;
    impl TerminalConfiguration for TestTermConfig {
        fn color_palette(&self) -> ColorPalette {
            ColorPalette::default()
        }
    }

    // === Test infrastructure ===

    fn ensure_promise_scheduler() {
        use std::sync::Once;
        static ONCE: Once = Once::new();
        ONCE.call_once(|| {
            // Dropping runnables is intentional: LocalPane's alert handler
            // spawns into the main thread, and running it would call Mux::get()
            // which is not installed in tests.
            onlyterm_promise::spawn::set_schedulers(
                Box::new(|_runnable| {}),
                Box::new(|_runnable| {}),
            );
        });
    }

    const ROWS: usize = 24;
    const COLS: usize = 80;

    fn make_local_pane(pane_id: PaneId) -> Arc<LocalPane> {
        ensure_promise_scheduler();
        let size = TerminalSize {
            rows: ROWS,
            cols: COLS,
            pixel_width: COLS * 8,
            pixel_height: ROWS * 16,
            dpi: 0,
        };
        let terminal = onlyterm_term::Terminal::new(
            size,
            Arc::new(TestTermConfig),
            "OnlyTerm",
            "0.0.0",
            Box::new(Vec::new()),
        );
        let pty = Box::new(FakeMasterPty {
            size: parking_lot::Mutex::new(PtySize {
                rows: ROWS as u16,
                cols: COLS as u16,
                pixel_width: 0,
                pixel_height: 0,
            }),
        });
        let writer = Box::new(Vec::new());
        Arc::new(LocalPane::new(
            pane_id,
            terminal,
            Box::new(NeverExitChild),
            pty,
            writer,
            1,
            "perf-probe".to_string(),
            None,
        ))
    }

    fn pos_for(pane: &Arc<LocalPane>, is_active: bool) -> PositionedPane {
        PositionedPane {
            index: 0,
            is_active,
            is_zoomed: false,
            left: 0,
            top: 0,
            width: COLS,
            height: ROWS,
            pixel_width: COLS * 8,
            pixel_height: ROWS * 16,
            pane: Arc::clone(pane) as Arc<dyn Pane>,
        }
    }

    fn median(durs: &[Duration]) -> Duration {
        if durs.is_empty() {
            return Duration::ZERO;
        }
        let mut sorted = durs.to_vec();
        sorted.sort();
        sorted[sorted.len() / 2]
    }

    fn ms(d: Duration) -> f64 {
        d.as_secs_f64() * 1000.0
    }

    fn min_duration(durs: &[Duration]) -> Duration {
        durs.iter()
            .copied()
            .reduce(|a, b| a.min(b))
            .unwrap_or(Duration::ZERO)
    }

    fn max_duration(durs: &[Duration]) -> Duration {
        durs.iter()
            .copied()
            .reduce(|a, b| a.max(b))
            .unwrap_or(Duration::ZERO)
    }

    #[test]
    #[ignore = "manual perf probe; run with: cargo test -p onlyterm-gui title_latency -- --ignored --nocapture"]
    fn measure_title_lock_path_under_contention() {
        ensure_promise_scheduler();

        // Create active pane and idle panes
        let active = make_local_pane(1);
        let idle1 = make_local_pane(2);
        let idle2 = make_local_pane(3);
        let idle3 = make_local_pane(4);

        let pos_active = pos_for(&active, true);
        let pos_idle1 = pos_for(&idle1, true);
        let pos_idle2 = pos_for(&idle2, true);
        let pos_idle3 = pos_for(&idle3, true);

        println!("\n=== UNCONTENDED BASELINE ===");
        let mut baseline_title = Vec::with_capacity(20);
        let mut baseline_progress = Vec::with_capacity(20);
        let mut baseline_cwd = Vec::with_capacity(20);
        let mut baseline_pos_to_info = Vec::with_capacity(20);

        for _ in 0..20 {
            let start = Instant::now();
            active.get_title();
            baseline_title.push(start.elapsed());

            let start = Instant::now();
            active.get_progress();
            baseline_progress.push(start.elapsed());

            let start = Instant::now();
            active.get_current_working_dir(CachePolicy::AllowStale);
            baseline_cwd.push(start.elapsed());

            let start = Instant::now();
            TermWindow::pos_pane_to_pane_info(&pos_active);
            baseline_pos_to_info.push(start.elapsed());
        }

        println!(
            "get_title: min={:.4}ms median={:.4}ms max={:.4}ms",
            ms(min_duration(&baseline_title)),
            ms(median(&baseline_title)),
            ms(max_duration(&baseline_title))
        );
        println!(
            "get_progress: min={:.4}ms median={:.4}ms max={:.4}ms",
            ms(min_duration(&baseline_progress)),
            ms(median(&baseline_progress)),
            ms(max_duration(&baseline_progress))
        );
        println!(
            "get_cwd: min={:.4}ms median={:.4}ms max={:.4}ms",
            ms(min_duration(&baseline_cwd)),
            ms(median(&baseline_cwd)),
            ms(max_duration(&baseline_cwd))
        );
        println!(
            "pos_pane_to_pane_info: min={:.4}ms median={:.4}ms max={:.4}ms",
            ms(min_duration(&baseline_pos_to_info)),
            ms(median(&baseline_pos_to_info)),
            ms(max_duration(&baseline_pos_to_info))
        );

        // === CALIBRATION: measure processing rate to size payload ===
        println!("\n=== CALIBRATION ===");
        let calib_start = Instant::now();
        active.perform_actions(vec![termwiz::escape::Action::PrintString(
            "X".repeat(1 << 20),
        )]);
        let calib_dur = calib_start.elapsed();
        let rate_bytes_per_sec = (1 << 20) as f64 / calib_dur.as_secs_f64();
        println!(
            "Calibration: processed 1MB in {:.2}s => {:.2} MB/s",
            calib_dur.as_secs_f64(),
            rate_bytes_per_sec / 1_048_576.0
        );

        // Target ~6 second hold, clamped to reasonable bounds
        let target_bytes = ((rate_bytes_per_sec * 6.0) as usize).clamp(4 << 20, 96 << 20);
        println!("Chosen payload size: {} MB", target_bytes >> 20);

        // === FEEDER: spawn thread to hold lock with large payload ===
        let active_feeder = Arc::clone(&active);
        let payload = "X".repeat(target_bytes);
        let feeder_handle = std::thread::spawn(move || {
            let start = Instant::now();
            active_feeder.perform_actions(vec![termwiz::escape::Action::PrintString(payload)]);
            start.elapsed()
        });

        // Give feeder time to acquire lock
        std::thread::sleep(Duration::from_millis(300));

        // === CONTENDED MEASUREMENTS ===
        println!("\n=== CONTENDED ACCESSORS ===");

        let mut title_times = Vec::with_capacity(20);
        let mut progress_times = Vec::with_capacity(20);
        let mut cwd_times = Vec::with_capacity(20);
        let mut title_unresponsive = 0;
        let mut progress_unresponsive = 0;
        let mut cwd_unresponsive = 0;

        for _ in 0..20 {
            let start = Instant::now();
            active.get_title();
            let dur = start.elapsed();
            title_times.push(dur);
            if active.is_unresponsive() {
                title_unresponsive += 1;
            }

            let start = Instant::now();
            active.get_progress();
            let dur = start.elapsed();
            progress_times.push(dur);
            if active.is_unresponsive() {
                progress_unresponsive += 1;
            }

            let start = Instant::now();
            active.get_current_working_dir(CachePolicy::AllowStale);
            let dur = start.elapsed();
            cwd_times.push(dur);
            if active.is_unresponsive() {
                cwd_unresponsive += 1;
            }
        }

        println!(
            "get_title: min={:.4}ms median={:.4}ms max={:.4}ms, unresponsive={}/20 ({:.0}%)",
            ms(min_duration(&title_times)),
            ms(median(&title_times)),
            ms(max_duration(&title_times)),
            title_unresponsive,
            (title_unresponsive as f64 / 20.0) * 100.0
        );
        println!(
            "get_progress: min={:.4}ms median={:.4}ms max={:.4}ms, unresponsive={}/20 ({:.0}%)",
            ms(min_duration(&progress_times)),
            ms(median(&progress_times)),
            ms(max_duration(&progress_times)),
            progress_unresponsive,
            (progress_unresponsive as f64 / 20.0) * 100.0
        );
        println!(
            "get_cwd: min={:.4}ms median={:.4}ms max={:.4}ms, unresponsive={}/20 ({:.0}%)",
            ms(min_duration(&cwd_times)),
            ms(median(&cwd_times)),
            ms(max_duration(&cwd_times)),
            cwd_unresponsive,
            (cwd_unresponsive as f64 / 20.0) * 100.0
        );

        println!("\n=== CONTENDED pos_pane_to_pane_info ===");
        let mut pos_times = Vec::with_capacity(20);
        for _ in 0..20 {
            let start = Instant::now();
            TermWindow::pos_pane_to_pane_info(&pos_active);
            pos_times.push(start.elapsed());
        }
        println!(
            "pos_pane_to_pane_info: min={:.4}ms median={:.4}ms max={:.4}ms",
            ms(min_duration(&pos_times)),
            ms(median(&pos_times)),
            ms(max_duration(&pos_times))
        );

        // === SIMULATED FULL UPDATE_TITLE_IMPL PATHS ===
        println!("\n=== BEFORE/AFTER FULL UPDATE PATHS ===");

        for &t in &[1, 4] {
            let idle_panes = match t {
                1 => vec![],
                4 => vec![&pos_idle1, &pos_idle2, &pos_idle3],
                _ => unreachable!(),
            };

            // BEFORE: active pane computed twice
            let mut before_times = Vec::with_capacity(20);
            for _ in 0..20 {
                let start = Instant::now();
                // Other tabs' active panes (uncontended)
                for idle_pos in &idle_panes {
                    TermWindow::pos_pane_to_pane_info(idle_pos);
                }
                // Active pane for tab list
                TermWindow::pos_pane_to_pane_info(&pos_active);
                // Active pane for pane list (computed again)
                TermWindow::pos_pane_to_pane_info(&pos_active);
                before_times.push(start.elapsed());
            }

            // AFTER: active pane computed once, shared
            let mut after_times = Vec::with_capacity(20);
            for _ in 0..20 {
                let start = Instant::now();
                // Other tabs' active panes (uncontended)
                for idle_pos in &idle_panes {
                    TermWindow::pos_pane_to_pane_info(idle_pos);
                }
                // Active pane computed once, shared
                TermWindow::pos_pane_to_pane_info(&pos_active);
                after_times.push(start.elapsed());
            }

            println!(
                "T={}: before median={:.4}ms, after median={:.4}ms, speedup={:.2}x",
                t,
                ms(median(&before_times)),
                ms(median(&after_times)),
                ms(median(&before_times)) / ms(median(&after_times))
            );
        }

        // === FINAL SUMMARY ===
        println!("\n=== SUMMARY ===");
        let before_1tab = {
            let mut times = Vec::with_capacity(20);
            for _ in 0..20 {
                let start = Instant::now();
                TermWindow::pos_pane_to_pane_info(&pos_active);
                TermWindow::pos_pane_to_pane_info(&pos_active);
                times.push(start.elapsed());
            }
            median(&times)
        };

        let after_1tab = {
            let mut times = Vec::with_capacity(20);
            for _ in 0..20 {
                let start = Instant::now();
                TermWindow::pos_pane_to_pane_info(&pos_active);
                times.push(start.elapsed());
            }
            median(&times)
        };

        let before_4tab = {
            let idle_panes = vec![&pos_idle1, &pos_idle2, &pos_idle3];
            let mut times = Vec::with_capacity(20);
            for _ in 0..20 {
                let start = Instant::now();
                for idle_pos in &idle_panes {
                    TermWindow::pos_pane_to_pane_info(idle_pos);
                }
                TermWindow::pos_pane_to_pane_info(&pos_active);
                TermWindow::pos_pane_to_pane_info(&pos_active);
                times.push(start.elapsed());
            }
            median(&times)
        };

        let after_4tab = {
            let idle_panes = vec![&pos_idle1, &pos_idle2, &pos_idle3];
            let mut times = Vec::with_capacity(20);
            for _ in 0..20 {
                let start = Instant::now();
                for idle_pos in &idle_panes {
                    TermWindow::pos_pane_to_pane_info(idle_pos);
                }
                TermWindow::pos_pane_to_pane_info(&pos_active);
                times.push(start.elapsed());
            }
            median(&times)
        };

        println!("1 tab (before):  {:.4}ms", ms(before_1tab));
        println!("1 tab (after):   {:.4}ms", ms(after_1tab));
        println!("4 tabs (before): {:.4}ms", ms(before_4tab));
        println!("4 tabs (after):  {:.4}ms", ms(after_4tab));

        // === JOIN FEEDER ===
        let feeder_hold_dur = feeder_handle.join().unwrap();
        println!(
            "\nFeeder held lock for {:.2}s",
            feeder_hold_dur.as_secs_f64()
        );

        // === SANITY ASSERTIONS ===
        assert!(
            feeder_hold_dur.as_secs_f64() >= 4.0,
            "feeder hold too short: {:.2}s",
            feeder_hold_dur.as_secs_f64()
        );

        let contended_pos_median = median(&pos_times);
        assert!(
            contended_pos_median >= Duration::from_millis(20),
            "contended pos_pane_to_pane_info median too low: {:.2}ms",
            ms(contended_pos_median)
        );

        assert!(
            median(&title_times) >= Duration::from_millis(7),
            "contended get_title median too low: {:.2}ms",
            ms(median(&title_times))
        );
        assert!(
            median(&progress_times) >= Duration::from_millis(7),
            "contended get_progress median too low: {:.2}ms",
            ms(median(&progress_times))
        );
        assert!(
            median(&cwd_times) >= Duration::from_millis(7),
            "contended get_cwd median too low: {:.2}ms",
            ms(median(&cwd_times))
        );

        let uncontended_pos_median = median(&baseline_pos_to_info);
        assert!(
            uncontended_pos_median <= Duration::from_millis(50),
            "uncontended pos_pane_to_pane_info median too high: {:.2}ms",
            ms(uncontended_pos_median)
        );

        let total_unresponsive = title_unresponsive + progress_unresponsive + cwd_unresponsive;
        let total_calls = 60;
        assert!(
            (total_unresponsive as f64 / total_calls as f64) >= 0.9,
            "unresponsive calls fraction too low: {}/60 ({:.0}%)",
            total_unresponsive,
            (total_unresponsive as f64 / total_calls as f64) * 100.0
        );
    }
}
