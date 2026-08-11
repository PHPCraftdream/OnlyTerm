//! GUI message-loop watchdog.
//!
//! `run_message_loop()` in `connection.rs` is the single `PeekMessageW`/
//! `DispatchMessageW` loop that services every window in the process, and
//! `WM_PAINT` handling runs synchronously inside it. If a paint ever blocks
//! forever (eg. a GPU driver stuck in `Present`/`SwapBuffers`), the entire
//! loop stops pumping messages: no window in the process can repaint,
//! accept input, or even process its close button.
//!
//! This module gives that failure mode visibility. The message loop bumps a
//! monotonic tick counter on every iteration; a background thread polls that
//! counter and, if it stops advancing for longer than a configurable
//! threshold, logs an error, records a metric, and flips a process-wide
//! `AtomicBool` that other subsystems can consult (eg. the future per-tab
//! supervisor in task #223) without needing to know anything about this
//! watchdog's internals.
//!
//! Deliberately NOT touched: the message loop itself. This module only reads
//! a counter it doesn't own the pacing of and increments state elsewhere.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::Duration;

/// Bumped once per message-loop iteration in `run_message_loop()`.
/// Using a plain counter (rather than a timestamp) keeps the hot-path write
/// to a single relaxed fetch_add with no clock syscall.
static HEARTBEAT_TICKS: AtomicU64 = AtomicU64::new(0);

/// Set to true once the watchdog believes the GUI thread is stuck; cleared
/// again as soon as the heartbeat resumes advancing. Readable from any
/// thread so other subsystems can make decisions based on it.
static GUI_THREAD_HUNG: AtomicBool = AtomicBool::new(false);

/// Set for the duration of `wait_message()` (Win32 `MsgWaitForMultipleObjects`
/// with an empty queue) so the watchdog can tell "parked, nothing to do"
/// apart from "stuck inside `DispatchMessageW`". The heartbeat only ticks
/// once per loop iteration, right before this wait, so without this flag a
/// long-but-legitimate idle period looks identical to a real stall.
static GUI_IDLE_WAIT: AtomicBool = AtomicBool::new(false);

/// Ensures the watchdog thread is only ever spawned once per process.
static WATCHDOG_STARTED: std::sync::Once = std::sync::Once::new();

/// Optional callback invoked (from the watchdog's own background thread,
/// never the GUI thread) the moment a hang is first detected. `window` has
/// no dependency on any notification/UI crate, so it can't show a toast
/// itself; instead it exposes this hook and leaves the decision of *how* to
/// surface the hang to whichever higher-level crate wires it up (see
/// `set_gui_hang_callback`).
static GUI_HANG_CALLBACK: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

/// Registers a callback to be run on the watchdog thread the instant a GUI
/// thread hang is first detected (edge-triggered: fires once per
/// hang-then-recovery cycle, not once per poll while still hung). Intended
/// to be called once, early during process startup (eg. alongside
/// `notify_on_panic()` in `wezterm-gui`'s `main()`), by a crate that can
/// actually show the user something (`wezterm-gui` already depends on
/// `wezterm-toast-notification`; `window` deliberately does not, to keep
/// this low-level crate decoupled from notification-backend concerns).
///
/// Only the first registration wins; later calls are silently ignored (the
/// process only has one GUI thread to watch, so only one callback should
/// ever need to be registered).
pub fn set_gui_hang_callback(cb: impl Fn() + Send + Sync + 'static) {
    if GUI_HANG_CALLBACK.set(Box::new(cb)).is_err() {
        log::warn!("gui-watchdog: set_gui_hang_callback called more than once; ignoring");
    }
}

/// Call once per iteration of the message loop, ideally right before
/// blocking/waiting so a slow-but-alive loop still advances the counter
/// regularly. Cheap: a single relaxed atomic increment.
pub(crate) fn record_heartbeat() {
    HEARTBEAT_TICKS.fetch_add(1, Ordering::Relaxed);
}

/// Marks the message loop as entering/leaving its blocking wait for the next
/// message. Call immediately around `wait_message()` -- see `GUI_IDLE_WAIT`.
pub(crate) fn begin_idle_wait() {
    GUI_IDLE_WAIT.store(true, Ordering::Release);
}

pub(crate) fn end_idle_wait() {
    GUI_IDLE_WAIT.store(false, Ordering::Release);
}

/// Returns true if the watchdog currently believes the GUI thread's message
/// loop is stuck (hasn't advanced its heartbeat for longer than the
/// configured threshold). Intended for other subsystems (eg. a future tab
/// supervisor) to check before taking drastic action.
pub fn gui_thread_is_hung() -> bool {
    GUI_THREAD_HUNG.load(Ordering::Relaxed)
}

