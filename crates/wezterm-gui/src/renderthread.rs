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
//! Gated behind `config::webgpu_render_thread`, which now defaults to
//! `true` (221.8 flipped the default) -- this render thread is the live
//! path for GPU frame submission, not dormant scaffolding.

use ::window::WindowOps;
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

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
    pub fn send_frame(&self, frame: crate::termwindow::webgpu::GpuFrame) {
        if self.in_flight.swap(true, Ordering::AcqRel) {
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
}

/// Clears `submit_started_at` back to `None` when dropped -- including via
/// unwind, if the call this guards panics. A plain `*submit_started_at.lock()
/// = None;` placed after that call only runs on the non-panicking path, so a
/// panic left it stuck at `Some(..)` forever: the render thread that
/// panicked is dead (`std::thread` catches the unwind so the process
/// survives, but nothing is left to process a `Frame` message again), yet
/// `is_hung_given` kept reading it as merely running-too-long, so the hang
/// supervisor treated a dead thread as "stuck" and kept rebuilding a
/// replacement it could never actually hand new frames to (confirmed live:
/// this happened during a real wgpu-error-turned-panic crash, before that
/// panic was itself fixed to no longer happen -- see
/// `install_uncaptured_error_callback` in `termwindow::webgpu::context`).
struct ClearSubmitStartedAtOnExit<'a>(&'a Mutex<Option<Instant>>);
impl Drop for ClearSubmitStartedAtOnExit<'_> {
    fn drop(&mut self) {
        *self.0.lock() = None;
    }
}

/// The actual "is a call that's been running since `submit_started_at` older
/// than `threshold`" predicate, split out from `render_thread_is_hung` so it
/// can be unit tested with a synthetic threshold instead of depending on
/// live global config state.
fn is_hung_given(submit_started_at: &Mutex<Option<Instant>>, threshold: Duration) -> bool {
    match *submit_started_at.lock() {
        Some(start) => start.elapsed() >= threshold,
        None => false,
    }
}

/// GUI-thread side of the in-flight/repaint-pending handshake: is a frame
/// still being processed by the render thread? Backs
/// `RenderThreadHandle::is_in_flight`; split out so the two-thread stress
/// test below calls this exact function rather than a hand-copied stand-in
/// that could silently drift out of sync with it.
///
/// SeqCst (not Acquire): paired with `mark_repaint_pending` and
/// `finish_in_flight_frame`'s SeqCst ops, this closes a store-buffer race in
/// the lost-wakeup check in `call_draw_webgpu` (which stores to
/// `repaint_pending` and then re-checks `in_flight`, a store followed by a
/// load of a *different* location). Release/Acquire alone doesn't order a
/// store against a later load of another location on the same thread (only
/// SeqCst's total order does), which could let both this thread and the
/// render thread simultaneously observe stale values and each assume the
/// other will request the repaint.
fn in_flight_is_set(in_flight: &AtomicBool) -> bool {
    in_flight.load(Ordering::SeqCst)
}

/// GUI-thread side: record that a fresh repaint is needed once the
/// currently in-flight frame finishes. Backs
/// `RenderThreadHandle::set_repaint_pending`; see `in_flight_is_set` for why
/// SeqCst and why this is split out as a standalone, directly-testable
/// function.
fn mark_repaint_pending(repaint_pending: &AtomicBool) {
    repaint_pending.store(true, Ordering::SeqCst);
}

