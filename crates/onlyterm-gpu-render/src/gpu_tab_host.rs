//! `onlyterm-gui.exe --gpu-tab-host` (task #650): a hidden mode of this same
//! binary that hosts one window's GPU rendering in its own OS process, for
//! `webgpu_engine: HostProcess` (see
//! docs/plans/2026-08-21-per-tab-gpu-process-isolation.md, Phase B, and the
//! `@ox` architecture review this session that settled on per-window rather
//! than per-tab granularity, silent respawn instead of a crash-visible
//! epitaph screen).
//!
//! This process has no window, no mux, no pty -- it owns a private
//! `ProcessGpuContext` (its own `wgpu::Instance`/`Adapter`/`Device`/`Queue`,
//! never shared with the parent process), a `wgpu::Surface` bound directly to
//! a DirectComposition composition-surface handle the parent hands it via
//! `AttachSurface` (`WindowGpuSurface::new_from_composition_surface_handle`),
//! and a mirrored copy of the window's glyph atlas texture, kept in sync via
//! `wire::WireFrame`'s atlas deltas read from stdin. A crash anywhere in this
//! process's GPU calls (the exact class of fault that used to take down the
//! whole `onlyterm-gui.exe` -- see the investigation doc) now only takes
//! down this one child; `HostProcessBackend` (task #651) respawns a
//! replacement, and since a DirectComposition surface keeps displaying its
//! last presented content after its producer dies (confirmed empirically
//! this session), nothing visibly drops while that happens.
//!
//! There is no CLI-supplied surface handle: attaching to a surface (both the
//! very first one and every respawn generation after it) goes through the
//! same `AttachSurface` message, so "first attach" and "reattach after
//! respawn" are one code path, not two -- this is also what would let a
//! future warm-standby pool (task #643) hand a pre-started, adapter-already-
//! initialized child a surface on demand instead of spawning fresh each time.
//!
//! Supervision mirrors `per_tab_process_isolation`'s pty-hosting children:
//! a Job Object (`windows_job::assign_to_kill_on_close_job`, set up by the
//! parent at spawn time) plus a `--supervise-pid` watcher thread here as a
//! fallback if the Job Object setup ever fails.
//!
//! stdout is a binary channel (the `Presented`/`Fatal` acks the parent reads
//! back) -- nothing in this module may ever use `println!`/`print!`; logging
//! goes through `log::` as usual, which writes to this process's own per-PID
//! log file, not stdout.

use crate::{wire, GpuDraw, GpuFrame, WebGpuState, WebGpuTexture};
use config::ConfigHandle;
use std::convert::TryFrom;
use std::io::{self, Write};
use window::bitmaps::{BitmapImage, Texture2d};
use window::{Dimensions, Point, Rect, Size};

pub struct GpuTabHostArgs {
    pub supervise_pid: u32,
}

/// Maintains the child's mirror of the window's glyph atlas: recreated
/// whenever the parent signals `atlas_reset` (the real atlas was recreated
/// too, see `wire::WireFrameHeader::atlas_reset`'s doc comment), otherwise
/// updated in place via the same `Texture2d::write` path the parent's own
/// glyph cache uses.
struct MirroredAtlas {
    texture: WebGpuTexture,
}

impl MirroredAtlas {
    fn new(state: &WebGpuState, width: u32, height: u32) -> anyhow::Result<Self> {
        Ok(Self {
            texture: WebGpuTexture::new(width, height, state)?,
        })
    }

    fn apply_updates(&self, updates: &[wire::AtlasUpdateRef<'_>]) -> anyhow::Result<()> {
        for update in updates {
            if !atlas_update_fits(
                self.texture.width(),
                self.texture.height(),
                update.x,
                update.y,
                update.width,
                update.height,
                update.pixels.len(),
            ) {
                anyhow::bail!(
                    "atlas update at ({}, {}) size {}x{} does not fit {}x{} mirror ({} bytes)",
                    update.x,
                    update.y,
                    update.width,
                    update.height,
                    self.texture.width(),
                    self.texture.height(),
                    update.pixels.len(),
                );
            }
            let rect = Rect::new(
                Point::new(update.x as isize, update.y as isize),
                Size::new(update.width as isize, update.height as isize),
            );
            let image = BorrowedAtlasImage {
                pixels: update.pixels,
                width: update.width as usize,
                height: update.height as usize,
            };
            self.texture.write(rect, &image);
        }
        Ok(())
    }
}

fn atlas_update_fits(
    atlas_width: usize,
    atlas_height: usize,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    pixel_len: usize,
) -> bool {
    let Some(x) = usize::try_from(x).ok() else {
        return false;
    };
    let Some(y) = usize::try_from(y).ok() else {
        return false;
    };
    let Some(width) = usize::try_from(width).ok() else {
        return false;
    };
    let Some(height) = usize::try_from(height).ok() else {
        return false;
    };
    let Some(x_end) = x.checked_add(width) else {
        return false;
    };
    let Some(y_end) = y.checked_add(height) else {
        return false;
    };
    let Some(expected_pixels) = width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(4))
    else {
        return false;
    };
    x_end <= atlas_width && y_end <= atlas_height && pixel_len == expected_pixels
}

