use crate::render::render_thread_loop;
use onlyterm_gpu_render::backpressure::{in_flight_is_set, is_hung_given, mark_repaint_pending};
use onlyterm_gpu_render::{FrameForm, GpuFrame, RenderBackend, SubmittableFrame, WebGpuState};
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// A message sent from the GUI thread to a window's dedicated render
/// thread.
pub enum RenderMsg {
    /// A fully built frame ready to submit to the GPU. The render thread
    /// calls `WebGpuState::submit_frame` with this.
    Frame(GpuFrame),
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
    pub webgpu: std::sync::Arc<WebGpuState>,
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
    /// Invoked on the render thread when a non-transient surface error needs
    /// GUI-thread renderer recovery. The GUI supplies this callback so this
    /// crate does not depend on `TermWindow`.
    pub on_renderer_error: Box<dyn Fn(::window::Window, String) + Send + Sync>,
}

impl RenderBackend for RenderThreadHandle {
    fn frame_form(&self) -> FrameForm {
        FrameForm::InProcess
    }
    fn send_resize(&self, dims: ::window::Dimensions) {
        RenderThreadHandle::send_resize(self, dims)
    }
    fn send_frame(&self, frame: SubmittableFrame) {
        match frame {
            SubmittableFrame::InProcess(frame) => RenderThreadHandle::send_frame(self, frame),
            SubmittableFrame::Wire(_) => {
                // Can't happen unless a caller ignores `frame_form()`: this
                // backend answers `FrameForm::InProcess`, never `Wire`.
                log::error!(
                    "RenderThreadHandle::send_frame received a Wire frame; \
                     dropping it (frame_form() said InProcess)"
                );
            }
        }
    }
    fn is_in_flight(&self) -> bool {
        RenderThreadHandle::is_in_flight(self)
    }
    fn set_repaint_pending(&self) {
        RenderThreadHandle::set_repaint_pending(self)
    }
    fn shutdown(&self) {
        RenderThreadHandle::shutdown(self)
    }
    fn render_thread_is_hung(&self) -> bool {
        RenderThreadHandle::render_thread_is_hung(self)
    }
    fn render_thread_has_died(&self) -> bool {
        RenderThreadHandle::render_thread_has_died(self)
    }
    fn teardown_sentinel(&self) -> std::sync::Weak<dyn std::any::Any + Send + Sync> {
        RenderThreadHandle::teardown_sentinel(self)
    }
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
    /// read by `render_thread_is_hung()`, which in turn is polled by
    /// `TermWindow`'s per-window render-thread hang supervisor (task #223).
    submit_started_at: Arc<Mutex<Option<Instant>>>,
    /// `Weak` half of this render thread's teardown sentinel (task #292).
    /// `spawn` creates a fresh, dedicated `Arc<()>` for this purpose and
    /// moves the *only* strong reference onto the spawned thread itself,
    /// where it is held until strictly after `render_thread_loop` (and
    /// therefore every `Arc<WebGpuState>` clone the thread closed over:
    /// `seed.webgpu`, and `render_thread_loop`'s own `webgpu`/
    /// `resize_webgpu` locals) has returned -- see `spawn`'s body for the
    /// exact ordering. Handed out (type-erased) via `teardown_sentinel()` to
    /// `TermWindow::begin_renderer_rebuild`, which stashes it alongside the
    /// retired WebGpu child HWND instead of a `Weak<WebGpuState>` (task
    /// #292's fix for the use-after-free race a raw `Weak<WebGpuState>`
    /// strong-count read left open: `Arc::drop` decrements the strong count
    /// *before* running the value's own `drop_in_place`, so reading
    /// `Weak<WebGpuState>::strong_count() == 0` does not prove
    /// `WebGpuState`/its `wgpu::Surface` has actually finished tearing down
    /// -- only that the last strong reference started being dropped. This
    /// sentinel instead reports zero strong references only once the
    /// spawned thread closure has itself moved past `render_thread_loop`'s
    /// return, i.e. strictly after every `Arc<WebGpuState>` clone on that
    /// thread has already been fully dropped.).
    teardown_sentinel: std::sync::Weak<()>,
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
        // Task #292: a dedicated, single-purpose `Arc<()>` whose only job is
        // to prove "this render thread has fully returned from
        // `render_thread_loop`, including dropping every `Arc<WebGpuState>`
        // clone it held" -- see `teardown_sentinel`'s doc comment for why
        // that's a strictly stronger guarantee than reading
        // `Weak<WebGpuState>::strong_count() == 0` directly. The only
        // strong reference is moved into the thread closure below and held
        // there, past the `render_thread_loop(seed)` call, until this
        // thread function itself returns; nothing else ever clones it, so
        // `teardown_sentinel`'s `Weak` reports zero strong references
        // exactly once, strictly after that point.
        let teardown_sentinel_strong = Arc::new(());
        let teardown_sentinel = Arc::downgrade(&teardown_sentinel_strong);
        let name = format!("render-{window_id}");
        let builder = std::thread::Builder::new().name(name);
        match builder.spawn(move || {
            render_thread_loop(seed);
            // Drop the sentinel's only strong reference here, strictly
            // after `render_thread_loop` has returned (and therefore after
            // every `Arc<WebGpuState>` clone it closed over -- `seed.webgpu`
            // moved in above, plus `render_thread_loop`'s own local
            // `webgpu`/`resize_webgpu` clones -- has already been dropped by
            // that function's own end-of-scope cleanup). This is what makes
            // `teardown_sentinel().strong_count() == 0` a genuine "WebGpu
            // teardown has fully completed on this thread" signal instead of
            // merely "the last `Arc<WebGpuState>`'s refcount hit zero",
            // which -- since `Arc::drop` decrements the count before running
            // the value's `drop_in_place` -- would still leave a window
            // where the surface/swapchain teardown is in progress.
            drop(teardown_sentinel_strong);
        }) {
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
                    teardown_sentinel,
                })
            }
            Err(err) => {
                log::error!("Failed to spawn render thread: {:#}", err);
                None
            }
        }
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

    /// Whether a frame sent via `send_frame` is still being processed by the
    /// render thread. The GUI thread checks this before building a `GpuFrame`
    /// to avoid writing to persistent GPU instance buffers that the in-flight
    /// frame may still be reading.
    pub fn is_in_flight(&self) -> bool {
        in_flight_is_set(&self.in_flight)
    }

    /// Records that a fresh repaint is needed once the currently in-flight
    /// frame finishes submitting. The render thread checks this after each
    /// frame and calls `window.invalidate()` if set.
    pub fn set_repaint_pending(&self) {
        mark_repaint_pending(&self.repaint_pending)
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
    pub fn send_frame(&self, frame: GpuFrame) {
        // Same handshake as the shared backpressure helpers, so this swap
        // must use the same total order (SeqCst).
        if self.in_flight.swap(true, Ordering::SeqCst) {
            // A frame is already in flight; drop this one (its buffers are
            // released here) instead of queueing, and remember to ask for
            // a fresh repaint once the in-flight frame finishes.
            drop(frame);
            // Via the shared helper rather than an inline store, so this
            // side of the handshake can't drift out of sync with the
            // ordering the rest of it (and the test that covers it) relies
            // on -- see `in_flight_is_set`.
            mark_repaint_pending(&self.repaint_pending);
            metrics::counter!("gui.render_thread.frames_dropped").increment(1);
            return;
        }
        if self.tx.send(RenderMsg::Frame(frame)).is_err() {
            // Thread is already gone; undo the in_flight flag we just set
            // so we don't wedge back-pressure checks forever (there's
            // nothing else useful to do here -- the window is going
            // away).
            self.in_flight.store(false, Ordering::SeqCst);
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
    /// with no logging/metrics of its own -- `TermWindow`'s per-window
    /// render-thread hang supervisor (task #223,
    /// `TermWindow::check_render_thread_hang_tick`) is the caller that does
    /// its own edge-detection/logging on top of this call.
    pub fn render_thread_is_hung(&self) -> bool {
        let threshold =
            Duration::from_millis(config::configuration().render_thread_hang_threshold_ms);
        is_hung_given(&self.submit_started_at, threshold)
    }

    /// Returns true if this window's render thread has *died* -- exited
    /// `render_thread_loop` without anyone asking it to.
    ///
    /// This is a different failure from `render_thread_is_hung`, and it is
    /// the one that used to be invisible. A panic inside a GPU call unwinds
    /// the render thread and `std::thread` swallows it, so the process
    /// survives with no thread left to turn `Frame` messages into pixels:
    /// the window keeps pumping its message loop (Windows still reports it
    /// as responding) and never paints again. Observed live -- ConPTY
    /// window frozen for 17 minutes with a `Surface::present` panic as the
    /// last line in its log and no render thread in the dump.
    ///
    /// It reads as *not* hung, and deliberately so: `ClearSubmitStartedAtOnExit`
    /// clears `submit_started_at` while unwinding, precisely so a dead
    /// thread stops being misreported as a stuck one (which used to make
    /// the supervisor rebuild replacements in a loop). That fix left the
    /// death itself with no signal at all, which is what this predicate
    /// restores.
    ///
    /// The sentinel's last strong reference is dropped by the thread
    /// closure strictly after `render_thread_loop` returns, so a zero
    /// strong count means "that thread is finished", not merely "it is
    /// unwinding". `window_destroyed` distinguishes the two ways it can
    /// finish: `shutdown()` sets that flag *before* sending `Shutdown`, so
    /// an orderly teardown is never reported as a death.
    pub fn render_thread_has_died(&self) -> bool {
        !self.window_destroyed.load(Ordering::Acquire) && self.teardown_sentinel.strong_count() == 0
    }

    /// Type-erased `Weak` handle to this render thread's teardown sentinel
    /// (task #292), for `TermWindow::begin_renderer_rebuild` to stash
    /// alongside the retired WebGpu child HWND via
    /// `Window::recreate_webgpu_child_window`, in place of a
    /// `Weak<WebGpuState>` obtained by downgrading `self.webgpu` directly.
    ///
    /// Returned as `Weak<dyn Any + Send + Sync>` (rather than a bare
    /// `Weak<()>`) purely to match `recreate_webgpu_child_window`'s existing
    /// type-erased signature -- `window` (this crate's sibling) cannot name
    /// `WebGpuState` and was already written generically against `dyn Any`;
    /// `()` implements `Any + Send + Sync` just as well as `WebGpuState`
    /// did; nothing downcasts either one, only `strong_count()` is ever
    /// read. See `teardown_sentinel`'s field doc comment for why this
    /// `Weak`'s strong count reaching zero is the correct signal (proves
    /// `WebGpuState`/its surface have actually finished tearing down),
    /// unlike a `Weak<WebGpuState>` obtained from the caller's own `Arc`
    /// (whose count can read zero while `WebGpuState::drop` is still
    /// running on this render thread).
    pub fn teardown_sentinel(&self) -> std::sync::Weak<dyn std::any::Any + Send + Sync> {
        self.teardown_sentinel.clone() as std::sync::Weak<dyn std::any::Any + Send + Sync>
    }

    #[cfg(test)]
    pub(super) fn for_test(window_destroyed: bool, sentinel: &Arc<()>) -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        std::mem::forget(rx);
        Self {
            tx,
            in_flight: Arc::new(AtomicBool::new(false)),
            repaint_pending: Arc::new(AtomicBool::new(false)),
            window_destroyed: Arc::new(AtomicBool::new(window_destroyed)),
            submit_started_at: Arc::new(Mutex::new(None)),
            teardown_sentinel: Arc::downgrade(sentinel),
        }
    }
}
