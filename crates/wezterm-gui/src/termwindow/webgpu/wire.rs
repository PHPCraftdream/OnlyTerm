//! Wire format for a `GpuFrame`'s worth of data when it has to cross a
//! process boundary (`webgpu_engine: PerTabProcess`, see
//! docs/plans/2026-08-21-per-tab-gpu-process-isolation.md, Phase B).
//!
//! `GpuFrame` (`termwindow::webgpu::GpuFrame`) holds live `wgpu::Buffer`/
//! `wgpu::Texture` handles that are only valid within the process/device
//! that created them, so it cannot be shipped to a child process as-is.
//! This module defines an equivalent that is either `Pod` or a plain
//! `Vec<u8>`, built from the same CPU-side data `GpuFrame` is built from
//! (`TripleVertexBuffer::instances`, `WebGpuTexture::write`'s pixel deltas)
//! instead of already-uploaded GPU resources.
//!
//! Both ends of the wire are the same binary (`onlyterm-gui.exe`, just
//! running in a different mode -- see task #650's `--gpu-tab-host`), so
//! there is no cross-version compatibility concern and no need for a serde
//! dependency: messages are a `u32` length prefix, a `u32` kind tag, then a
//! payload that is just the `bytemuck` bytes of the structs below.
//!
//! The wire is bidirectional, carried on two separate byte streams (the
//! child's stdin for parent->child, its stdout for child->parent -- see
//! `HostProcessBackend`, task #651): `Frame`/`Resize`/`Shutdown`/
//! `AttachSurface` flow parent->child; `Presented`/`Fatal` flow the other
//! way. Both directions share one `WireMessageKind`/`read_message` vocabulary
//! since which physical pipe is being read already tells a caller which
//! subset of kinds it can legally see -- there is no reason to duplicate the
//! framing code for a distinction the caller already knows structurally.
//! `AttachSurface`/respawn (task #651, informed by an empirical finding: a
//! DirectComposition composition surface keeps displaying its last presented
//! content even after its producer process dies, so a dead child's tab does
//! not visibly go blank while a replacement spawns) hands the child a fresh
//! composition-surface handle to bind a new swapchain to -- the surface
//! handle is no longer fixed at child-process startup (see `gpu_tab_host`'s
//! CLI args), it can be sent again over the wire whenever the parent needs
//! this child (or its replacement) to attach to a new surface generation.

use crate::quad::QuadInstance;
use crate::termwindow::webgpu::ShaderUniform;
use std::io::{self, Read, Write};

#[repr(u32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum WireMessageKind {
    Frame = 1,
    Resize = 2,
    Shutdown = 3,
    AttachSurface = 4,
    Presented = 5,
    Fatal = 6,
}