/// A read-only [`BitmapImage`] over pixels that still live in the wire read
/// buffer.
///
/// `Image::from_raw` takes ownership, which would mean copying every atlas
/// update straight back out of the buffer it was just read into.
/// `WebGpuTexture::write` only ever *reads* (through `pixel_data_slice`)
/// before handing the bytes to `Queue::write_texture`, so borrowing is
/// enough and that copy is avoidable.
struct BorrowedAtlasImage<'a> {
    pixels: &'a [u8],
    width: usize,
    height: usize,
}

impl BitmapImage for BorrowedAtlasImage<'_> {
    unsafe fn pixel_data(&self) -> *const u8 {
        self.pixels.as_ptr()
    }

    unsafe fn pixel_data_mut(&mut self) -> *mut u8 {
        // Deliberately unreachable: this adapter exists only to hand
        // already-decoded pixels to `WebGpuTexture::write`, which is
        // read-only, and a shared `&[u8]` could not honour a mutable
        // request anyway.
        unreachable!("BorrowedAtlasImage is read-only")
    }

    fn image_dimensions(&self) -> (usize, usize) {
        (self.width, self.height)
    }
}

fn build_gpu_frame(
    state: &WebGpuState,
    atlas: &mut Option<MirroredAtlas>,
    frame: &wire::WireFrameRef<'_>,
) -> anyhow::Result<GpuFrame> {
    if let Some((width, height)) = frame.atlas_reset {
        *atlas = Some(MirroredAtlas::new(state, width, height)?);
    }
    let mirrored = atlas.as_ref().ok_or_else(|| {
        anyhow::anyhow!("received a frame before any atlas_reset established a mirrored atlas size")
    })?;
    mirrored.apply_updates(&frame.atlas_updates)?;

    let draws = frame
        .draws
        .iter()
        .map(|instances| {
            use wgpu::util::DeviceExt;
            let instance_count = instances.len() as u32;
            let vertex_buffer =
                state
                    .device()
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("gpu-tab-host wire instance buffer"),
                        usage: wgpu::BufferUsages::VERTEX,
                        contents: bytemuck::cast_slice(instances),
                    });
            GpuDraw {
                vertex_buffer,
                instance_count,
            }
        })
        .collect();

    Ok(GpuFrame {
        draws,
        atlas: wgpu::Texture::clone(mirrored.texture.texture()),
        uniform: frame.uniform,
    })
}

/// Rebuilds `state`/`atlas`/`dimensions` for a new (or first) surface
/// generation. Shared by the initial attach and every respawn reattach --
/// there is deliberately only one code path for both.
fn attach_surface(
    config: &ConfigHandle,
    attach: wire::WireAttachSurface,
) -> anyhow::Result<(WebGpuState, Dimensions)> {
    let dimensions = Dimensions {
        pixel_width: attach.width as usize,
        pixel_height: attach.height as usize,
        dpi: ::window::default_dpi() as usize,
    };

    // SAFETY: `attach.surface_handle` is a composition-surface handle the
    // parent duplicated into this process specifically for this generation
    // (`HostProcessBackend::spawn`/respawn), and the parent keeps its own
    // reference (and the visual displaying it) alive until it has confirmed
    // this generation's first `Presented` ack, satisfying
    // `new_headless_from_composition_surface_handle`'s safety contract.
    let state = futures::executor::block_on(unsafe {
        WebGpuState::new_headless_from_composition_surface_handle(
            attach.surface_handle as *mut core::ffi::c_void,
            dimensions,
            config,
        )
    })?;

    Ok((state, dimensions))
}

