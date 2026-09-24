use super::*;
use crate::backpressure::{in_flight_is_set, is_hung_given, mark_repaint_pending};
use crate::{FrameForm, RenderBackend, SubmittableFrame};
use std::sync::atomic::Ordering;
use std::sync::Weak;

impl RenderBackend for HostProcessBackend {
    fn frame_form(&self) -> FrameForm {
        FrameForm::Wire {
            full_resync: self.shared.needs_full_resync.swap(false, Ordering::AcqRel),
        }
    }

    fn wants_atlas_mirroring(&self) -> bool {
        // The child process renders against its own `wgpu::Device`, so it
        // needs a pixel-for-pixel replay of this window's atlas; see the
        // trait method's doc comment for why answering this before the
        // atlas is built (rather than inferring it from `frame_form`) is
        // load-bearing.
        true
    }

    fn atlas_mirroring_failed(&self) {
        if !self.shared.demoted.swap(true, Ordering::AcqRel) {
            log::error!(
                "HostProcessBackend: atlas mirror memory budget exhausted; demoting this window to the in-process renderer"
            );
            metrics::counter!("gui.host_process.demoted_to_in_process").increment(1);
            self.shared.in_flight.store(false, Ordering::SeqCst);
            (self.shared.invalidate)();
        }
    }

    fn send_resize(&self, dims: Dimensions) {
        *self.shared.dimensions.lock() = dims;
        if let Some(current) = self.shared.current.lock().as_ref() {
            current
                .writer_tx
                .send(HostToChildMsg::Resize {
                    width: dims.pixel_width as u32,
                    height: dims.pixel_height as u32,
                })
                .ok();
        }
    }

    fn send_frame(&self, frame: SubmittableFrame) {
        let SubmittableFrame::Wire(frame) = frame else {
            log::error!(
                "HostProcessBackend::send_frame received an InProcess frame; dropping it \
                 (frame_form() said Wire)"
            );
            return;
        };
        // SeqCst: handshake op -- see backpressure.rs.
        if self.shared.in_flight.swap(true, Ordering::SeqCst) {
            mark_repaint_pending(&self.shared.repaint_pending);
            metrics::counter!("gui.host_process.frames_dropped").increment(1);
            return;
        }
        *self.shared.submit_started_at.lock() = Some(Instant::now());
        let Some(current) = self
            .shared
            .current
            .lock()
            .as_ref()
            .map(|c| c.writer_tx.clone())
        else {
            self.shared.in_flight.store(false, Ordering::SeqCst);
            return;
        };
        if current.send(HostToChildMsg::Frame(frame)).is_err() {
            // The writer thread for this generation is gone (it exits when
            // a write to the child's stdin fails, i.e. the child is dying
            // or dead). Clearing `in_flight` alone silently lost this
            // frame: nothing set `repaint_pending`, nothing invalidated,
            // and the GUI's `last_frame_signature` had already been
            // recorded -- so when the respawn's invalidate produced a paint
            // with identical content, the signature check skipped it and
            // the new generation never received a first frame. Force the
            // next paint past the signature skip. `submit_started_at` is
            // deliberately left running: if no respawn ever lands, it is
            // what makes `render_thread_is_hung` fire and the window's
            // supervisor rebuild.
            self.shared.in_flight.store(false, Ordering::SeqCst);
            self.shared.needs_full_resync.store(true, Ordering::Release);
            if SEND_QUEUED_FAILED_LOG.should_log(HANDSHAKE_LOG_RATE) {
                log::warn!(
                    "HostProcessBackend: frame could not be queued to this generation's \
                     writer (child dying or dead); next repaint resends with a full resync"
                );
            }
        }
    }

    fn is_in_flight(&self) -> bool {
        in_flight_is_set(&self.shared.in_flight)
    }

    fn set_repaint_pending(&self) {
        mark_repaint_pending(&self.shared.repaint_pending)
    }

    fn shutdown(&self) {
        self.shared.window_destroyed.store(true, Ordering::Release);
        // A torn-down backend must not leave a stale "submit in flight"
        // timestamp behind for `render_thread_is_hung` to misread as a
        // stall.
        *self.shared.submit_started_at.lock() = None;
        if let Some(current) = self.shared.current.lock().take() {
            current.writer_tx.send(HostToChildMsg::Shutdown).ok();
        }
    }

    fn render_thread_is_hung(&self) -> bool {
        // A respawn already in flight resolves its own stall: the backoff
        // window (up to 1s) routinely exceeds how long ago the last frame
        // was sent, and the supervisor piling a full renderer rebuild on
        // top of a working respawn would defeat it (and demote this window
        // to the in-process renderer unnecessarily). The gate clears when
        // the respawn attempt runs, either way: on success the new
        // generation resets the clock, and on failure the check below
        // becomes the recovery path.
        if self.shared.respawn_pending.load(Ordering::SeqCst) {
            return false;
        }
        let threshold =
            Duration::from_millis(onlyterm_config::configuration().render_thread_hang_threshold_ms);
        is_hung_given(&self.shared.submit_started_at, threshold)
    }

    fn render_thread_has_died(&self) -> bool {
        self.shared.demoted.load(Ordering::Acquire)
    }

    fn teardown_sentinel(&self) -> Weak<dyn std::any::Any + Send + Sync> {
        Arc::downgrade(&self.shared.teardown_sentinel_strong)
            as Weak<dyn std::any::Any + Send + Sync>
    }

    fn wire_draw_pool(&self) -> Option<wire::WireDrawPool> {
        Some(Arc::clone(&self.shared.draw_pool))
    }
}