impl WireMessageKind {
    fn from_u32(v: u32) -> Option<Self> {
        match v {
            1 => Some(Self::Frame),
            2 => Some(Self::Resize),
            3 => Some(Self::Shutdown),
            4 => Some(Self::AttachSurface),
            5 => Some(Self::Presented),
            6 => Some(Self::Fatal),
            _ => None,
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct WireFrameHeader {
    uniform: ShaderUniform,
    draw_count: u32,
    atlas_update_count: u32,
    /// Non-zero when the atlas was recreated since the last frame (glyph
    /// cache ran out of room -- `recreate_texture_atlas_impl` in
    /// `renderstate.rs` always builds a brand new, empty atlas rather than
    /// growing the old one in place). The child must (re)allocate its
    /// mirrored atlas texture at `atlas_width`x`atlas_height` before
    /// applying this frame's `atlas_updates` -- which is safe to do exactly
    /// because the real atlas also starts genuinely empty at that moment.
    atlas_reset: u32,
    atlas_width: u32,
    atlas_height: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct WireDrawHeader {
    instance_count: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct WireAtlasUpdateHeader {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    /// Byte length of the pixel payload immediately following this header
    /// (always `width * height * 4` for the rgba32 layout `BitmapImage`
    /// uses, but carried explicitly so the reader never has to assume it).
    byte_len: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct WireResize {
    pub width: u32,
    pub height: u32,
}

/// Parent->child: attach (or re-attach, on respawn) to a composition-surface
/// generation. `surface_handle` is this child process's own value of a
/// handle the parent duplicated to it (`DuplicateHandle`) -- meaningless in
/// any other process, per Windows handle semantics.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct WireAttachSurface {
    pub surface_handle: i64,
    pub width: u32,
    pub height: u32,
}

/// Child->parent: acknowledges a successfully submitted+presented frame.
/// `seq` is a plain per-child monotonic counter (not tied to the parent's
/// own frame numbering) -- its only job is letting the parent's back-pressure
/// bookkeeping and hang detection tell "some frame landed" from "nothing has
/// landed since we started waiting".
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct WirePresented {
    pub seq: u64,
}

/// Child->parent, best-effort: sent just before the child intends to exit
/// abnormally, if it gets the chance (a real unhandled SEH fault never will
/// -- that's fine, EOF on the ack stream already means the same thing to the
/// parent, just without a reason code attached).
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct WireFatal {
    pub code: i32,
}

/// One atlas dirty-rect update: the same `(rect, pixel bytes)` pair
/// `WebGpuTexture::write` already receives, captured instead of uploaded.
pub struct AtlasUpdate {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// A `GpuFrame`'s worth of data that can cross a process boundary. See the
/// module doc comment for why this exists instead of reusing `GpuFrame`.
pub struct WireFrame {
    pub uniform: ShaderUniform,
    /// One entry per `GpuDraw`: the `QuadInstance`s that would otherwise
    /// have been uploaded to a `wgpu::Buffer` on the parent's device.
    pub draws: Vec<Vec<QuadInstance>>,
    /// Set when the atlas texture instance changed since the last frame
    /// (size the child should (re)allocate its mirror at). `None` means
    /// this frame's `atlas_updates` apply to the mirror the child already
    /// has from a previous frame.
    pub atlas_reset: Option<(u32, u32)>,
    pub atlas_updates: Vec<AtlasUpdate>,
}

fn write_message(w: &mut impl Write, kind: WireMessageKind, body: &[u8]) -> io::Result<()> {
    let len = body.len() as u32;
    w.write_all(&len.to_le_bytes())?;
    w.write_all(&(kind as u32).to_le_bytes())?;
    w.write_all(body)?;
    Ok(())
}

pub fn write_frame(w: &mut impl Write, frame: &WireFrame) -> io::Result<()> {
    let mut body = Vec::new();
    let (atlas_reset, atlas_width, atlas_height) = match frame.atlas_reset {
        Some((w, h)) => (1u32, w, h),
        None => (0u32, 0u32, 0u32),
    };
    let header = WireFrameHeader {
        uniform: frame.uniform,
        draw_count: frame.draws.len() as u32,
        atlas_update_count: frame.atlas_updates.len() as u32,
        atlas_reset,
        atlas_width,
        atlas_height,
    };
    body.extend_from_slice(bytemuck::bytes_of(&header));
    for draw in &frame.draws {
        let draw_header = WireDrawHeader {
            instance_count: draw.len() as u32,
        };
        body.extend_from_slice(bytemuck::bytes_of(&draw_header));
        body.extend_from_slice(bytemuck::cast_slice(draw));
    }
    for update in &frame.atlas_updates {
        let update_header = WireAtlasUpdateHeader {
            x: update.x,
            y: update.y,
            width: update.width,
            height: update.height,
            byte_len: update.pixels.len() as u32,
        };
        body.extend_from_slice(bytemuck::bytes_of(&update_header));
        body.extend_from_slice(&update.pixels);
    }
    write_message(w, WireMessageKind::Frame, &body)
}

pub fn write_resize(w: &mut impl Write, width: u32, height: u32) -> io::Result<()> {
    let payload = WireResize { width, height };
    write_message(w, WireMessageKind::Resize, bytemuck::bytes_of(&payload))
}

pub fn write_shutdown(w: &mut impl Write) -> io::Result<()> {
    write_message(w, WireMessageKind::Shutdown, &[])
}

pub fn write_attach_surface(
    w: &mut impl Write,
    surface_handle: i64,
    width: u32,
    height: u32,
) -> io::Result<()> {
    let payload = WireAttachSurface {
        surface_handle,
        width,
        height,
    };
    write_message(
        w,
        WireMessageKind::AttachSurface,
        bytemuck::bytes_of(&payload),
    )
}

pub fn write_presented(w: &mut impl Write, seq: u64) -> io::Result<()> {
    let payload = WirePresented { seq };
    write_message(w, WireMessageKind::Presented, bytemuck::bytes_of(&payload))
}

pub fn write_fatal(w: &mut impl Write, code: i32) -> io::Result<()> {
    let payload = WireFatal { code };
    write_message(w, WireMessageKind::Fatal, bytemuck::bytes_of(&payload))
}

/// A decoded message read from the wire. `Frame` deliberately keeps the raw
/// body around rather than eagerly reconstructing owned `Vec<QuadInstance>`s
/// -- the reader decides when it wants to pay that cost.
pub enum WireMessage {
    Frame(WireFrame),
    Resize(WireResize),
    Shutdown,
    AttachSurface(WireAttachSurface),
    Presented(WirePresented),
    Fatal(WireFatal),
}

fn read_exact_vec(r: &mut impl Read, len: usize) -> io::Result<Vec<u8>> {
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    Ok(buf)
}

/// Reads one message from the wire. Returns `Ok(None)` on a clean EOF
/// (the writer end closed without a `Shutdown` message, e.g. the child
/// process died) rather than an error, so callers can tell that apart from
/// a real I/O failure.
pub fn read_message(r: &mut impl Read) -> io::Result<Option<WireMessage>> {
    let mut len_buf = [0u8; 4];
    match r.read_exact(&mut len_buf) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len = u32::from_le_bytes(len_buf) as usize;

    let mut kind_buf = [0u8; 4];
    r.read_exact(&mut kind_buf)?;
    let kind = WireMessageKind::from_u32(u32::from_le_bytes(kind_buf))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "unknown wire message kind"))?;

    let body = read_exact_vec(r, len)?;

    match kind {
        WireMessageKind::Shutdown => Ok(Some(WireMessage::Shutdown)),
        WireMessageKind::Resize => {
            let payload: WireResize = *bytemuck::from_bytes(&body);
            Ok(Some(WireMessage::Resize(payload)))
        }
        WireMessageKind::Frame => Ok(Some(WireMessage::Frame(parse_frame(&body)?))),
        WireMessageKind::AttachSurface => {
            let payload: WireAttachSurface = *bytemuck::from_bytes(&body);
            Ok(Some(WireMessage::AttachSurface(payload)))
        }
        WireMessageKind::Presented => {
            let payload: WirePresented = *bytemuck::from_bytes(&body);
            Ok(Some(WireMessage::Presented(payload)))
        }
        WireMessageKind::Fatal => {
            let payload: WireFatal = *bytemuck::from_bytes(&body);
            Ok(Some(WireMessage::Fatal(payload)))
        }
    }
}

fn parse_frame(body: &[u8]) -> io::Result<WireFrame> {
    let header_size = std::mem::size_of::<WireFrameHeader>();
    if body.len() < header_size {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "frame body shorter than its own header",
        ));
    }
    let header: WireFrameHeader = *bytemuck::from_bytes(&body[..header_size]);
    let mut cursor = header_size;

    let mut draws = Vec::with_capacity(header.draw_count as usize);
    for _ in 0..header.draw_count {
        let draw_header_size = std::mem::size_of::<WireDrawHeader>();
        let draw_header: WireDrawHeader =
            *bytemuck::from_bytes(&body[cursor..cursor + draw_header_size]);
        cursor += draw_header_size;

        let byte_len = draw_header.instance_count as usize * std::mem::size_of::<QuadInstance>();
        let instances: &[QuadInstance] = bytemuck::cast_slice(&body[cursor..cursor + byte_len]);
        draws.push(instances.to_vec());
        cursor += byte_len;
    }

    let mut atlas_updates = Vec::with_capacity(header.atlas_update_count as usize);
    for _ in 0..header.atlas_update_count {
        let update_header_size = std::mem::size_of::<WireAtlasUpdateHeader>();
        let update_header: WireAtlasUpdateHeader =
            *bytemuck::from_bytes(&body[cursor..cursor + update_header_size]);
        cursor += update_header_size;

        let byte_len = update_header.byte_len as usize;
        let pixels = body[cursor..cursor + byte_len].to_vec();
        cursor += byte_len;

        atlas_updates.push(AtlasUpdate {
            x: update_header.x,
            y: update_header.y,
            width: update_header.width,
            height: update_header.height,
            pixels,
        });
    }

    Ok(WireFrame {
        uniform: header.uniform,
        draws,
        atlas_reset: if header.atlas_reset != 0 {
            Some((header.atlas_width, header.atlas_height))
        } else {
            None
        },
        atlas_updates,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_frame() -> WireFrame {
        WireFrame {
            uniform: ShaderUniform {
                foreground_text_hsb: [0.1, 0.2, 0.3],
                milliseconds: 42,
                projection: [[1.0; 4]; 4],
            },
            draws: vec![
                vec![QuadInstance::default(), QuadInstance::default()],
                vec![],
            ],
            atlas_reset: Some((512, 512)),
            atlas_updates: vec![AtlasUpdate {
                x: 1,
                y: 2,
                width: 3,
                height: 4,
                pixels: vec![9u8; 3 * 4 * 4],
            }],
        }
    }

    #[test]
    fn a_frame_round_trips_through_the_wire() {
        let frame = sample_frame();
        let mut buf = Vec::new();
        write_frame(&mut buf, &frame).unwrap();

        let mut cursor = std::io::Cursor::new(buf);
        let decoded = read_message(&mut cursor).unwrap().expect("one message");
        let WireMessage::Frame(decoded) = decoded else {
            panic!("expected a Frame message");
        };

        assert_eq!(decoded.uniform.milliseconds, 42);
        assert_eq!(decoded.draws.len(), 2);
        assert_eq!(decoded.draws[0].len(), 2);
        assert_eq!(decoded.draws[1].len(), 0);
        assert_eq!(decoded.atlas_reset, Some((512, 512)));
        assert_eq!(decoded.atlas_updates.len(), 1);
        assert_eq!(decoded.atlas_updates[0].pixels.len(), 3 * 4 * 4);
        assert_eq!(decoded.atlas_updates[0].pixels[0], 9);
    }

    #[test]
    fn no_atlas_reset_round_trips_as_none() {
        let mut frame = sample_frame();
        frame.atlas_reset = None;
        let mut buf = Vec::new();
        write_frame(&mut buf, &frame).unwrap();

        let mut cursor = std::io::Cursor::new(buf);
        let decoded = read_message(&mut cursor).unwrap().expect("one message");
        let WireMessage::Frame(decoded) = decoded else {
            panic!("expected a Frame message");
        };
        assert_eq!(decoded.atlas_reset, None);
    }

    #[test]
    fn resize_round_trips_through_the_wire() {
        let mut buf = Vec::new();
        write_resize(&mut buf, 640, 480).unwrap();

        let mut cursor = std::io::Cursor::new(buf);
        let decoded = read_message(&mut cursor).unwrap().expect("one message");
        let WireMessage::Resize(resize) = decoded else {
            panic!("expected a Resize message");
        };
        assert_eq!(resize.width, 640);
        assert_eq!(resize.height, 480);
    }

    #[test]
    fn shutdown_round_trips_through_the_wire() {
        let mut buf = Vec::new();
        write_shutdown(&mut buf).unwrap();

        let mut cursor = std::io::Cursor::new(buf);
        let decoded = read_message(&mut cursor).unwrap().expect("one message");
        assert!(matches!(decoded, WireMessage::Shutdown));
    }

    #[test]
    fn a_clean_eof_before_any_message_reads_as_none() {
        let mut cursor = std::io::Cursor::new(Vec::<u8>::new());
        let decoded = read_message(&mut cursor).unwrap();
        assert!(
            decoded.is_none(),
            "clean EOF should read as None, not an error"
        );
    }

    #[test]
    fn attach_surface_round_trips_through_the_wire() {
        let mut buf = Vec::new();
        write_attach_surface(&mut buf, -7, 800, 600).unwrap();

        let mut cursor = std::io::Cursor::new(buf);
        let decoded = read_message(&mut cursor).unwrap().expect("one message");
        let WireMessage::AttachSurface(attach) = decoded else {
            panic!("expected an AttachSurface message");
        };
        assert_eq!(attach.surface_handle, -7);
        assert_eq!(attach.width, 800);
        assert_eq!(attach.height, 600);
    }

    #[test]
    fn presented_round_trips_through_the_wire() {
        let mut buf = Vec::new();
        write_presented(&mut buf, 42).unwrap();

        let mut cursor = std::io::Cursor::new(buf);
        let decoded = read_message(&mut cursor).unwrap().expect("one message");
        let WireMessage::Presented(presented) = decoded else {
            panic!("expected a Presented message");
        };
        assert_eq!(presented.seq, 42);
    }

    #[test]
    fn fatal_round_trips_through_the_wire() {
        let mut buf = Vec::new();
        write_fatal(&mut buf, -1).unwrap();

        let mut cursor = std::io::Cursor::new(buf);
        let decoded = read_message(&mut cursor).unwrap().expect("one message");
        let WireMessage::Fatal(fatal) = decoded else {
            panic!("expected a Fatal message");
        };
        assert_eq!(fatal.code, -1);
    }

    #[test]
    fn multiple_messages_read_back_in_order() {
        let mut buf = Vec::new();
        write_resize(&mut buf, 100, 200).unwrap();
        write_shutdown(&mut buf).unwrap();

        let mut cursor = std::io::Cursor::new(buf);
        let first = read_message(&mut cursor).unwrap().expect("first message");
        assert!(matches!(first, WireMessage::Resize(_)));
        let second = read_message(&mut cursor).unwrap().expect("second message");
        assert!(matches!(second, WireMessage::Shutdown));
        let third = read_message(&mut cursor).unwrap();
        assert!(third.is_none());
    }
}
