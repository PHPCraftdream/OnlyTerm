use crate::handle::RenderMsg;
use onlyterm_gpu_render::GpuFrame;

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
pub(super) fn dispatch_loop(
    rx: &std::sync::mpsc::Receiver<RenderMsg>,
    on_frame: &mut dyn FnMut(GpuFrame),
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
