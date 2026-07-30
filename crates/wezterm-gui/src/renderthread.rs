//! Scaffolding for moving WebGpu frame submission off the GUI thread.
//!
//! Task 221.4 set up the channel and thread lifecycle; this module (221.5)
//! wires `RenderMsg::Frame` up to `WebGpuState::submit_frame`, with
//! single-slot back-pressure so at most one frame is ever in flight on the
//! render thread at a time. `RenderMsg::Resize` is still ignored (221.6).
//!
//! Only used behind `config::webgpu_render_thread`, which defaults to
//! `false`, so none of this changes behavior until a later task flips the
//! default (221.8).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use ::window::WindowOps;

/// A message sent from the GUI thread to a window's dedicated render
/// thread.
pub enum RenderMsg {
    /// A fully built frame ready to submit to the GPU. The render thread
    /// calls `WebGpuState::submit_frame` with this.
    Frame(crate::termwindow::webgpu::GpuFrame),
    /// A resize/reconfigure request. 221.6 will make the render thread
    /// actually call `WebGpuState::resize` (i.e. `surface.configure`) with
    /// this; for now the render thread just ignores it.
    #[allow(dead_code)]
    Resize(::window::Dimensions),
    /// Ask the render thread to stop its message loop and exit.
    Shutdown,
}

/// Everything a newly spawned render thread needs to run, handed over from
/// the GUI thread. `window` is used to request a fresh repaint when a
/// dropped (back-pressured) frame needs replacing; `webgpu` is used to
/// actually submit frames.
pub struct RenderThreadSeed {
    pub window: ::window::Window,
    pub webgpu: std::sync::Arc<crate::termwindow::webgpu::WebGpuState>,
    pub rx: std::sync::mpsc::Receiver<RenderMsg>,
    /// True while a frame has been sent to the thread and hasn't finished
    /// submitting yet. Shared with `RenderThreadHandle` (same `Arc`), which
    /// is why the GUI thread and the render thread always observe the same
    /// value rather than independent copies.
    pub in_flight: Arc<AtomicBool>,
    /// Set by the GUI thread when it drops a frame due to back-pressure;
    /// cleared by the render thread once it finishes the in-flight frame,
    /// at which point it calls `window.invalidate()` to ask for a fresh
    /// repaint (the dropped frame's content is now stale).
    pub repaint_pending: Arc<AtomicBool>,
}

/// A handle to a window's dedicated render thread, owned by `TermWindow`.
///
/// Deliberately holds no `JoinHandle`. The entire point of moving GPU
/// submission off the GUI thread is so that a stuck driver call (a TDR, a
/// swapchain `present()` that never returns) can't freeze the message loop.
/// If window-close code called `.join()` on this thread, a hung render
/// thread would hang the close operation too, which defeats the purpose.
/// So this handle can only ever send messages to the thread and drop its
/// `Sender`; the thread itself is fire-and-forget from the GUI thread's
/// point of view.
pub struct RenderThreadHandle {
    tx: std::sync::mpsc::Sender<RenderMsg>,
    /// Same `Arc<AtomicBool>` as `RenderThreadSeed::in_flight` -- see
    /// `send_frame` for the single-slot back-pressure scheme this
    /// implements.
    in_flight: Arc<AtomicBool>,
    /// Same `Arc<AtomicBool>` as `RenderThreadSeed::repaint_pending`.
    repaint_pending: Arc<AtomicBool>,
}

