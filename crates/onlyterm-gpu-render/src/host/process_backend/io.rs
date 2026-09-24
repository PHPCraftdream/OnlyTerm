use super::*;
use std::io::Write;
use std::sync::atomic::Ordering;
use std::sync::mpsc::Receiver;
use windows::core::IUnknown;

pub(super) fn spawn_writer_thread(
    generation: u64,
    mut stdin: std::process::ChildStdin,
    rx: Receiver<HostToChildMsg>,
    draw_pool: wire::WireDrawPool,
) {
    std::thread::Builder::new()
        .name(format!("gpu-host-writer-{generation}"))
        .spawn(move || {
            // Reuse the serialized wire body across frames.  The draw
            // buffers have their own pool; this second buffer is the byte
            // representation produced after the draw buffers are borrowed.
            // Keeping both allocations alive across frames prevents the
            // custom allocator from retaining a fresh large arena for every
            // slightly-different terminal frame.
            let mut frame_body = Vec::new();
            for msg in rx {
                let result = match msg {
                    HostToChildMsg::AttachSurface {
                        surface_handle,
                        width,
                        height,
                    } => wire::write_attach_surface(&mut stdin, surface_handle, width, height),
                    HostToChildMsg::Frame(mut frame) => {
                        let res =
                            wire::write_frame_with_buffer(&mut stdin, &frame, &mut frame_body);
                        // Return draw buffers to the pool now that their
                        // contents have been serialized onto the wire.
                        // This is the return half of the pool contract:
                        // `build_wire_frame` took them, we give them back.
                        wire::pool_return_draws(&draw_pool, &mut frame.draws);
                        res
                    }
                    HostToChildMsg::Resize { width, height } => {
                        wire::write_resize(&mut stdin, width, height)
                    }
                    HostToChildMsg::Shutdown => {
                        let res = wire::write_shutdown(&mut stdin);
                        let _ = stdin.flush();
                        res
                    }
                };
                if let Err(err) = result.and_then(|()| stdin.flush()) {
                    log::warn!(
                        "gpu-host-writer-{generation}: write failed (child likely dead): {err}"
                    );
                    break;
                }
            }
        })
        .expect("failed to spawn gpu-host writer thread");
}

/// Spawns the reader thread for one child generation. Runs `reader_loop`
/// until the child's ack channel closes or errors, then always calls
/// `handle_child_death` exactly once -- regardless of which condition ended
/// the loop.
pub(super) fn spawn_reader_thread_with_death_handling(
    shared: Arc<Shared>,
    generation: u64,
    stdout: std::process::ChildStdout,
) {
    let shared_for_thread = Arc::clone(&shared);
    std::thread::Builder::new()
        .name(format!("gpu-host-reader-{generation}"))
        .spawn(move || {
            reader_loop(&shared_for_thread, generation, stdout);
            lifecycle::handle_child_death(&shared_for_thread, generation);
        })
        .expect("failed to spawn gpu-host reader thread");
}

fn reader_loop(shared: &Arc<Shared>, generation: u64, mut stdout: std::process::ChildStdout) {
    loop {
        match wire::read_message(&mut stdout) {
            Ok(Some(wire::WireMessage::Presented(presented))) => {
                on_presented(shared, generation, presented.seq);
            }
            Ok(Some(wire::WireMessage::Failed(failed))) => {
                on_failed(shared, generation, failed.seq);
            }
            Ok(Some(wire::WireMessage::Fatal(fatal))) => {
                log::error!(
                    "gpu-host generation {generation}: child reported Fatal({})",
                    fatal.code
                );
            }
            Ok(Some(_)) => {}
            Ok(None) => {
                log::info!("gpu-host generation {generation}: child closed its ack channel");
                return;
            }
            Err(err) => {
                log::warn!("gpu-host generation {generation}: ack channel read failed: {err}");
                return;
            }
        }
    }
}

