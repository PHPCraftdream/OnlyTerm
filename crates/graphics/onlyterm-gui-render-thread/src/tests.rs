use crate::dispatch::dispatch_loop;
use crate::handle::{RenderMsg, RenderThreadHandle};
use crate::render::{surface_error_needs_renderer_rebuild, ClearSubmitStartedAtOnExit};
use onlyterm_gpu_render::backpressure::{
    finish_in_flight_frame, in_flight_is_set, is_hung_given, mark_repaint_pending,
};
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

/// Exercises the channel mechanics directly (no real OS thread, no
/// `GpuFrame`/GPU dependency): a `Shutdown` message stops
/// `dispatch_loop`, and a `Resize` message before it is observed (via
/// `on_resize` firing once, and the loop continuing past it to the
/// `Shutdown`) exactly as expected.
///
/// Note: because `dispatch_loop` coalesces a `Resize` with whatever
/// immediately follows it in the channel, the `Resize` sent here ends up
/// carrying the first `Shutdown` over as `carried_over` (since it's the
/// very next message already sitting in the channel) rather than the
/// `Resize`'s own recv triggering a separate loop iteration; either way
/// `on_resize` fires exactly once and the loop still stops at the first
/// `Shutdown`, which is what this test asserts.
#[test]
fn dispatch_loop_stops_on_shutdown() {
    let (tx, rx) = mpsc::channel();
    let frames_seen = AtomicUsize::new(0);
    let resizes_seen = AtomicUsize::new(0);

    tx.send(RenderMsg::Resize(::window::Dimensions {
        pixel_width: 100,
        pixel_height: 100,
        dpi: 96,
    }))
    .unwrap();
    tx.send(RenderMsg::Shutdown).unwrap();
    // Anything sent after Shutdown must never be observed, because the
    // loop breaks as soon as it processes the Shutdown message.
    tx.send(RenderMsg::Shutdown).unwrap();

    dispatch_loop(
        &rx,
        &mut |_frame| {
            frames_seen.fetch_add(1, Ordering::SeqCst);
        },
        &mut |_dims| {
            resizes_seen.fetch_add(1, Ordering::SeqCst);
        },
    );

    // No Frame messages were sent in this test, so the callback should
    // never have fired; the Resize/Shutdown handling is exercised by
    // the loop simply returning instead of hanging.
    assert_eq!(frames_seen.load(Ordering::SeqCst), 0);
    assert_eq!(resizes_seen.load(Ordering::SeqCst), 1);
}