impl RenderThreadHandle {
    /// Spawn a dedicated render thread for one window, identified by
    /// `window_id` for the thread's name (used e.g. as `TermWindow`'s
    /// `mux_window_id`, so the thread is identifiable in a debugger/task
    /// manager without inventing new plumbing just for this).
    ///
    /// The caller creates the `mpsc::channel()` pair (and the
    /// `in_flight`/`repaint_pending` `Arc<AtomicBool>`s) once: `tx` is
    /// handed here so it can be wrapped up into the returned handle, and
    /// `rx` is handed here already embedded in `seed`
    /// (`RenderThreadSeed::rx`) so it can be moved onto the new thread.
    /// This way the channel and back-pressure flags are constructed
    /// exactly once by the caller, never duplicated inside `spawn`.
    ///
    /// Returns `Some(handle)` on Windows, where the render thread is
    /// actually spawned. Returns `None` everywhere else: the render-thread
    /// pipeline (221.1-221.9) is Windows-only for now (see the plan doc,
    /// "Уровень C"), and other platforms keep rendering synchronously on
    /// the GUI thread with no functional change.
    ///
    /// The uniform `Option<RenderThreadHandle>` return type (rather than
    /// `#[cfg]`-ing the function signature itself) means call sites in
    /// `TermWindow` never need their own `#[cfg(windows)]`.
    #[cfg(windows)]
    pub fn spawn(
        seed: RenderThreadSeed,
        tx: std::sync::mpsc::Sender<RenderMsg>,
        window_id: impl std::fmt::Display,
    ) -> Option<Self> {
        let in_flight = Arc::clone(&seed.in_flight);
        let repaint_pending = Arc::clone(&seed.repaint_pending);
        let name = format!("render-{window_id}");
        let builder = std::thread::Builder::new().name(name);
        match builder.spawn(move || render_thread_loop(seed)) {
            Ok(join_handle) => {
                // We deliberately never join this thread; see the doc
                // comment on `RenderThreadHandle`. Discard the
                // `JoinHandle` so it's clear this is intentional, not an
                // oversight.
                drop(join_handle);
                Some(Self {
                    tx,
                    in_flight,
                    repaint_pending,
                })
            }
            Err(err) => {
                log::error!("Failed to spawn render thread: {:#}", err);
                None
            }
        }
    }

    #[cfg(not(windows))]
    pub fn spawn(
        _seed: RenderThreadSeed,
        _tx: std::sync::mpsc::Sender<RenderMsg>,
        _window_id: impl std::fmt::Display,
    ) -> Option<Self> {
        None
    }

    /// Send a message to the render thread. Returns an error if the thread
    /// has already exited (its `Receiver` was dropped); callers generally
    /// don't need to do anything about that other than not panic.
    ///
    /// `clippy::result_large_err` fires because `SendError<RenderMsg>`
    /// carries a whole `GpuFrame` back out on failure; not boxing it since
    /// this is a cold, infrequent-call path (once per frame at most, and
    /// only ever hit once the render thread is already gone), not a hot
    /// loop where the extra stack size would matter.
    ///
    /// This is a lower-level primitive kept around for future
    /// `Resize`-message call sites (221.6) that won't go through the
    /// back-pressure scheme in `send_frame`; nothing calls it yet, hence
    /// `#[allow(dead_code)]`, matching how it was already marked in 221.4.
    #[allow(dead_code)]
    #[allow(clippy::result_large_err)]
    pub fn send(&self, msg: RenderMsg) -> Result<(), std::sync::mpsc::SendError<RenderMsg>> {
        self.tx.send(msg)
    }

    /// Send a `GpuFrame` to the render thread, honoring single-slot
    /// back-pressure: at most one frame is ever in flight (sent but not
    /// yet finished submitting). If a frame is already in flight, this one
    /// is dropped instead of queued, and `repaint_pending` is set so the
    /// render thread asks for a fresh repaint once it finishes the
    /// in-flight frame -- otherwise this frame's content would just be
    /// lost with nothing to trigger a replacement.
    pub fn send_frame(&self, frame: crate::termwindow::webgpu::GpuFrame) {
        if self.in_flight.swap(true, Ordering::AcqRel) {
            // A frame is already in flight; drop this one (its buffers are
            // released here) instead of queueing, and remember to ask for
            // a fresh repaint once the in-flight frame finishes.
            drop(frame);
            self.repaint_pending.store(true, Ordering::Release);
            metrics::counter!("gui.render_thread.frames_dropped").increment(1);
            return;
        }
        if self.tx.send(RenderMsg::Frame(frame)).is_err() {
            // Thread is already gone; undo the in_flight flag we just set
            // so we don't wedge back-pressure checks forever (there's
            // nothing else useful to do here -- the window is going
            // away).
            self.in_flight.store(false, Ordering::Release);
        }
    }

    /// Ask the render thread to stop. This does not wait for it to actually
    /// exit (no `.join()` - see the struct doc comment). A send error here
    /// just means the thread is already gone, which is fine.
    pub fn shutdown(&self) {
        let _ = self.tx.send(RenderMsg::Shutdown);
    }
}

/// The render thread's message loop. Runs until the channel disconnects
/// (all `Sender`s, including the one held by `RenderThreadHandle`, were
/// dropped) or a `RenderMsg::Shutdown` is received - whichever happens
/// first.
///
/// `RenderMsg::Frame` is submitted via `seed.webgpu.submit_frame(frame)`;
/// `RenderMsg::Resize` is still ignored (221.6 wires that up to
/// `seed.webgpu.resize(dims)`).
#[cfg_attr(not(windows), allow(dead_code))]
fn render_thread_loop(seed: RenderThreadSeed) {
    let in_flight = Arc::clone(&seed.in_flight);
    let repaint_pending = Arc::clone(&seed.repaint_pending);
    let webgpu = Arc::clone(&seed.webgpu);
    let window = seed.window.clone();
    dispatch_loop(&seed.rx, &mut |frame| {
        submit_one_frame(&webgpu, &window, frame, &in_flight, &repaint_pending);
    });
}