fn on_presented(shared: &Arc<Shared>, generation: u64, _seq: u64) {
    {
        let current = shared.current.lock();
        let Some(current) = current.as_ref() else {
            return;
        };
        if current.generation != generation {
            // A stale ack from a generation we've already moved past.
            return;
        }
    }

    // SeqCst (was Release): the first half of the shared handshake's finish
    // step -- the exact two ops `backpressure::finish_in_flight_frame`
    // performs, with the visual swap in between.
    shared.in_flight.store(false, Ordering::SeqCst);
    *shared.submit_started_at.lock() = None;

    // First ack of a freshly (re)attached generation: swap the visual over
    // to it now -- never before this point, or a broken/blank child would
    // flash on screen instead of the previous generation's last good frame.
    let mut pending = shared.pending_surface_handles.lock();
    if let Some(handle) = pending.remove(&generation) {
        drop(pending);
        if let Err(err) = swap_visual_content(shared, handle) {
            log::error!("HostProcessBackend: failed to swap visual content: {err:#}");
        }
        // Real content has now actually landed on screen for the first time
        // (this generation's first ack) -- safe to tear down the Windows
        // GDI placeholder. Idempotent, so unconditional here is fine even
        // across respawns.
        (shared.clear_placeholder_background)();
        // Reset the respawn budget: a generation that reached its first
        // real frame is a success, not a lingering failure to hold against
        // future budget checks.
        shared.respawn_attempts.lock().clear();
    }

    if shared.repaint_pending.swap(false, Ordering::SeqCst) {
        // SeqCst (was AcqRel): the second half of the same handshake pair.
        // With Release/Acquire this pairing had no StoreLoad ordering
        // against the GUI thread's store to repaint_pending + re-check of
        // in_flight (see draw.rs's call_draw_webgpu and the doc comment on
        // `backpressure::in_flight_is_set`), so both sides could miss each
        // other's update and the freshly built frame dropped for good.
        if PRESENTED_PENDING_REPAINT_LOG.should_log(HANDSHAKE_LOG_RATE) {
            log::info!(
                "HostProcessBackend: ack from generation {generation} arrived with a \
                 repaint pending; invalidating (rate-limited to 1/s)"
            );
        }
        (shared.invalidate)();
    }
}

/// A child-reported, recoverable frame failure: the child received the
/// frame but `submit_frame` returned a recoverable surface error
/// (`Lost`/`Outdated`), reconfigured its surface, and dropped exactly that
/// frame -- see `gpu_tab_host`'s submit handling and `wire::WireFailed`.
///
/// Before this existed, the child logged and continued without acking
/// anything, so the parent's `in_flight` (set by `send_frame`) stayed `true`
/// forever: every later frame hit the back-pressure bailout and was
/// dropped, and `render_thread_is_hung` unconditionally reported `false`,
/// so nothing ever recovered -- the window froze for good.
///
/// Unlike `on_presented`, nothing was presented, so the visual must not be
/// swapped. What must happen is what `handle_child_death` arranges for its
/// own lost frame: release `in_flight`, and force the next repaint past the
/// GUI's frame-signature skip (`needs_full_resync`) so the dropped frame's
/// content is actually rebuilt and resent rather than skipped as
/// "unchanged".
fn on_failed(shared: &Arc<Shared>, generation: u64, _seq: u64) {
    {
        let current = shared.current.lock();
        let Some(current) = current.as_ref() else {
            return;
        };
        if current.generation != generation {
            // A stale ack from a generation we've already moved past.
            return;
        }
    }

    shared.in_flight.store(false, Ordering::SeqCst);
    *shared.submit_started_at.lock() = None;
    shared.needs_full_resync.store(true, Ordering::Release);
    metrics::counter!("gui.host_process.frames_failed_recovered").increment(1);
    if FAILED_FRAME_LOG.should_log(HANDSHAKE_LOG_RATE) {
        log::info!(
            "HostProcessBackend: generation {generation} failed to submit a frame \
             (recoverable surface error); requesting a full-resync repaint \
             (rate-limited to 1/s)"
        );
    }
    (shared.invalidate)();
}

fn swap_visual_content(shared: &Shared, handle: HANDLE) -> anyhow::Result<()> {
    // SAFETY: `handle` is a composition-surface handle this generation's
    // child just successfully presented into (we're here because its first
    // `Presented` ack arrived), still open (not yet closed by any
    // now-superseded generation), satisfying `CreateSurfaceFromHandle`'s
    // requirement that the handle be valid and usable to build a swapchain
    // on. `SetContent`/`Commit` are plain COM calls on a live device/visual
    // this struct owns for its whole lifetime.
    unsafe {
        let content: IUnknown = shared.dcomp_device.CreateSurfaceFromHandle(handle)?;
        shared.dcomp_visual.SetContent(&content)?;
        shared.dcomp_device.Commit()?;
    }
    Ok(())
}