/// Proves `dispatch_loop`'s `Resize` coalescing: three `Resize` messages
/// (A, B, C) followed by `Shutdown`, all sent before `dispatch_loop`
/// starts consuming so they're all sitting in the channel together when
/// the coalescing inner loop runs. Asserts `on_resize` is called exactly
/// once, with C (the latest), never with A or B.
///
/// This test intentionally uses only `Resize`/`Shutdown` messages, not a
/// `Frame`: `GpuFrame` holds real `wgpu::Buffer`/`wgpu::Texture` values
/// that need a live `wgpu::Device` to construct, which isn't available
/// in a unit test / CI (no GPU adapter) -- this is exactly why
/// `dispatch_loop`'s whole `on_frame`/`on_resize` callback design exists
/// in the first place (see 221.4/221.5), so tests never need to build
/// one. Generalizing `RenderMsg`/`dispatch_loop` further so tests could
/// use a placeholder payload type in a real `RenderMsg::Frame` slot would
/// be a bigger structural change than this task's scope.
///
/// Instead, the "a `Frame` sitting between Resizes is still delivered,
/// not silently dropped by coalescing" half of the property is argued
/// here rather than tested directly: `carried_over` is a plain
/// `Option<RenderMsg>`, generic over every `RenderMsg` variant, not just
/// `Resize`/`Shutdown`. When the inner `try_recv` loop (coalescing a run
/// of `Resize`s) encounters ANY non-`Resize` message -- `Frame` just as
/// much as `Shutdown` -- it stores that exact message in `carried_over`
/// and breaks immediately, without inspecting which variant it is. The
/// outer loop's next iteration then takes `carried_over` first (before
/// ever calling `rx.recv()` again) and dispatches it through the normal
/// `match msg { ... }`, which sends a `Frame` to `on_frame` exactly as it
/// would if it had been `rx.recv()`'d directly. So a `Frame` carried over
/// this way is handled on the very next loop iteration, never dropped --
/// the mechanism doesn't special-case which message it's carrying, as
/// this test's `Shutdown`-not-dropped assertion below directly
/// demonstrates for that variant.
#[test]
fn dispatch_loop_coalesces_resize_and_preserves_next_message() {
    let (tx, rx) = mpsc::channel();
    let resizes_seen: Vec<::window::Dimensions> = Vec::new();
    let resizes_seen = std::sync::Mutex::new(resizes_seen);

    let dims = |w: usize| ::window::Dimensions {
        pixel_width: w,
        pixel_height: w,
        dpi: 96,
    };

    tx.send(RenderMsg::Resize(dims(100))).unwrap(); // A
    tx.send(RenderMsg::Resize(dims(200))).unwrap(); // B
    tx.send(RenderMsg::Resize(dims(300))).unwrap(); // C
    tx.send(RenderMsg::Shutdown).unwrap();

    let mut frames_seen = 0usize;
    dispatch_loop(
        &rx,
        &mut |_frame| {
            frames_seen += 1;
        },
        &mut |d| {
            resizes_seen.lock().unwrap().push(d);
        },
    );

    let resizes_seen = resizes_seen.into_inner().unwrap();
    assert_eq!(
        resizes_seen.len(),
        1,
        "a run of back-to-back Resize messages must coalesce into a single on_resize call"
    );
    assert_eq!(
        resizes_seen[0],
        dims(300),
        "coalescing must keep the latest Resize (C), not an earlier one (A or B)"
    );
    assert_eq!(frames_seen, 0, "no Frame messages were sent in this test");
    // The loop must still have stopped at Shutdown (which was carried
    // over rather than dropped by the coalescing loop) -- if it hadn't,
    // dispatch_loop would still be blocked in rx.recv() and this test
    // would hang instead of reaching this point.
}

/// Confirms `dispatch_loop` also stops when the channel disconnects
/// (every `Sender` dropped) even if no explicit `Shutdown` was ever
/// sent - this is the fallback path relied on by
/// `TermWindow::drop`/`WindowEvent::Destroyed`, which drop the handle
/// without necessarily guaranteeing the explicit `Shutdown` message is
/// processed first.
#[test]
fn dispatch_loop_stops_on_disconnect() {
    let (tx, rx) = mpsc::channel();
    drop(tx);

    let mut frame_called = false;
    let mut resize_called = false;
    dispatch_loop(
        &rx,
        &mut |_frame| {
            frame_called = true;
        },
        &mut |_dims| {
            resize_called = true;
        },
    );

    assert!(!frame_called);
    assert!(!resize_called);
}

