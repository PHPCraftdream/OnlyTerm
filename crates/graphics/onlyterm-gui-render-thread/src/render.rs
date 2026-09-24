use crate::dispatch::dispatch_loop;
use crate::handle::RenderThreadSeed;
use ::window::WindowOps;
use onlyterm_gpu_render::backpressure::finish_in_flight_frame;
use onlyterm_gpu_render::{GpuFrame, WebGpuState};
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

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
/// `install_uncaptured_error_callback` in `onlyterm-gpu-render::context`).
pub(super) struct ClearSubmitStartedAtOnExit<'a>(pub(super) &'a Mutex<Option<Instant>>);
impl Drop for ClearSubmitStartedAtOnExit<'_> {
    fn drop(&mut self) {
        *self.0.lock() = None;
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
pub(super) fn render_thread_loop(seed: RenderThreadSeed) {
    let in_flight = Arc::clone(&seed.in_flight);
    let repaint_pending = Arc::clone(&seed.repaint_pending);
    let window_destroyed = Arc::clone(&seed.window_destroyed);
    let submit_started_at = Arc::clone(&seed.submit_started_at);
    let webgpu = Arc::clone(&seed.webgpu);
    let window = seed.window.clone();
    let resize_webgpu = Arc::clone(&seed.webgpu);
    let resize_window_destroyed = Arc::clone(&seed.window_destroyed);
    let on_renderer_error = seed.on_renderer_error;
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
                &*on_renderer_error,
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
            // Same reasoning as `submit_frame`'s `catch_unwind` in
            // `submit_one_frame`: a Rust panic escaping `resize` (i.e.
            // `Surface::configure`) used to take the render thread down
            // with it, invisibly, for the same reason. This does NOT catch
            // every way a driver call here can kill the process -- a raw
            // Win32 structured exception raised from inside DXGI/D3D12
            // itself (as opposed to a Rust `panic!`) propagates straight
            // through `catch_unwind` by design; Rust's unwinding only
            // intercepts its own panics, not arbitrary foreign SEH. Closing
            // that harder case needs either a vectored exception handler
            // willing to recover mid-instruction from a call whose
            // in-flight state is unknown, or moving GPU work into an
            // isolated child process -- both real architectural decisions,
            // not something to fold into this fix silently.
            if let Err(panic) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                resize_webgpu.resize(dims);
            })) {
                let msg = panic
                    .downcast_ref::<&str>()
                    .map(|s| s.to_string())
                    .or_else(|| panic.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "non-string panic payload".to_string());
                log::error!(
                    "render thread: resize panicked ({}); the render thread survives, but \
                     this window's surface may now be out of sync with its actual size",
                    msg
                );
                metrics::counter!("gui.render_thread.resize_panic").increment(1);
            }
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
    webgpu: &WebGpuState,
    window: &::window::Window,
    frame: GpuFrame,
    state: SubmitState<'_>,
    placeholder_cleared: &mut bool,
    on_renderer_error: &dyn Fn(::window::Window, String),
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
    let stall_ms = onlyterm_config::configuration().debug_render_thread_stall_ms;
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
                        on_renderer_error(win, reason);
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
pub(super) fn surface_error_needs_renderer_rebuild(err: &wgpu::SurfaceError) -> bool {
    !matches!(err, wgpu::SurfaceError::OutOfMemory)
}