pub fn run(args: GpuTabHostArgs, config: ConfigHandle) -> anyhow::Result<()> {
    onlyterm_client::client::parent_watcher::spawn_parent_watcher(args.supervise_pid);

    let stdin = io::stdin();
    let mut reader = stdin.lock();
    let stdout = io::stdout();
    let mut writer = stdout.lock();

    let mut state: Option<WebGpuState> = None;
    let mut dimensions = Dimensions {
        pixel_width: 0,
        pixel_height: 0,
        dpi: ::window::default_dpi() as usize,
    };
    let mut atlas: Option<MirroredAtlas> = None;
    let mut presented_seq: u64 = 0;
    // One buffer serves every message for this process's lifetime, and each
    // frame is consumed as borrowed views straight out of it (see
    // `wire::read_message_into`). Reading into a fresh allocation per frame
    // -- and then copying the draws and atlas pixels back out into owned
    // `Vec`s -- was the same never-reused large-allocation pattern that
    // exhausted memory on the parent side.
    let mut wire_body = Vec::new();

    loop {
        let message = match wire::read_message_into(&mut reader, &mut wire_body) {
            Ok(Some(m)) => m,
            Ok(None) => {
                log::info!("gpu-tab-host: parent closed the control channel, exiting");
                break;
            }
            Err(err) => {
                log::error!("gpu-tab-host: control channel read failed: {err}; exiting");
                break;
            }
        };

        match message {
            wire::WireMessageRef::Shutdown => {
                log::info!("gpu-tab-host: received Shutdown, exiting");
                break;
            }
            wire::WireMessageRef::AttachSurface(attach) => {
                atlas = None;
                match attach_surface(&config, attach) {
                    Ok((new_state, new_dimensions)) => {
                        state = Some(new_state);
                        dimensions = new_dimensions;
                        log::info!(
                            "gpu-tab-host: attached to a new surface generation ({}x{})",
                            dimensions.pixel_width,
                            dimensions.pixel_height
                        );
                    }
                    Err(err) => {
                        log::error!("gpu-tab-host: failed to attach to surface: {err:#}");
                        let _ = wire::write_fatal(&mut writer, 1);
                        return Err(err);
                    }
                }
            }
            wire::WireMessageRef::Resize(resize) => {
                if let Some(state) = &state {
                    dimensions.pixel_width = resize.width as usize;
                    dimensions.pixel_height = resize.height as usize;
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        state.resize(dimensions);
                    }));
                    if result.is_err() {
                        log::error!(
                            "gpu-tab-host: resize panicked; exiting so the parent can respawn us"
                        );
                        let _ = wire::write_fatal(&mut writer, 2);
                        break;
                    }
                } else {
                    log::warn!("gpu-tab-host: ignoring Resize before any AttachSurface");
                }
            }
            wire::WireMessageRef::Frame(frame) => {
                let Some(state) = &state else {
                    log::warn!("gpu-tab-host: ignoring Frame before any AttachSurface");
                    continue;
                };
                let gpu_frame = match build_gpu_frame(state, &mut atlas, &frame) {
                    Ok(gpu_frame) => gpu_frame,
                    Err(err) => {
                        log::error!("gpu-tab-host: failed to build a frame from the wire: {err:#}");
                        // Continuing would leave the child rendering against
                        // a stale/partial atlas and would repeat the same
                        // validation failure on every frame. Exit cleanly so
                        // the parent can respawn us and force a full atlas
                        // resync instead.
                        let _ = wire::write_fatal(&mut writer, 4);
                        let _ = writer.flush();
                        break;
                    }
                };

                // A Rust panic inside GPU submission should not kill a
                // process that could keep serving the next frame; a raw SEH
                // fault still will (unwinding cannot cross that), which is
                // exactly the point of this process boundary -- it dies,
                // the parent respawns a replacement, the on-screen content
                // stays frozen on the last good frame in the meantime.
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    state.submit_frame(gpu_frame)
                }));
                match result {
                    Ok(Ok(())) => {
                        presented_seq += 1;
                        if let Err(err) = wire::write_presented(&mut writer, presented_seq) {
                            log::error!(
                                "gpu-tab-host: failed to write Presented ack: {err}; exiting"
                            );
                            break;
                        }
                        if let Err(err) = writer.flush() {
                            log::error!(
                                "gpu-tab-host: failed to flush ack channel: {err}; exiting"
                            );
                            break;
                        }
                    }
                    Ok(Err(err)) => {
                        log::error!("gpu-tab-host: submit_frame failed: {err:?}");
                    }
                    Err(_) => {
                        log::error!(
                            "gpu-tab-host: submit_frame panicked; exiting so the parent can respawn us"
                        );
                        let _ = wire::write_fatal(&mut writer, 3);
                        break;
                    }
                }
            }
            wire::WireMessageRef::Presented(_) | wire::WireMessageRef::Fatal(_) => {
                // These only ever flow child->parent; seeing one here would
                // mean the parent's writer and this child's reader got
                // wired to the wrong pipes.
                log::error!(
                    "gpu-tab-host: received a child->parent-only message on stdin; ignoring"
                );
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::atlas_update_fits;

    #[test]
    fn atlas_update_accepts_exactly_in_bounds_pixels() {
        assert!(atlas_update_fits(128, 128, 120, 120, 8, 8, 8 * 8 * 4));
    }

    #[test]
    fn atlas_update_rejects_out_of_bounds_rectangles() {
        assert!(!atlas_update_fits(128, 128, 127, 0, 2, 1, 8));
        assert!(!atlas_update_fits(128, 128, 0, 127, 1, 2, 8));
    }

    #[test]
    fn atlas_update_rejects_overflow_and_wrong_pixel_lengths() {
        assert!(!atlas_update_fits(128, 128, u32::MAX, 0, 1, 1, 4));
        assert!(!atlas_update_fits(128, 128, 0, 0, 2, 2, 3));
    }
}