/// Render-thread side of the same handshake, run once a frame finishes
/// submitting: clears `in_flight`, then observes-and-clears
/// `repaint_pending`. Returns `true` if the caller should invalidate the
/// window (a repaint was requested while this frame was in flight). Backs
/// `submit_one_frame`'s tail; see `in_flight_is_set` for why SeqCst and why
/// this is split out as a standalone, directly-testable function.
fn finish_in_flight_frame(in_flight: &AtomicBool, repaint_pending: &AtomicBool) -> bool {
    in_flight.store(false, Ordering::SeqCst);
    repaint_pending.swap(false, Ordering::SeqCst)
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
    // Task #407: local (not `Arc`/atomic -- this closure only ever runs on
    // this single render thread, one message at a time) one-shot guard so
    // `submit_one_frame`'s `Window::clear_placeholder_background` call below
    // fires at most once per window, right after the first successful
    // `submit_frame`/`present()`, instead of on every frame forever. See
    // `submit_one_frame`'s doc comment for why this needs to happen here
    // rather than in `TermWindow::paint_impl`.
    let mut placeholder_cleared = false;
    dispatch_loop(
        &seed.rx,
        &mut |frame| {
            submit_one_frame(
                &webgpu,
                &window,
                frame,
                SubmitState {
                    in_flight: &in_flight,
                    repaint_pending: &repaint_pending,
                    window_destroyed: &window_destroyed,
                    submit_started_at: &submit_started_at,
                },
                &mut placeholder_cleared,
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

/// The `Arc`-shared back-pressure/hang-visibility state `submit_one_frame`
/// needs, grouped into one borrow so that function takes one parameter for
/// all of it instead of four separate references (keeps it under clippy's
/// `too_many_arguments` threshold now that task #407 added
/// `placeholder_cleared` on top). Purely a borrow-side grouping -- the
/// fields themselves are still the same `Arc` clones `render_thread_loop`
/// already held individually, just referenced through one struct here.
struct SubmitState<'a> {
    in_flight: &'a AtomicBool,
    repaint_pending: &'a AtomicBool,
    window_destroyed: &'a AtomicBool,
    submit_started_at: &'a Mutex<Option<Instant>>,
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
///
/// `placeholder_cleared` (task #407): `*placeholder_cleared` starts `false`
/// and this function flips it to `true` and calls
/// `Window::clear_placeholder_background` the first time `webgpu.submit_frame`
/// (which does the real `Queue::submit` + `SurfaceTexture::present`) actually
/// succeeds. This -- not `TermWindow::paint_impl` returning from `call_draw`
/// -- is the true "a real frame has been presented" event on this path:
/// `call_draw` only enqueues the frame via `RenderThreadHandle::send_frame`
/// and returns immediately, well before this function (running here, on the
/// render thread, potentially one or more GUI-thread iterations later) picks
/// it up and actually presents it. Clearing the GDI placeholder any earlier
/// than this left the WebGpu child window's swapchain surface exposed to DWM
/// composition before it had ever been presented to -- invisible against the
/// desktop, but showing another overlapping OnlyTerm window's real content
/// through it, which is what task #407 reported.
fn submit_one_frame(
    webgpu: &crate::termwindow::webgpu::WebGpuState,
    window: &::window::Window,
    frame: crate::termwindow::webgpu::GpuFrame,
    state: SubmitState<'_>,
    placeholder_cleared: &mut bool,
) {
    let SubmitState {
        in_flight,
        repaint_pending,
        window_destroyed,
        submit_started_at,
    } = state;
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
    // `submit_started_at` must cover the debug stall too, not just the real
    // `submit_frame` call below: this is what task #253's manual hang/rebuild
    // verification (and anyone else exercising `debug_render_thread_stall_ms`)
    // relies on to simulate a stuck GPU driver call. Setting it only around
    // `submit_frame` (which is fast) would mean `render_thread_is_hung()`
    // never observes the artificial stall as a hang at all, defeating the
    // point of the debug knob.
    let start = std::time::Instant::now();
    *submit_started_at.lock() = Some(start);
    let stall_ms = config::configuration().debug_render_thread_stall_ms;
    if stall_ms > 0 {
        std::thread::sleep(std::time::Duration::from_millis(stall_ms));
    }
    // See `ClearSubmitStartedAtOnExit`'s doc comment: this clears
    // `submit_started_at` even if `webgpu.submit_frame` panics.
    //
    // And it can panic: wgpu's `Surface::present` reports failure by
    // panicking rather than returning, and it is not routed through the
    // uncaptured-error callback that `WebGpuState` installs for device
    // errors. Observed live -- a window whose GPU ran out of memory logged
    // `Present failed: Not enough memory resources (0x8007000E)` followed by
    // a panic in `wgpu_core.rs`, and that was the last thing it ever logged:
    // the panic unwound the render thread out of existence, leaving the
    // window pumping its message loop forever with nobody to paint it.
    //
    // Catching it here keeps the thread alive so the frame is merely lost,
    // which is the same outcome as any other failed submit, and lets
    // `submit_one_frame` fall through to its normal error handling below.
    // `render_thread_has_died` covers the case where a panic escapes some
    // *other* call on this thread; this arm covers the one that is known to
    // happen, and recovers from it without a whole renderer rebuild.
    let result = {
        let _clear_on_exit = ClearSubmitStartedAtOnExit(submit_started_at);
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| webgpu.submit_frame(frame)))
        {
            Ok(result) => result,
            Err(panic) => {
                let msg = panic
                    .downcast_ref::<&str>()
                    .map(|s| s.to_string())
                    .or_else(|| panic.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "non-string panic payload".to_string());
                log::error!(
                    "render thread: submit_frame panicked ({}); dropping this frame and \
                     keeping the render thread alive",
                    msg
                );
                metrics::counter!("gui.render_thread.submit_panic").increment(1);
                // Reported as `Other` rather than `OutOfMemory`: the observed
                // panic was an out-of-memory `present`, but nothing here can
                // confirm that for the next one, and the real reason is in
                // the log line above. `Other` lands in the same arm below as
                // every non-`Lost`/`Outdated` variant, which signals the GUI
                // thread to rebuild this window's renderer through the
                // existing circuit breaker.
                Err(wgpu::SurfaceError::Other)
            }
        }
    };
    if result.is_ok() && !*placeholder_cleared {
        // Task #407: this is the first frame this render thread has ever
        // actually presented (see this function's doc comment) -- now, and
        // only now, is it safe to tear down the Windows GDI placeholder.
        // `Window::clear_placeholder_background` marshals onto the GUI
        // thread itself (via `Connection::with_window_inner`) and is
        // idempotent (`Option::take` on the GUI-thread side), so it's safe
        // to call directly from here.
        *placeholder_cleared = true;
        window.clear_placeholder_background();
    }
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
                if window_destroyed.load(Ordering::Acquire) {
                    // Same rationale as the Lost/Outdated branch above: the
                    // window is gone (or on its way out), so there is no
                    // `TermWindow` left to rebuild and signaling one would be
                    // pointless (and could race a torn-down window).
                    log::debug!(
                        "render thread: surface error after window destruction, \
                         skipping rebuild trigger"
                    );
                } else {
                    // Unlike Lost/Outdated (a merely-stale swapchain that
                    // `WebGpuState::reconfigure` fixes), every other
                    // `SurfaceError` variant (OutOfMemory, Timeout, etc.)
                    // means the surface/device itself is in trouble in a way
                    // a lightweight reconfigure won't fix. The render thread
                    // can't call `TermWindow::begin_renderer_rebuild`
                    // directly (GUI-thread-only state), so signal back to the
                    // GUI thread via the same `TermWindowNotif::Apply`
                    // mechanism `schedule_render_thread_hang_check` uses,
                    // reusing task #253's in-place rebuild (and its circuit
                    // breaker, so a persistently broken adapter that throws
                    // this on every frame doesn't loop-rebuild forever).
                    if !surface_error_needs_renderer_rebuild(&other) {
                        // Out of memory is a *shortage*, not a broken device.
                        // Answering it by tearing the renderer down and
                        // building a new one is both unnecessary and the
                        // single most dangerous thing available: rebuilding
                        // calls `CreateSwapChainForHwnd`, and a crash dump
                        // from this machine caught the GPU driver
                        // dereferencing NULL inside exactly that call while
                        // the system was still out of memory. The shortage
                        // that made us rebuild is what made the rebuild
                        // fatal.
                        //
                        // Drop the frame instead and ask for a repaint. If
                        // memory frees up, the next frame simply works; if
                        // the device really is broken, it will say so with
                        // an error that does need a rebuild, or the
                        // device-lost callback will.
                        log::warn!(
                            "render thread: dropping a frame after {:?}; not rebuilding \
                             the renderer, since recreating a device/swapchain while out \
                             of memory is how the driver gets pushed over",
                            other
                        );
                        metrics::counter!("gui.render_thread.frame_dropped_out_of_memory")
                            .increment(1);
                        window.invalidate();
                    } else {
                        let win = window.clone();
                        let reason = format!(
                            "this window's render thread hit a GPU surface error ({:?}) \
                             other than the transient Lost/Outdated case",
                            other
                        );
                        window.notify(crate::termwindow::TermWindowNotif::Apply(Box::new(
                            move |tw| {
                                tw.handle_render_error_recovery(&win, &reason);
                            },
                        )));
                    }
                }
            }
        }
    }
    metrics::histogram!("gui.render_thread.submit").record(start.elapsed());
    // Note: if the reconfigure branch above already called
    // `window.invalidate()`, and `repaint_pending` also happens to be true
    // here, this can call `invalidate()` again. That's harmless: it just
    // requests a repaint, and requesting one twice back-to-back doesn't
    // double-render anything.
    if finish_in_flight_frame(in_flight, repaint_pending) {
        window.invalidate();
    }
}