/// Spawns the background watchdog thread the first time it is called; safe
/// to call repeatedly (eg. once per `Connection::create_new()`). Reads
/// `gui_watchdog_enabled` / `gui_watchdog_threshold_ms` from the live config
/// each time it wakes up, so toggling the setting takes effect without a
/// restart.
pub(crate) fn spawn_watchdog_thread() {
    WATCHDOG_STARTED.call_once(|| {
        std::thread::Builder::new()
            .name("gui-watchdog".to_string())
            .spawn(watchdog_loop)
            .expect("failed to spawn gui-watchdog thread");
    });
}

fn watchdog_loop() {
    let mut last_seen = HEARTBEAT_TICKS.load(Ordering::Relaxed);
    let mut last_change = std::time::Instant::now();

    loop {
        let config = config::configuration();
        let enabled = config.gui_watchdog_enabled;
        let threshold = Duration::from_millis(config.gui_watchdog_threshold_ms.max(1));

        // Poll at a fraction of the threshold so we notice a stall promptly
        // without busy-looping.
        let poll_interval = (threshold / 4).max(Duration::from_millis(50));
        let slept_at = std::time::Instant::now();
        std::thread::sleep(poll_interval);

        if !enabled {
            // Reset state so a stale "hung" flag doesn't linger if the
            // watchdog is disabled mid-hang, and re-sync the baseline so
            // re-enabling later doesn't immediately fire on stale data.
            last_seen = HEARTBEAT_TICKS.load(Ordering::Relaxed);
            last_change = std::time::Instant::now();
            if GUI_THREAD_HUNG.swap(false, Ordering::Relaxed) {
                log::info!("gui-watchdog: disabled while hung state was set; clearing");
            }
            continue;
        }

        // The whole process (this watchdog thread included) can be
        // suspended wholesale by the OS -- sleep, hibernate, a paused VM.
        // On resume, `std::thread::sleep` returns having "slept" far longer
        // than requested even though nothing was actually stuck; read
        // naively that gap would be misattributed to the GUI thread the
        // instant we wake up. Re-baseline instead of accusing it.
        if slept_at.elapsed() > poll_interval * 4 {
            last_seen = HEARTBEAT_TICKS.load(Ordering::Relaxed);
            last_change = std::time::Instant::now();
            continue;
        }

        if GUI_IDLE_WAIT.load(Ordering::Acquire) {
            // Parked in `wait_message()` with an empty queue: Windows
            // itself considers this window fully responsive, it simply has
            // nothing to do. Don't let the heartbeat's silence during a
            // long-but-legitimate idle period read as a stall.
            last_seen = HEARTBEAT_TICKS.load(Ordering::Relaxed);
            last_change = std::time::Instant::now();
            if GUI_THREAD_HUNG.swap(false, Ordering::Relaxed) {
                log::warn!("gui-watchdog: GUI thread heartbeat resumed; no longer hung");
                metrics::counter!("gui.watchdog.recovered").increment(1);
            }
            continue;
        }

        let current = HEARTBEAT_TICKS.load(Ordering::Relaxed);
        if current != last_seen {
            last_seen = current;
            last_change = std::time::Instant::now();
            if GUI_THREAD_HUNG.swap(false, Ordering::Relaxed) {
                log::warn!("gui-watchdog: GUI thread heartbeat resumed; no longer hung");
                metrics::counter!("gui.watchdog.recovered").increment(1);
            }
            continue;
        }

        let stalled_for = last_change.elapsed();
        if stalled_for >= threshold {
            if !GUI_THREAD_HUNG.swap(true, Ordering::Relaxed) {
                log::error!(
                    "gui-watchdog: GUI thread message loop appears hung: \
                     heartbeat has not advanced for {:?} (threshold {:?})",
                    stalled_for,
                    threshold
                );
                metrics::counter!("gui.watchdog.hang_detected").increment(1);
                // Take user-visible action right here, on this background
                // thread. This is deliberate: if the GUI thread's message
                // loop is genuinely stuck, nothing that needs the GUI
                // thread to run (eg. anything scheduled via
                // `promise::spawn`, which is GUI-thread-only) can ever fire
                // until the loop frees up again -- so this watchdog thread
                // is the only place that can react *while* the hang is
                // still happening. No "recovered" toast is fired
                // symmetrically below: by the time recovery is detected the
                // freeze is already over and the user has already noticed
                // (the window was frozen), so a second toast would just be
                // noise on top of the log line + metric that already cover
                // it.
                if let Some(cb) = GUI_HANG_CALLBACK.get() {
                    cb();
                }
            }
            metrics::histogram!("gui.watchdog.stall_duration").record(stalled_for);
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    /// Minimal standalone re-implementation of the polling/decision logic so
    /// we can drive it with a fake, test-controlled heartbeat and a short
    /// threshold, without waiting on real wall-clock seconds or touching the
    /// process-global statics (which are shared with any real watchdog
    /// thread that might be running in this test binary).
    struct TestWatchdog {
        heartbeat: AtomicU64,
        hung: AtomicBool,
    }

    impl TestWatchdog {
        fn new() -> Self {
            Self {
                heartbeat: AtomicU64::new(0),
                hung: AtomicBool::new(false),
            }
        }

        fn tick(&self) {
            self.heartbeat.fetch_add(1, Ordering::Relaxed);
        }

        /// Runs one evaluation step; mirrors `watchdog_loop`'s decision
        /// logic for a single (last_seen, last_change) pair. `idle` mirrors
        /// a `GUI_IDLE_WAIT` read: when true, a stalled heartbeat is treated
        /// as legitimate idle time rather than a hang.
        fn evaluate(
            &self,
            last_seen: &mut u64,
            last_change: &mut std::time::Instant,
            threshold: Duration,
            idle: bool,
        ) {
            if idle {
                *last_seen = self.heartbeat.load(Ordering::Relaxed);
                *last_change = std::time::Instant::now();
                self.hung.store(false, Ordering::Relaxed);
                return;
            }
            let current = self.heartbeat.load(Ordering::Relaxed);
            if current != *last_seen {
                *last_seen = current;
                *last_change = std::time::Instant::now();
                self.hung.store(false, Ordering::Relaxed);
                return;
            }
            if last_change.elapsed() >= threshold {
                self.hung.store(true, Ordering::Relaxed);
            }
        }

        fn is_hung(&self) -> bool {
            self.hung.load(Ordering::Relaxed)
        }
    }

    #[test]
    fn detects_stalled_heartbeat_after_threshold() {
        let watchdog = TestWatchdog::new();
        let threshold = Duration::from_millis(50);
        let mut last_seen = watchdog.heartbeat.load(Ordering::Relaxed);
        let mut last_change = std::time::Instant::now();

        // Heartbeat advances normally: never flagged as hung.
        for _ in 0..3 {
            watchdog.tick();
            watchdog.evaluate(&mut last_seen, &mut last_change, threshold, false);
            assert!(!watchdog.is_hung());
            std::thread::sleep(Duration::from_millis(5));
        }

        // Now stop ticking (simulate the message loop getting stuck) and
        // let the short test threshold elapse.
        std::thread::sleep(threshold + Duration::from_millis(20));
        watchdog.evaluate(&mut last_seen, &mut last_change, threshold, false);
        assert!(
            watchdog.is_hung(),
            "watchdog should detect a heartbeat that hasn't advanced past the threshold"
        );

        // Resuming the heartbeat clears the hung flag again.
        watchdog.tick();
        watchdog.evaluate(&mut last_seen, &mut last_change, threshold, false);
        assert!(
            !watchdog.is_hung(),
            "watchdog should clear the hung flag once the heartbeat resumes"
        );
    }

    #[test]
    fn idle_wait_is_not_mistaken_for_a_hang() {
        let watchdog = TestWatchdog::new();
        let threshold = Duration::from_millis(50);
        let mut last_seen = watchdog.heartbeat.load(Ordering::Relaxed);
        let mut last_change = std::time::Instant::now();

        watchdog.tick();
        watchdog.evaluate(&mut last_seen, &mut last_change, threshold, false);
        assert!(!watchdog.is_hung());

        // Heartbeat stops advancing for well past the threshold, but the
        // loop is reported as idle (parked in `wait_message()`) throughout:
        // this must never be flagged as hung, no matter how long it lasts.
        for _ in 0..4 {
            std::thread::sleep(threshold);
            watchdog.evaluate(&mut last_seen, &mut last_change, threshold, true);
            assert!(
                !watchdog.is_hung(),
                "idle time must never be reported as a hang"
            );
        }

        // Coming out of idle with a genuinely stuck loop is still caught.
        std::thread::sleep(threshold + Duration::from_millis(20));
        watchdog.evaluate(&mut last_seen, &mut last_change, threshold, false);
        assert!(
            watchdog.is_hung(),
            "a real stall must still be detected once idle time is over"
        );
    }

    #[test]
    fn record_heartbeat_and_reader_are_visible_across_threads() {
        // Exercise the real process-global counter/API surface (not the
        // hang-detection timing, which is covered above) to make sure
        // `record_heartbeat` and `gui_thread_is_hung` compile and behave as
        // simple cross-thread visible primitives.
        let before = HEARTBEAT_TICKS.load(Ordering::Relaxed);
        record_heartbeat();
        let after = HEARTBEAT_TICKS.load(Ordering::Relaxed);
        assert_eq!(after, before + 1);

        // Just confirm the accessor doesn't panic; its value depends on
        // whatever real watchdog thread state exists in this test binary.
        let _ = gui_thread_is_hung();
    }
}
