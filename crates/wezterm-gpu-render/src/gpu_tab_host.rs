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
use std::io::{self, Write};
use window::bitmaps::Texture2d;
use window::{Dimensions, Image, Point, Rect, Size};

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

    fn apply_updates(&self, updates: Vec<wire::AtlasUpdate>) {
        for update in updates {
            let rect = Rect::new(
                Point::new(update.x as isize, update.y as isize),
                Size::new(update.width as isize, update.height as isize),
            );
            let image =
                Image::from_raw(update.width as usize, update.height as usize, update.pixels);
            self.texture.write(rect, &image);
        }
    }
}

fn build_gpu_frame(
    state: &WebGpuState,
    atlas: &mut Option<MirroredAtlas>,
    frame: wire::WireFrame,
) -> anyhow::Result<GpuFrame> {
    if let Some((width, height)) = frame.atlas_reset {
        *atlas = Some(MirroredAtlas::new(state, width, height)?);
    }
    let mirrored = atlas.as_ref().ok_or_else(|| {
        anyhow::anyhow!("received a frame before any atlas_reset established a mirrored atlas size")
    })?;
    mirrored.apply_updates(frame.atlas_updates);

    let draws = frame
        .draws
        .into_iter()
        .map(|instances| {
            use wgpu::util::DeviceExt;
            let instance_count = instances.len() as u32;
            let vertex_buffer =
                state
                    .device()
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("gpu-tab-host wire instance buffer"),
                        usage: wgpu::BufferUsages::VERTEX,
                        contents: bytemuck::cast_slice(&instances),
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
    wezterm_client::client::parent_watcher::spawn_parent_watcher(args.supervise_pid);

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

    loop {
        let message = match wire::read_message(&mut reader) {
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
            wire::WireMessage::Shutdown => {
                log::info!("gpu-tab-host: received Shutdown, exiting");
                break;
            }
            wire::WireMessage::AttachSurface(attach) => {
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
            wire::WireMessage::Resize(resize) => {
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
            wire::WireMessage::Frame(frame) => {
                let Some(state) = &state else {
                    log::warn!("gpu-tab-host: ignoring Frame before any AttachSurface");
                    continue;
                };
                let gpu_frame = match build_gpu_frame(state, &mut atlas, frame) {
                    Ok(gpu_frame) => gpu_frame,
                    Err(err) => {
                        log::error!("gpu-tab-host: failed to build a frame from the wire: {err:#}");
                        continue;
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
            wire::WireMessage::Presented(_) | wire::WireMessage::Fatal(_) => {
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
