//! The in-flight/repaint-pending frame-delivery handshake and hang-detection
//! predicate shared by both `RenderBackend` implementations:
//! `onlyterm_gui_render_thread::RenderThreadHandle` (in-process render
//! thread) and `onlyterm_gpu_render::HostProcessBackend` (per-window
//! `--gpu-tab-host` child process).
//!
//! These used to live as private copies in the render-thread crate, and
//! `HostProcessBackend` re-implemented the handshake inline with
//! `Release`/`Acquire` orderings -- silently reintroducing the lost-wakeup
//! race that commit aaf1f8f58 had already fixed for the in-process path
//! (SeqCst): `Release`/`Acquire` does not order a store to one atomic
//! against a later load of a *different* atomic on the same thread (no
//! StoreLoad ordering), so `call_draw_webgpu`'s check / set_repaint_pending
//! / re-check sequence could observe stale values on both sides and lose
//! the wakeup, leaving a freshly built frame dropped and never repainted.
//! Sharing these exact functions is what keeps that guarantee in one place.

use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// One side of the handshake: is a frame still being processed by the
/// render backend (render thread or child process)?
///
/// SeqCst (not Acquire): paired with [`mark_repaint_pending`] and
/// [`finish_in_flight_frame`]'s SeqCst ops, this closes a store-buffer race
/// in `call_draw_webgpu`'s lost-wakeup check (which stores to
/// `repaint_pending` and then re-checks `in_flight`, a store followed by a
/// load of a *different* location). Release/Acquire alone doesn't order a
/// store against a later load of another location on the same thread (only
/// SeqCst's total order does), which could let both this thread and the
/// render side simultaneously observe stale values and each assume the
/// other will request the repaint.
pub fn in_flight_is_set(in_flight: &AtomicBool) -> bool {
    in_flight.load(Ordering::SeqCst)
}

/// The other GUI-side op of the handshake: record that a fresh repaint is
/// needed once the currently in-flight frame finishes. See
/// [`in_flight_is_set`] for why SeqCst.
pub fn mark_repaint_pending(repaint_pending: &AtomicBool) {
    repaint_pending.store(true, Ordering::SeqCst)
}

/// The render-side op of the handshake, run once a frame finishes
/// submitting: clears `in_flight`, then observes-and-clears
/// `repaint_pending`. Returns `true` if the caller should invalidate the
/// window (a repaint was requested while this frame was in flight). See
/// [`in_flight_is_set`] for why SeqCst.
pub fn finish_in_flight_frame(in_flight: &AtomicBool, repaint_pending: &AtomicBool) -> bool {
    in_flight.store(false, Ordering::SeqCst);
    repaint_pending.swap(false, Ordering::SeqCst)
}

/// The "is a call that's been running since `submit_started_at` older than
/// `threshold`" predicate, split out from the callers'
/// `render_thread_is_hung` implementations so it can be unit tested with a
/// synthetic threshold instead of depending on live global config state.
pub fn is_hung_given(submit_started_at: &Mutex<Option<Instant>>, threshold: Duration) -> bool {
    match *submit_started_at.lock() {
        Some(start) => start.elapsed() >= threshold,
        None => false,
    }
}

/// A minimal best-effort log throttle for diagnostics that can fire on
/// every colliding paint under sustained terminal output: their job is to
/// leave a per-incident trace that can be correlated against other
/// timestamped log lines after the fact, not to record every occurrence.
///
/// Instances are typically `static`s (`Mutex::new` is const, so
/// `static LOG: LogRateLimiter = LogRateLimiter::new();` works), and are
/// safe to share across threads.
pub struct LogRateLimiter {
    last: Mutex<Option<Instant>>,
}

impl LogRateLimiter {
    pub const fn new() -> Self {
        Self {
            last: Mutex::new(None),
        }
    }

    /// Returns true at most once per `min_interval` (first call always
    /// returns true); the caller is expected to log only when it does.
    pub fn should_log(&self, min_interval: Duration) -> bool {
        let mut last = self.last.lock();
        if let Some(t) = *last {
            if t.elapsed() < min_interval {
                return false;
            }
        }
        *last = Some(Instant::now());
        true
    }
}

impl Default for LogRateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// At most one admission per interval, with the first call always
    /// admitted.
    #[test]
    fn should_log_admits_at_most_one_per_interval() {
        let limiter = LogRateLimiter::new();
        let interval = Duration::from_millis(50);
        assert!(limiter.should_log(interval), "first call must be admitted");
        assert!(
            !limiter.should_log(interval),
            "an immediate second call must be throttled"
        );
        std::thread::sleep(interval + Duration::from_millis(20));
        assert!(
            limiter.should_log(interval),
            "after the interval elapses the next call must be admitted again"
        );
    }
}