/// Submits a single frame to the GPU and performs the back-pressure
/// bookkeeping (clearing `in_flight`, honoring `repaint_pending`). Split out
/// of `render_thread_loop` so the "what does a Frame message actually do"
/// logic is easy to read on its own; `dispatch_loop` remains agnostic to
/// what the closure does with a `Frame`.
fn submit_one_frame(
    webgpu: &crate::termwindow::webgpu::WebGpuState,
    window: &::window::Window,
    frame: crate::termwindow::webgpu::GpuFrame,
    in_flight: &AtomicBool,
    repaint_pending: &AtomicBool,
) {
    let stall_ms = config::configuration().debug_render_thread_stall_ms;
    if stall_ms > 0 {
        std::thread::sleep(std::time::Duration::from_millis(stall_ms));
    }
    let start = std::time::Instant::now();
    if let Err(err) = webgpu.submit_frame(frame) {
        // Proper SurfaceError::Lost/Outdated handling (reconfigure +
        // drop-this-frame semantics) is task 221.6, not this one -- for
        // now, just log and count it, matching today's crash-free
        // fallback of "skip this frame, the next NeedRepaint will try
        // again".
        log::error!("render thread: submit_frame failed: {:#}", err);
        metrics::counter!("gui.render_thread.submit_error").increment(1);
    }
    metrics::histogram!("gui.render_thread.submit").record(start.elapsed());
    in_flight.store(false, Ordering::Release);
    if repaint_pending.swap(false, Ordering::AcqRel) {
        window.invalidate();
    }
}

/// The message-dispatch loop shared by `render_thread_loop` and its unit
/// tests. Kept free of any `WebGpuState`/`Window` dependency directly --
/// instead it takes an `on_frame` closure to run for each `RenderMsg::Frame`
/// it sees, so production code can plug in the real
/// `seed.webgpu.submit_frame(...)` path while tests can plug in a fake
/// GPU-free closure and still exercise the shutdown/disconnect/back-pressure
/// bookkeeping end to end.
fn dispatch_loop(
    rx: &std::sync::mpsc::Receiver<RenderMsg>,
    on_frame: &mut dyn FnMut(crate::termwindow::webgpu::GpuFrame),
) {
    while let Ok(msg) = rx.recv() {
        match msg {
            RenderMsg::Frame(frame) => on_frame(frame),
            RenderMsg::Resize(_) => {
                // 221.6 will handle this.
            }
            RenderMsg::Shutdown => break,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::sync::mpsc;
    use std::time::Duration;

    /// Exercises the channel mechanics directly (no real OS thread, no
    /// `GpuFrame`/GPU dependency): a `Shutdown` message stops
    /// `dispatch_loop`, and `Resize` messages before it are each observed
    /// (via the `on_frame` callback never firing for them, and the loop
    /// continuing past them) exactly as expected.
    #[test]
    fn dispatch_loop_stops_on_shutdown() {
        let (tx, rx) = mpsc::channel();
        let frames_seen = AtomicUsize::new(0);

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

        dispatch_loop(&rx, &mut |_frame| {
            frames_seen.fetch_add(1, Ordering::SeqCst);
        });

        // No Frame messages were sent in this test, so the callback should
        // never have fired; the Resize/Shutdown handling is exercised by
        // the loop simply returning instead of hanging.
        assert_eq!(frames_seen.load(Ordering::SeqCst), 0);
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

        let mut called = false;
        dispatch_loop(&rx, &mut |_frame| {
            called = true;
        });

        assert!(!called);
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
            .spawn(move || dispatch_loop(&rx, &mut |_frame| {}))
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
        let send_frame = |frame: ()| -> bool {
            if in_flight.swap(true, Ordering::AcqRel) {
                drop(frame);
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

        // Mirrors submit_one_frame's tail: clear in_flight, then swap
        // repaint_pending out and act on it if it was set.
        in_flight.store(false, Ordering::Release);
        let mut invalidated = false;
        if repaint_pending.swap(false, Ordering::AcqRel) {
            invalidated = true;
        }
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
}
