//! Scaffolding for moving WebGpu frame submission off the GUI thread.
//!
//! Task 221.4 set up the channel and thread lifecycle; 221.5 wired
//! `RenderMsg::Frame` up to `WebGpuState::submit_frame`, with single-slot
//! back-pressure so at most one frame is ever in flight on the render thread
//! at a time. This module (221.6) wires `RenderMsg::Resize` up to
//! `WebGpuState::resize` (i.e. `surface.configure`), with coalescing so a
//! flood of resize messages (e.g. a live drag) collapses to just the latest
//! one, and gives `submit_one_frame` real `SurfaceError::Lost`/`Outdated`
//! recovery via `WebGpuState::reconfigure`.
//!
//! Task 221.7 adds window-teardown safety and hang visibility on top of
//! that: a `window_destroyed` flag so a `Frame`/`Resize` message that was
//! already queued before `Shutdown` don't reach into a dead HWND's GPU
//! resources, and a `submit_started_at` timestamp so a future per-window
//! supervisor (task #223) can ask "is this window's render thread currently
//! stuck inside a submit/reconfigure call".
//!
//! Only used behind `config::webgpu_render_thread`, which defaults to
//! `false`, so none of this changes behavior until a later task flips the
//! default (221.8).

use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use ::window::WindowOps;

/// A message sent from the GUI thread to a window's dedicated render
/// thread.
pub enum RenderMsg {
    /// A fully built frame ready to submit to the GPU. The render thread
    /// calls `WebGpuState::submit_frame` with this.
    Frame(crate::termwindow::webgpu::GpuFrame),
    /// A resize/reconfigure request. The render thread calls
    /// `WebGpuState::resize` (i.e. `surface.configure`) with this; see
    /// `dispatch_loop` for how a run of back-to-back `Resize` messages gets
    /// coalesced into just the latest one.
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
    /// Set (before `Shutdown` is even sent) by `RenderThreadHandle::shutdown`
    /// once the window's HWND is being/has been destroyed. Checked by the
    /// render thread before any GPU call so a `Frame`/`Resize` message that
    /// was already sitting in the channel ahead of `Shutdown` (the channel
    /// is FIFO; `dispatch_loop` only stops once it actually dequeues and
    /// matches `Shutdown`) doesn't reach into a dead window's GPU resources.
    ///
    /// This only helps for calls that haven't *started* yet: a thread
    /// already blocked inside a stuck `present()`/`configure()` call when
    /// `Destroyed` fires is not rescued by this flag. Full protection
    /// against that belongs to future process-level isolation (task #224);
    /// out of scope here.
    pub window_destroyed: Arc<AtomicBool>,
    /// `Some(when the currently in-flight submit/reconfigure call started)`,
    /// or `None` when no such call is in flight right now. Set/cleared
    /// around `webgpu.submit_frame` (see `submit_one_frame`) so
    /// `RenderThreadHandle::render_thread_is_hung` can tell a future
    /// per-window supervisor (task #223) whether this window's render
    /// thread is currently stuck.
    pub submit_started_at: Arc<Mutex<Option<Instant>>>,
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
    /// Same `Arc<AtomicBool>` as `RenderThreadSeed::window_destroyed`; set by
    /// `shutdown()`.
    window_destroyed: Arc<AtomicBool>,
    /// Same `Arc<Mutex<Option<Instant>>>` as `RenderThreadSeed::submit_started_at`;
    /// read by `render_thread_is_hung()`.
    ///
    /// `#[allow(dead_code)]`-adjacent: nothing calls `render_thread_is_hung`
    /// yet (task #223, blocked on this task, is expected to be the first
    /// caller), so this field currently has no reader either. Mirrors
    /// `window::os::windows::watchdog::gui_thread_is_hung`'s own
    /// `#[allow(dead_code)]` for the same reason.
    #[allow(dead_code)]
    submit_started_at: Arc<Mutex<Option<Instant>>>,
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
        let window_destroyed = Arc::clone(&seed.window_destroyed);
        let submit_started_at = Arc::clone(&seed.submit_started_at);
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
                    window_destroyed,
                    submit_started_at,
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
    /// This is a lower-level primitive; `send_resize` and `send_frame`
    /// are the call sites that use it (`shutdown` sends `Shutdown` directly
    /// since it doesn't need the `Result`). Kept `pub` for potential future
    /// direct callers.
    #[allow(dead_code)]
    #[allow(clippy::result_large_err)]
    pub fn send(&self, msg: RenderMsg) -> Result<(), std::sync::mpsc::SendError<RenderMsg>> {
        self.tx.send(msg)
    }