/// Whether a surface error means the device/surface is genuinely broken and
/// has to be rebuilt, or merely that this one frame could not be produced.
///
/// `OutOfMemory` is the interesting case, and it is deliberately *not* a
/// rebuild. It says the system could not find memory right now, which says
/// nothing about the device being usable a moment later -- and the recovery
/// it used to trigger (drop the device and surface, create new ones) runs
/// `CreateSwapChainForHwnd`, which is where a driver starved of memory is
/// most likely to fail. That is not hypothetical: a crash dump from a
/// machine running this code caught the GPU driver dereferencing NULL inside
/// that call, moments after an out-of-memory submit had asked for a rebuild.
/// Answering a shortage with the most allocation-hungry operation available
/// turned a dropped frame into a lost window.
///
/// `Lost`/`Outdated` never reach here (handled earlier by reconfiguring).
/// Everything else does describe a device in trouble, and still rebuilds.
fn surface_error_needs_renderer_rebuild(err: &wgpu::SurfaceError) -> bool {
    !matches!(err, wgpu::SurfaceError::OutOfMemory)
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

    /// Builds a `RenderThreadHandle` around a caller-owned teardown sentinel,
    /// with no GPU and no real thread. `spawn` cannot be used here (it needs
    /// a `WebGpuState`, hence a GPU adapter), but every field the liveness
    /// predicates read is plain shared state, so they can be exercised
    /// directly. Returns the handle plus the sentinel's only strong
    /// reference: dropping it is what "the render thread returned" looks
    /// like to the handle.
    fn handle_with_sentinel(window_destroyed: bool) -> (RenderThreadHandle, Arc<()>) {
        let (tx, rx) = mpsc::channel();
        // Keep the receiver alive; a disconnected channel is a different
        // condition from the one under test.
        std::mem::forget(rx);
        let strong = Arc::new(());
        let handle = RenderThreadHandle {
            tx,
            in_flight: Arc::new(AtomicBool::new(false)),
            repaint_pending: Arc::new(AtomicBool::new(false)),
            window_destroyed: Arc::new(AtomicBool::new(window_destroyed)),
            submit_started_at: Arc::new(Mutex::new(None)),
            teardown_sentinel: Arc::downgrade(&strong),
        };
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
}