/// End-to-end thread-lifecycle test: spawns a *real* OS thread running
/// `dispatch_loop` (not the GPU-touching `render_thread_loop`, since
/// building a real `RenderThreadSeed` needs a `WebGpuState`, which
/// needs a GPU adapter that isn't available in CI/unit tests), sends it
/// a `Shutdown`, and confirms the thread actually exits within a short
/// timeout. This is the cross-platform-compiling, GPU-free stand-in for
/// testing `RenderThreadHandle::spawn`'s thread lifecycle; `spawn`
/// itself is `#[cfg(windows)]`-gated to actually spawn anything, but the
/// underlying loop/channel mechanics it relies on are exercised here on
/// every platform.
#[test]
fn spawned_thread_exits_after_shutdown() {
    let (tx, rx) = mpsc::channel();

    let join_handle = std::thread::Builder::new()
        .name("render-test".to_string())
        .spawn(move || dispatch_loop(&rx, &mut |_frame| {}, &mut |_dims| {}))
        .expect("spawn test render thread");

    tx.send(RenderMsg::Shutdown).unwrap();

    // Only test code joins here; production code (RenderThreadHandle)
    // never does, for the reasons documented on that struct. Bound the
    // wait so a regression (loop never exiting) fails the test instead
    // of hanging the suite forever.
    let start = std::time::Instant::now();
    loop {
        if join_handle.is_finished() {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "render thread did not exit within timeout after Shutdown"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    join_handle.join().expect("render thread panicked");
}

/// Builds a `RenderThreadHandle` around a caller-owned teardown sentinel,
/// with no GPU and no real thread. `spawn` cannot be used here (it needs
/// a `WebGpuState`, hence a GPU adapter), but every field the liveness
/// predicates read is plain shared state, so they can be exercised
/// directly. Returns the handle plus the sentinel's only strong
/// reference: dropping it is what "the render thread returned" looks
/// like to the handle.
fn handle_with_sentinel(window_destroyed: bool) -> (RenderThreadHandle, Arc<()>) {
    let strong = Arc::new(());
    let handle = RenderThreadHandle::for_test(window_destroyed, &strong);
    (handle, strong)
}

/// Running out of memory must not be answered by rebuilding the
/// renderer. The rebuild recreates the device and swapchain, and a crash
/// dump from this machine caught the GPU driver dereferencing NULL
/// inside `CreateSwapChainForHwnd` while the system was still short of
/// memory -- so the recovery destroyed the window that the dropped frame
/// would merely have flickered.
#[test]
fn out_of_memory_drops_a_frame_instead_of_rebuilding_the_renderer() {
    assert!(
        !surface_error_needs_renderer_rebuild(&wgpu::SurfaceError::OutOfMemory),
        "an out-of-memory frame must not trigger device/swapchain recreation"
    );
}

/// Everything that is not a shortage still describes a device in
/// trouble, and must keep triggering the rebuild it always did --
/// otherwise the narrowing above would quietly disable recovery
/// altogether.
#[test]
fn other_surface_errors_still_rebuild_the_renderer() {
    for err in [
        wgpu::SurfaceError::Timeout,
        wgpu::SurfaceError::Other,
        // Lost/Outdated are handled before this predicate is consulted,
        // but if they ever reach it they must not be treated as benign.
        wgpu::SurfaceError::Lost,
        wgpu::SurfaceError::Outdated,
    ] {
        assert!(
            surface_error_needs_renderer_rebuild(&err),
            "{:?} must still be recovered from by rebuilding",
            err
        );
    }
}

/// A render thread that unwound out of `render_thread_loop` -- the shape
/// a panic inside a wgpu call leaves behind -- must be reported as dead.
///
/// This is the gap that let a window freeze silently for as long as it
/// was left open: the thread is gone, so it is *not* hung (
/// `ClearSubmitStartedAtOnExit` clears `submit_started_at` during the
/// unwind on purpose), and before `render_thread_has_died` existed the
/// supervisor consulted nothing else. It saw "not hung", re-armed its
/// timer, and the window never painted again while still pumping its
/// message loop.
#[test]
fn a_render_thread_that_exited_is_reported_as_dead_not_as_hung() {
    let (handle, strong) = handle_with_sentinel(false);

    assert!(
        !handle.render_thread_has_died(),
        "a live render thread still holds the sentinel"
    );

    // The thread returns: its closure drops the sentinel's only strong
    // reference, strictly after `render_thread_loop` has returned.
    drop(strong);

    assert!(
        handle.render_thread_has_died(),
        "a render thread that has returned must be reported as dead"
    );
    assert!(
        !handle.render_thread_is_hung(),
        "test premise: a dead thread reads as not-hung, which is exactly \
             why the supervisor needs the death signal as well"
    );
}

/// The other way a render thread finishes is on request, and that one
/// must not be mistaken for a death -- otherwise closing a window would
/// have its supervisor try to rebuild the renderer on the way out.
/// `shutdown()` sets `window_destroyed` before sending `Shutdown`, which
/// is what makes the two distinguishable.
#[test]
fn an_orderly_shutdown_is_not_reported_as_a_death() {
    let (handle, strong) = handle_with_sentinel(true);
    drop(strong);

    assert!(
        !handle.render_thread_has_died(),
        "a thread that exited after shutdown() asked it to is not a death"
    );
}

/// Back-pressure bookkeeping test: a fake "in flight" frame slot backed
/// by plain `AtomicBool`s (no real `GpuFrame`/`WebGpuState` needed,
/// since `send_frame`'s swap-then-check-then-maybe-drop semantics don't
/// depend on what a "frame" actually is). Verifies that:
/// - the first send goes through and marks `in_flight`.
/// - a second send while still in flight is dropped and sets
///   `repaint_pending`, without ever reaching the "submit" step.
/// - finishing the in-flight frame clears `in_flight`, and observes
///   (and clears) `repaint_pending` so a caller can decide to
///   invalidate.
#[test]
fn back_pressure_drops_second_frame_while_in_flight() {
    let in_flight = AtomicBool::new(false);
    let repaint_pending = AtomicBool::new(false);
    let submitted = AtomicUsize::new(0);

    // Mirrors RenderThreadHandle::send_frame's swap-then-check logic,
    // using a unit "frame" (`()`) instead of a real `GpuFrame`.
    let send_frame = |_frame: ()| -> bool {
        if in_flight.swap(true, Ordering::AcqRel) {
            // The real `send_frame` drops the rejected `GpuFrame` here,
            // releasing its GPU resources. The stand-in frame is `()`,
            // which is `Copy`, so an explicit `drop` would be a no-op
            // that merely reads as though it did something.
            repaint_pending.store(true, Ordering::Release);
            return false;
        }
        submitted.fetch_add(1, Ordering::SeqCst);
        true
    };

    assert!(send_frame(()), "first send should go through");
    assert!(in_flight.load(Ordering::Acquire));
    assert_eq!(submitted.load(Ordering::SeqCst), 1);

    assert!(
        !send_frame(()),
        "second send while in flight should be dropped"
    );
    assert!(
        repaint_pending.load(Ordering::Acquire),
        "dropping a frame under back-pressure should request a repaint"
    );
    // Still only one frame ever reached "submit".
    assert_eq!(submitted.load(Ordering::SeqCst), 1);

    // Calls the real submit_one_frame tail helper (not a copy of its
    // ordering) so a regression there can't silently pass this test.
    let invalidated = finish_in_flight_frame(&in_flight, &repaint_pending);
    assert!(
        invalidated,
        "finishing the in-flight frame should observe repaint_pending and invalidate"
    );
    assert!(!in_flight.load(Ordering::Acquire));
    assert!(
        !repaint_pending.load(Ordering::Acquire),
        "repaint_pending should be cleared once observed"
    );

    // Now that in_flight is clear, a subsequent send goes through
    // again instead of being dropped.
    assert!(send_frame(()), "send after finishing should go through");
    assert_eq!(submitted.load(Ordering::SeqCst), 2);
}

/// Two-thread stress test for the in-flight/repaint-pending handshake.
/// Calls `in_flight_is_set`/`mark_repaint_pending`/
/// `finish_in_flight_frame` directly -- the exact functions
/// `RenderThreadHandle::is_in_flight`/`set_repaint_pending` and
/// `submit_one_frame`'s tail call in production -- from two real OS
/// threads racing against each other, instead of a single-threaded
/// stand-in that copies the algorithm and could silently drift out of
/// sync with a production ordering change (e.g. reverting SeqCst back
/// to Release/Acquire).
///
/// Models the actual race window in `call_draw_webgpu`: the GUI thread
/// stores to `repaint_pending` and then re-checks `in_flight` (a store
/// followed by a load of a *different* location -- the classic
/// StoreLoad pattern that Release/Acquire don't order between threads,
/// only SeqCst's total order does). The render thread's
/// `finish_in_flight_frame` is the other side of that same pair: it
/// clears `in_flight` then observes `repaint_pending`.
///
/// A "lost wakeup" is when neither side ends up thinking it should
/// invalidate: the GUI thread's re-check still reads `in_flight` as
/// `true` (so it defers to the render thread), and
/// `finish_in_flight_frame` observes `repaint_pending` as still `false`
/// (so the render thread sees nothing to do either) -- the frame that
/// was dropped under back-pressure never gets repainted. A `Barrier`
/// aligns each trial's start to maximize the chance of landing in that
/// interleaving window; run many trials and assert this never happens.
///
/// This is a best-effort stress test, not a formal proof: x86's strong
/// memory model makes the underlying anomaly rare to observe even under
/// Release/Acquire, so a regression isn't guaranteed to flip this test
/// red. It's still strictly better than the single-threaded version it
/// replaces, which could never have caught this class of bug at all.
#[test]
fn in_flight_repaint_handshake_never_loses_a_wakeup_under_contention() {
    use std::sync::Barrier;

    const TRIALS: usize = 100_000;
    let in_flight = AtomicBool::new(false);
    let repaint_pending = AtomicBool::new(false);
    let gui_would_invalidate = AtomicBool::new(false);
    let render_would_invalidate = AtomicBool::new(false);
    let lost_wakeups = AtomicUsize::new(0);
    let start = Barrier::new(2);
    let done = Barrier::new(2);

    std::thread::scope(|scope| {
        // GUI-thread side: mirrors call_draw_webgpu's guard -- ask for a
        // repaint, then re-check in_flight in case the render thread
        // already finished in the gap.
        scope.spawn(|| {
            for _ in 0..TRIALS {
                start.wait();
                mark_repaint_pending(&repaint_pending);
                let would_invalidate = !in_flight_is_set(&in_flight);
                gui_would_invalidate.store(would_invalidate, Ordering::SeqCst);
                done.wait();
            }
        });

        // Render-thread side: mirrors submit_one_frame's tail. Runs on
        // this (the test's own) thread.
        for _ in 0..TRIALS {
            // Reset both flags to the "frame in flight, no repaint
            // requested yet" state before this trial's racing segment
            // starts. This happens strictly before `start.wait()`, so
            // the Barrier's own synchronization (not the SeqCst under
            // test) is what makes it visible to the GUI thread --
            // exactly the setup/measurement split a litmus test needs.
            in_flight.store(true, Ordering::SeqCst);
            repaint_pending.store(false, Ordering::SeqCst);

            start.wait();
            let would_invalidate = finish_in_flight_frame(&in_flight, &repaint_pending);
            render_would_invalidate.store(would_invalidate, Ordering::SeqCst);
            done.wait();

            if !gui_would_invalidate.load(Ordering::SeqCst)
                && !render_would_invalidate.load(Ordering::SeqCst)
            {
                lost_wakeups.fetch_add(1, Ordering::SeqCst);
            }
        }
    });

    assert_eq!(
        lost_wakeups.load(Ordering::SeqCst),
        0,
        "at least one side must always observe the need to invalidate -- losing this race \
             means a dropped frame's content stays stuck on screen with nothing left to repaint it"
    );
}

/// Sending fails (the render thread is gone) *after* `in_flight` was
/// already swapped to `true` -- confirms `send_frame`'s failure path
/// resets `in_flight` back to `false` so back-pressure doesn't wedge
/// forever once the thread has exited.
#[test]
fn send_failure_resets_in_flight() {
    let (tx, rx) = mpsc::channel::<()>();
    drop(rx);

    let in_flight = AtomicBool::new(false);

    // Mirrors send_frame: swap in_flight, attempt the send, and on
    // failure reset in_flight.
    let was_in_flight = in_flight.swap(true, Ordering::AcqRel);
    assert!(!was_in_flight);
    if tx.send(()).is_err() {
        in_flight.store(false, Ordering::Release);
    }

    assert!(
        !in_flight.load(Ordering::Acquire),
        "in_flight must be reset when the send fails, or back-pressure wedges forever"
    );
}

/// Exercises `is_hung_given` (the config-free core of
/// `RenderThreadHandle::render_thread_is_hung`) directly against a
/// synthetic, short threshold and a real `Instant`/`sleep`, mirroring
/// `window::os::windows::watchdog`'s `TestWatchdog` style: fast, no fake
/// clock, no dependency on global `onlyterm_config::configuration()` state (which
/// `render_thread_is_hung` itself reads, but this lower-level helper
/// does not).
#[test]
fn is_hung_given_detects_a_long_running_call() {
    let submit_started_at: Mutex<Option<Instant>> = Mutex::new(None);
    let threshold = Duration::from_millis(50);

    // Nothing in flight: never hung.
    assert!(!is_hung_given(&submit_started_at, threshold));

    // Something starts running, but hasn't been running long: not hung
    // yet.
    *submit_started_at.lock() = Some(Instant::now());
    assert!(!is_hung_given(&submit_started_at, threshold));

    // Let the short threshold elapse for real.
    std::thread::sleep(threshold + Duration::from_millis(20));
    assert!(
        is_hung_given(&submit_started_at, threshold),
        "a call running longer than the threshold should be reported as hung"
    );

    // Clearing submit_started_at (as submit_one_frame does once the
    // call returns) goes back to not-hung, even though the elapsed time
    // since the (now-forgotten) start would still exceed the threshold.
    *submit_started_at.lock() = None;
    assert!(
        !is_hung_given(&submit_started_at, threshold),
        "clearing submit_started_at back to None should report not-hung again"
    );
}

/// Regression test for the bug fixed by `ClearSubmitStartedAtOnExit`:
/// before it existed, a panic inside the guarded call left
/// `submit_started_at` stuck at `Some(..)` forever, since the plain
/// `*submit_started_at.lock() = None;` written after the call (mirrored
/// here as `submit_frame` returning normally, in the first half of this
/// test) never runs when that call unwinds instead. This drives a
/// panicking closure through the exact guard/scope pattern
/// `submit_one_frame` uses and asserts `is_hung_given` reports not-hung
/// afterward either way -- proving the guard's `Drop` impl, not the
/// call's own return, is what clears the flag.
#[test]
fn clear_submit_started_at_on_exit_clears_on_panic() {
    let submit_started_at: Mutex<Option<Instant>> = Mutex::new(None);
    let threshold = Duration::from_millis(0);

    // Non-panicking call: still hung while the guard is alive, clear
    // once it drops at the end of the scope.
    *submit_started_at.lock() = Some(Instant::now());
    {
        let _clear_on_exit = ClearSubmitStartedAtOnExit(&submit_started_at);
        assert!(is_hung_given(&submit_started_at, threshold));
    }
    assert!(
        !is_hung_given(&submit_started_at, threshold),
        "the guard must clear submit_started_at on ordinary scope exit"
    );

    // Panicking call: the guard's Drop must still run during unwind.
    *submit_started_at.lock() = Some(Instant::now());
    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _clear_on_exit = ClearSubmitStartedAtOnExit(&submit_started_at);
        panic!("simulated submit_frame panic");
    }))
    .is_err();
    assert!(panicked, "the closure was supposed to panic");
    assert!(
        !is_hung_given(&submit_started_at, threshold),
        "the guard must clear submit_started_at even when the guarded call panics, \
             or the hang supervisor misreads a dead render thread as merely stuck"
    );
}