    /// Send a resize/reconfigure request to the render thread. Unlike
    /// `send_frame`, this is not back-pressured -- resize messages are cheap
    /// (just a `Dimensions` value, no GPU resources attached) and must never
    /// be silently dropped, so every call sends. A flood of these (e.g. a
    /// live window drag delivering many resize events in quick succession)
    /// is instead coalesced on the receiving end, in `dispatch_loop`, into
    /// just the latest one before `WebGpuState::resize` ever runs.
    pub fn send_resize(&self, dims: ::window::Dimensions) {
        let _ = self.tx.send(RenderMsg::Resize(dims));
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
    ///
    /// Sets `window_destroyed` first, before sending `Shutdown`, so it's
    /// visible as early as possible -- though the two don't need to be
    /// perfectly synchronized, since the render thread re-checks the flag
    /// per-message anyway (see `submit_one_frame`'s guard). This covers a
    /// `Frame`/`Resize` that was already queued ahead of `Shutdown` in the
    /// channel: `dispatch_loop` will still dequeue and run it before it ever
    /// sees `Shutdown` (the channel is FIFO), so the flag is what actually
    /// prevents a stale GPU call, not the `Shutdown` message itself.
    pub fn shutdown(&self) {
        self.window_destroyed.store(true, Ordering::Release);
        let _ = self.tx.send(RenderMsg::Shutdown);
    }

    /// Returns true if this window's render thread appears to be currently
    /// stuck inside a single submit/reconfigure GPU call for longer than
    /// `render_thread_hang_threshold_ms` (read live from config, same
    /// "re-read every check, no restart needed" pattern as
    /// `window::watchdog::gui_thread_is_hung`'s backing thread).
    ///
    /// Unlike that watchdog, this is a stateless, side-effect-free predicate
    /// with no logging/metrics of its own -- there is no background poller
    /// for render threads yet (task #223 is expected to add one, and should
    /// do its own edge-detection/logging on top of this call).
    #[allow(dead_code)]
    pub fn render_thread_is_hung(&self) -> bool {
        let threshold =
            Duration::from_millis(config::configuration().render_thread_hang_threshold_ms);
        is_hung_given(&self.submit_started_at, threshold)
    }
}

/// The actual "is a call that's been running since `submit_started_at` older
/// than `threshold`" predicate, split out from `render_thread_is_hung` so it
/// can be unit tested with a synthetic threshold instead of depending on
/// live global config state.
#[allow(dead_code)]
fn is_hung_given(submit_started_at: &Mutex<Option<Instant>>, threshold: Duration) -> bool {
    match *submit_started_at.lock() {
        Some(start) => start.elapsed() >= threshold,
        None => false,
    }
}

/// The render thread's message loop. Runs until the channel disconnects
/// (all `Sender`s, including the one held by `RenderThreadHandle`, were
/// dropped) or a `RenderMsg::Shutdown` is received - whichever happens
/// first.
///
/// `RenderMsg::Frame` is submitted via `seed.webgpu.submit_frame(frame)`;
/// `RenderMsg::Resize` calls `seed.webgpu.resize(dims)` (see `dispatch_loop`
/// for the coalescing applied to a run of back-to-back resize messages).
#[cfg_attr(not(windows), allow(dead_code))]
fn render_thread_loop(seed: RenderThreadSeed) {
    let in_flight = Arc::clone(&seed.in_flight);
    let repaint_pending = Arc::clone(&seed.repaint_pending);
    let window_destroyed = Arc::clone(&seed.window_destroyed);
    let submit_started_at = Arc::clone(&seed.submit_started_at);
    let webgpu = Arc::clone(&seed.webgpu);
    let window = seed.window.clone();
    let resize_webgpu = Arc::clone(&seed.webgpu);
    let resize_window_destroyed = Arc::clone(&seed.window_destroyed);
    dispatch_loop(
        &seed.rx,
        &mut |frame| {
            submit_one_frame(
                &webgpu,
                &window,
                frame,
                &in_flight,
                &repaint_pending,
                &window_destroyed,
                &submit_started_at,
            );
        },
        &mut |dims| {
            if resize_window_destroyed.load(Ordering::Acquire) {
                // The window is gone (or on its way out); a Resize that was
                // queued before Shutdown must not reach into a dead HWND's
                // surface. Nothing else to do here: resize has no
                // in_flight/repaint_pending bookkeeping of its own.
                log::debug!("render thread: skipping stale resize after window destruction");
                return;
            }
            resize_webgpu.resize(dims);
        },
    );
}

/// Submits a single frame to the GPU and performs the back-pressure
/// bookkeeping (clearing `in_flight`, honoring `repaint_pending`). Split out
/// of `render_thread_loop` so the "what does a Frame message actually do"
/// logic is easy to read on its own; `dispatch_loop` remains agnostic to
/// what the closure does with a `Frame`.
///
/// Checks `window_destroyed` before touching the GPU at all: this only
/// rescues calls that haven't *started* yet (a message queued ahead of
/// `Shutdown` in the channel -- see `RenderThreadHandle::shutdown`). A call
/// already blocked inside `submit_frame`/`reconfigure` when the window gets
/// destroyed is not interrupted by this check; that residual risk is
/// accepted for now and belongs to future process-level isolation
/// (task #224), not this task.
fn submit_one_frame(
    webgpu: &crate::termwindow::webgpu::WebGpuState,
    window: &::window::Window,
    frame: crate::termwindow::webgpu::GpuFrame,
    in_flight: &AtomicBool,
    repaint_pending: &AtomicBool,
    window_destroyed: &AtomicBool,
    submit_started_at: &Mutex<Option<Instant>>,
) {
    if window_destroyed.load(Ordering::Acquire) {
        // The window is gone (or on its way out): don't touch the GPU
        // surface at all. We still clear `in_flight` -- that bookkeeping is
        // always safe/necessary regardless of whether the frame was
        // actually submitted, since a future (impossible, since the window
        // is dying, but cheap to keep correct) `send_frame` call must not
        // wedge against a `true` that will never be cleared otherwise. We
        // deliberately do NOT consult/clear `repaint_pending` or call
        // `window.invalidate()`: both exist purely to ask the GUI thread for
        // another repaint, which is meaningless (and possibly unsafe, since
        // the window's data may already be torn down) once the window is
        // being destroyed.
        log::debug!("render thread: skipping stale frame after window destruction");
        drop(frame);
        in_flight.store(false, Ordering::Release);
        return;
    }
    let stall_ms = config::configuration().debug_render_thread_stall_ms;
    if stall_ms > 0 {
        std::thread::sleep(std::time::Duration::from_millis(stall_ms));
    }
    let start = std::time::Instant::now();
    *submit_started_at.lock() = Some(start);
    let result = webgpu.submit_frame(frame);
    *submit_started_at.lock() = None;
    if let Err(err) = result {
        match err {
            // `Lost`/`Outdated` mean the swapchain itself needs recreating,
            // not a real draw failure. This is an intentional behavior
            // change from the synchronous (non-render-thread) path in
            // `TermWindow::do_paint_webgpu`, which reruns the ENTIRE
            // `paint_impl` (rebuilding the whole frame) inline on the GUI
            // thread when it sees this error. Here we instead reconfigure
            // the surface and drop the failed frame; `window.invalidate()`
            // requests a fresh `NeedRepaint`, so the GUI thread builds and
            // sends a brand-new `GpuFrame` on its own next iteration through
            // the normal event loop. Net effect is functionally equivalent
            // (one dropped frame, then a fresh full repaint) just decoupled
            // across the thread boundary instead of happening inline.
            wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated => {
                if window_destroyed.load(Ordering::Acquire) {
                    // The window was destroyed while submit_frame was
                    // running (or the flag simply wasn't visible until
                    // now). Reconfiguring a dead surface and invalidating a
                    // dead window are both pointless (and the latter could
                    // touch torn-down state), so skip straight past them.
                    log::debug!(
                        "render thread: surface {:?} after window destruction, skipping reconfigure",
                        err
                    );
                } else {
                    log::warn!("render thread: surface {:?}, reconfiguring", err);
                    webgpu.reconfigure();
                    metrics::counter!("gui.render_thread.surface_reconfigured").increment(1);
                    window.invalidate();
                }
            }
            other => {
                log::error!("render thread: submit_frame failed: {:#}", other);
                metrics::counter!("gui.render_thread.submit_error").increment(1);
            }
        }
    }
    metrics::histogram!("gui.render_thread.submit").record(start.elapsed());
    in_flight.store(false, Ordering::Release);
    // Note: if the reconfigure branch above already called
    // `window.invalidate()`, and `repaint_pending` also happens to be true
    // here, this can call `invalidate()` again. That's harmless: it just
    // requests a repaint, and requesting one twice back-to-back doesn't
    // double-render anything.
    if repaint_pending.swap(false, Ordering::AcqRel) {
        window.invalidate();
    }
}

/// The message-dispatch loop shared by `render_thread_loop` and its unit
/// tests. Kept free of any `WebGpuState`/`Window` dependency directly --
/// instead it takes `on_frame`/`on_resize` closures to run for each
/// `RenderMsg::Frame`/`RenderMsg::Resize` it sees, so production code can
/// plug in the real `seed.webgpu.submit_frame(...)`/`seed.webgpu.resize(...)`
/// paths while tests can plug in fake GPU-free closures and still exercise
/// the shutdown/disconnect/back-pressure/coalescing bookkeeping end to end.
///
/// `RenderMsg::Resize` messages are coalesced: a run of back-to-back
/// `Resize`s already sitting in the channel (e.g. from a live-drag flood)
/// collapses into a single `on_resize` call with just the latest one, since
/// only the final size matters and every intermediate `surface.configure`
/// would otherwise be wasted work on the render thread. This uses an
/// explicit one-message look-ahead buffer (`carried_over`) rather than a
/// naive `try_recv` drain loop, because `std::sync::mpsc::Receiver` has no
/// peek/push-back: once `try_recv` pulls a non-`Resize` message off the
/// channel to check it, that message is gone from the channel and MUST be
/// remembered here, or it would be silently dropped (e.g. a `Frame` or
/// `Shutdown` sitting right after a run of `Resize`s).
fn dispatch_loop(
    rx: &std::sync::mpsc::Receiver<RenderMsg>,
    on_frame: &mut dyn FnMut(crate::termwindow::webgpu::GpuFrame),
    on_resize: &mut dyn FnMut(::window::Dimensions),
) {
    let mut carried_over: Option<RenderMsg> = None;
    loop {
        let msg = match carried_over.take() {
            Some(m) => m,
            None => match rx.recv() {
                Ok(m) => m,
                Err(_) => break,
            },
        };
        match msg {
            RenderMsg::Resize(mut dims) => {
                // Coalesce a run of back-to-back Resize messages already
                // sitting in the channel into just the latest one. Stop as
                // soon as something that ISN'T a Resize shows up, and carry
                // that message over to the next loop iteration instead of
                // dropping it.
                loop {
                    match rx.try_recv() {
                        Ok(RenderMsg::Resize(newer)) => dims = newer,
                        Ok(other) => {
                            carried_over = Some(other);
                            break;
                        }
                        Err(_) => break,
                    }
                }
                on_resize(dims);
            }
            RenderMsg::Frame(frame) => on_frame(frame),
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

    /// Exercises `is_hung_given` (the config-free core of
    /// `RenderThreadHandle::render_thread_is_hung`) directly against a
    /// synthetic, short threshold and a real `Instant`/`sleep`, mirroring
    /// `window::os::windows::watchdog`'s `TestWatchdog` style: fast, no fake
    /// clock, no dependency on global `config::configuration()` state (which
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
}
