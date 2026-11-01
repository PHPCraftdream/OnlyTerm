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
//! `AttachSurface` flow parent->child; `Presented`/`Failed`/`Fatal` flow the other
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

use crate::{QuadInstance, ShaderUniform};
use parking_lot::Mutex;
use std::convert::{TryFrom, TryInto};
use std::io::{self, Read, Write};

use std::sync::Arc;

/// A thread-safe pool of reusable `Vec<QuadInstance>` buffers, shared between
/// the GUI thread (which fills them in `build_wire_frame`) and the writer
/// thread (which returns them after `write_frame` has serialized their
/// contents onto the wire).
///
/// Without this pool, every frame clones each layer's `instances` Vec into a
/// fresh heap allocation (~300-435 KB per frame, sized to the current quad
/// count). Because the size drifts slightly frame-to-frame (terminal content
/// changes), the allocator sees a relentless stream of large, never-identical
/// allocations that it retains as unreusable arenas -- the root cause of the
/// OOM crash diagnosed from the PID 64440 dump (see
/// docs/investigations/2026-08-21-hostprocess-oom-crash-ktav-lang.md).
///
/// With the pool, the same buffers are reused across frames: the GUI thread
/// takes a buffer from the pool (or allocates fresh on the very first frame),
/// fills it via `extend_from_slice`, sends it inside a `WireFrame`, and the
/// writer thread pushes it back after serialization. Since `in_flight` limits
/// the pipeline to one frame, the pool stays at its steady-state size
/// (typically one buffer per draw) and the allocator never sees the unbounded
/// stream of large allocations that killed the process.
///
/// Mirrors the `TripleVertexBuffer::scratch_pool` idiom already established
/// in `renderstate.rs`.
pub type WireDrawPool = Arc<Mutex<Vec<Vec<QuadInstance>>>>;

/// Creates a new, empty draw-buffer pool.
pub fn new_draw_pool() -> WireDrawPool {
    Arc::new(Mutex::new(Vec::new()))
}

/// Takes a buffer from the pool (preserving its capacity from a previous
/// frame) or allocates a fresh one if the pool is empty.
pub fn pool_take(pool: &WireDrawPool) -> Vec<QuadInstance> {
    pool.lock().pop().unwrap_or_default()
}

/// Returns a buffer to the pool after the writer thread has finished
/// serializing it. The buffer is cleared (preserving capacity) before being
/// pushed back, so the next `pool_take` gets a ready-to-fill buffer.
pub fn pool_return(pool: &WireDrawPool, mut buf: Vec<QuadInstance>) {
    buf.clear();
    pool.lock().push(buf);
}

/// Returns all draw buffers from a `WireFrame` to the pool at once. Called
/// by the writer thread after `write_frame` has serialized the frame's
/// contents.
pub fn pool_return_draws(pool: &WireDrawPool, draws: &mut Vec<Vec<QuadInstance>>) {
    let mut guard = pool.lock();
    for mut buf in draws.drain(..) {
        buf.clear();
        guard.push(buf);
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum WireMessageKind {
    Frame,
    Resize,
    Shutdown,
    AttachSurface,
    Presented,
    Fatal,
    Failed,
    AdapterInfo,
    Unknown(u32),
}

impl WireMessageKind {
    fn to_u32(self) -> u32 {
        match self {
            Self::Frame => 1,
            Self::Resize => 2,
            Self::Shutdown => 3,
            Self::AttachSurface => 4,
            Self::Presented => 5,
            Self::Fatal => 6,
            Self::Failed => 7,
            Self::AdapterInfo => 8,
            Self::Unknown(value) => value,
        }
    }
    fn from_u32(v: u32) -> Self {
        match v {
            1 => Self::Frame,
            2 => Self::Resize,
            3 => Self::Shutdown,
            4 => Self::AttachSurface,
            5 => Self::Presented,
            6 => Self::Fatal,
            7 => Self::Failed,
            8 => Self::AdapterInfo,
            value => Self::Unknown(value),
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

pub const MAX_ADAPTER_INFO_STRING_BYTES: usize = 4096;
pub const MAX_UNKNOWN_MESSAGE_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WireAdapterInfo {
    pub name: String,
    pub device_type: String,
    pub backend: String,
    pub driver: String,
    pub driver_info: String,
    pub vendor: u32,
    pub device: u32,
}

fn put_string(body: &mut Vec<u8>, value: &str) -> io::Result<()> {
    if value.len() > MAX_ADAPTER_INFO_STRING_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "adapter string exceeds wire limit",
        ));
    }
    let len = u32::try_from(value.len()).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "adapter string length overflow",
        )
    })?;
    body.extend_from_slice(&len.to_le_bytes());
    body.extend_from_slice(value.as_bytes());
    Ok(())
}

pub fn write_adapter_info(w: &mut impl Write, info: &WireAdapterInfo) -> io::Result<()> {
    let mut body = Vec::new();
    for value in [
        &info.name,
        &info.device_type,
        &info.backend,
        &info.driver,
        &info.driver_info,
    ] {
        put_string(&mut body, value)?;
    }
    body.extend_from_slice(&info.vendor.to_le_bytes());
    body.extend_from_slice(&info.device.to_le_bytes());
    write_message(w, WireMessageKind::AdapterInfo, &body)
}

fn parse_adapter_info(body: &[u8]) -> io::Result<WireAdapterInfo> {
    let mut cursor = 0usize;
    let mut fields = Vec::with_capacity(5);
    for _ in 0..5 {
        let len_end = cursor.checked_add(4).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "adapter string length overflow")
        })?;
        let len_bytes = body.get(cursor..len_end).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "truncated adapter string length",
            )
        })?;
        let len = u32::from_le_bytes(len_bytes.try_into().map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidData, "invalid adapter string length")
        })?) as usize;
        cursor = len_end;
        if len > MAX_ADAPTER_INFO_STRING_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "adapter string exceeds wire limit",
            ));
        }
        let end = cursor.checked_add(len).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "adapter string length overflow")
        })?;
        let bytes = body.get(cursor..end).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "truncated adapter string")
        })?;
        fields.push(String::from_utf8(bytes.to_vec()).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidData, "adapter string is not UTF-8")
        })?);
        cursor = end;
    }
    if body.len().saturating_sub(cursor) != 8 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid adapter info tail",
        ));
    }
    let vendor_bytes = body
        .get(cursor..cursor + 4)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid vendor"))?;
    let device_bytes = body
        .get(cursor + 4..cursor + 8)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid device"))?;
    let vendor = u32::from_le_bytes(
        vendor_bytes
            .try_into()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid vendor"))?,
    );
    let device = u32::from_le_bytes(
        device_bytes
            .try_into()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid device"))?,
    );
    let mut fields = fields.into_iter();
    Ok(WireAdapterInfo {
        name: fields.next().unwrap(),
        device_type: fields.next().unwrap(),
        backend: fields.next().unwrap(),
        driver: fields.next().unwrap(),
        driver_info: fields.next().unwrap(),
        vendor,
        device,
    })
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

/// Child->parent: acknowledges a frame that was received but NOT presented,
/// where the failure is recoverable in place (`submit_frame` returned
/// `SurfaceError::Lost`/`Outdated`; the child reconfigured its surface and
/// dropped exactly that one frame -- see `gpu_tab_host`'s submit handling).
/// Tells the parent to release its back-pressure slot for this frame and
/// force the next repaint past its frame-signature skip, so the dropped
/// frame's content is rebuilt and resent instead of staying frozen
/// off-screen. `seq` follows the same per-child counter as `WirePresented`.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct WireFailed {
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
    /// Shared with the parent's replay log when a full atlas resync is sent.
    /// A fresh child still gets an owned allocation while decoding the wire,
    /// but the parent no longer clones every retained atlas write just to
    /// build a frame.
    pub pixels: Arc<[u8]>,
}

/// A borrowed view of one atlas update, pointing straight into the caller's
/// message-body buffer instead of owning a copy of the pixels.
pub struct AtlasUpdateRef<'a> {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub pixels: &'a [u8],
}

/// A borrowed view of a frame, parsed in place from a caller-owned body
/// buffer (see [`read_message_into`]).
///
/// The receiving side of the wire never needs to own this data: every draw
/// goes straight into a `wgpu` vertex buffer and every atlas update straight
/// into a texture write, both of which copy into GPU-owned storage. Handing
/// it owned `Vec`s instead meant allocating (and immediately freeing) the
/// whole frame -- hundreds of KB, at a size that drifts frame to frame --
/// on every single frame, which is exactly the allocation pattern that
/// exhausted memory on the sending side (see
/// docs/investigations/2026-08-21-hostprocess-oom-crash-ktav-lang.md). The
/// only allocations left here are the two small `Vec`s of references.
pub struct WireFrameRef<'a> {
    pub uniform: ShaderUniform,
    pub draws: Vec<&'a [QuadInstance]>,
    pub atlas_reset: Option<(u32, u32)>,
    pub atlas_updates: Vec<AtlasUpdateRef<'a>>,
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
    let len = u32::try_from(body.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "message length overflow"))?;
    w.write_all(&len.to_le_bytes())?;
    w.write_all(&kind.to_u32().to_le_bytes())?;
    w.write_all(body)?;
    Ok(())
}

pub fn write_frame(w: &mut impl Write, frame: &WireFrame) -> io::Result<()> {
    let mut body = Vec::new();
    write_frame_with_buffer(w, frame, &mut body)
}

/// Serializes a frame using caller-owned storage for the message body.
///
/// The host-process writer calls this with one buffer that lives for the
/// writer thread's lifetime.  Keeping the capacity between frames is
/// essential for the custom allocator used by the GUI: a fresh 300--435 KB
/// body allocation per frame otherwise becomes a steady stream of retained
/// arenas even though the wire body is dropped after each write.
pub fn write_frame_with_buffer(
    w: &mut impl Write,
    frame: &WireFrame,
    body: &mut Vec<u8>,
) -> io::Result<()> {
    body.clear();
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
        body.extend_from_slice(update.pixels.as_ref());
    }
    write_message(w, WireMessageKind::Frame, body)
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

pub fn write_failed(w: &mut impl Write, seq: u64) -> io::Result<()> {
    let payload = WireFailed { seq };
    write_message(w, WireMessageKind::Failed, bytemuck::bytes_of(&payload))
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
    Failed(WireFailed),
    Fatal(WireFatal),
    AdapterInfo(WireAdapterInfo),
    Unknown { kind: u32, body: Vec<u8> },
}

/// Borrowed counterpart of [`WireMessage`], produced by
/// [`read_message_into`]. Only `Frame` actually borrows; the rest are small
/// `Pod` payloads carried by value, exactly as in the owned enum.
pub enum WireMessageRef<'a> {
    Frame(WireFrameRef<'a>),
    Resize(WireResize),
    Shutdown,
    AttachSurface(WireAttachSurface),
    Presented(WirePresented),
    Failed(WireFailed),
    Fatal(WireFatal),
    AdapterInfo(WireAdapterInfo),
    Unknown { kind: u32, body: &'a [u8] },
}

fn read_pod<T: bytemuck::Pod>(body: &[u8]) -> io::Result<T> {
    if body.len() != std::mem::size_of::<T>() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid fixed-size wire payload",
        ));
    }
    Ok(*bytemuck::from_bytes(body))
}
fn read_exact_vec(r: &mut impl Read, len: usize) -> io::Result<Vec<u8>> {
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    Ok(buf)
}

/// Reads the fixed-size length+kind prefix. `Ok(None)` on a clean EOF, with
/// the same meaning as [`read_message`]'s.
fn read_header(r: &mut impl Read) -> io::Result<Option<(usize, WireMessageKind)>> {
    let mut len_buf = [0u8; 4];
    match r.read_exact(&mut len_buf) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len = u32::from_le_bytes(len_buf) as usize;
    let mut kind_buf = [0u8; 4];
    r.read_exact(&mut kind_buf)?;
    let kind = WireMessageKind::from_u32(u32::from_le_bytes(kind_buf));
    if matches!(kind, WireMessageKind::Unknown(_)) && len > MAX_UNKNOWN_MESSAGE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unknown message exceeds wire limit",
        ));
    }
    if matches!(kind, WireMessageKind::AdapterInfo)
        && len > 5 * (4 + MAX_ADAPTER_INFO_STRING_BYTES) + 8
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "adapter info exceeds wire limit",
        ));
    }
    Ok(Some((len, kind)))
}

/// Reads one message using caller-owned storage for the body, returning a
/// view that borrows it.
///
/// This is the read-side counterpart of [`write_frame_with_buffer`], and the
/// form the `gpu-tab-host` child's message loop uses: one buffer serves every
/// frame for the process's lifetime, and no part of a frame is ever copied
/// out of it -- draws go straight into a `wgpu` vertex buffer, atlas pixels
/// straight into a texture write. [`read_message`] allocates a fresh body
/// (and then a second set of owned copies inside it) per call, which for a
/// stream of multi-hundred-KB frames is the very allocation pattern that
/// exhausted memory on the sending side.
pub fn read_message_into<'a>(
    r: &mut impl Read,
    body: &'a mut Vec<u8>,
) -> io::Result<Option<WireMessageRef<'a>>> {
    let Some((len, kind)) = read_header(r)? else {
        return Ok(None);
    };
    body.clear();
    body.resize(len, 0);
    r.read_exact(body)?;

    Ok(Some(match kind {
        WireMessageKind::Shutdown => {
            if !body.is_empty() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid Shutdown payload",
                ));
            }
            WireMessageRef::Shutdown
        }
        WireMessageKind::Resize => WireMessageRef::Resize(read_pod(body)?),
        WireMessageKind::Frame => WireMessageRef::Frame(parse_frame_ref(body)?),
        WireMessageKind::AttachSurface => WireMessageRef::AttachSurface(read_pod(body)?),
        WireMessageKind::Presented => WireMessageRef::Presented(read_pod(body)?),
        WireMessageKind::Fatal => WireMessageRef::Fatal(read_pod(body)?),
        WireMessageKind::Failed => WireMessageRef::Failed(read_pod(body)?),
        WireMessageKind::AdapterInfo => WireMessageRef::AdapterInfo(parse_adapter_info(body)?),
        WireMessageKind::Unknown(kind) => WireMessageRef::Unknown {
            kind,
            body: &body[..],
        },
    }))
}

/// Reads one message from the wire. Returns `Ok(None)` on a clean EOF
/// (the writer end closed without a `Shutdown` message, e.g. the child
/// process died) rather than an error, so callers can tell that apart from
/// a real I/O failure.
pub fn read_message(r: &mut impl Read) -> io::Result<Option<WireMessage>> {
    let Some((len, kind)) = read_header(r)? else {
        return Ok(None);
    };

    let body = read_exact_vec(r, len)?;

    Ok(Some(match kind {
        WireMessageKind::Shutdown => {
            if !body.is_empty() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid Shutdown payload",
                ));
            }
            WireMessage::Shutdown
        }
        WireMessageKind::Resize => WireMessage::Resize(read_pod(&body)?),
        WireMessageKind::Frame => WireMessage::Frame(parse_frame(&body)?),
        WireMessageKind::AttachSurface => WireMessage::AttachSurface(read_pod(&body)?),
        WireMessageKind::Presented => WireMessage::Presented(read_pod(&body)?),
        WireMessageKind::Fatal => WireMessage::Fatal(read_pod(&body)?),
        WireMessageKind::Failed => WireMessage::Failed(read_pod(&body)?),
        WireMessageKind::AdapterInfo => WireMessage::AdapterInfo(parse_adapter_info(&body)?),
        WireMessageKind::Unknown(kind) => WireMessage::Unknown { kind, body },
    }))
}

/// Parses a frame body into borrowed views over that same buffer, without
/// copying any of the bulk payload. See [`WireFrameRef`] for why the reading
/// side wants this rather than owned `Vec`s.
fn parse_frame_ref(body: &[u8]) -> io::Result<WireFrameRef<'_>> {
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
        draws.push(instances);
        cursor += byte_len;
    }

    let mut atlas_updates = Vec::with_capacity(header.atlas_update_count as usize);
    for _ in 0..header.atlas_update_count {
        let update_header_size = std::mem::size_of::<WireAtlasUpdateHeader>();
        let update_header: WireAtlasUpdateHeader =
            *bytemuck::from_bytes(&body[cursor..cursor + update_header_size]);
        cursor += update_header_size;

        let byte_len = update_header.byte_len as usize;
        let pixels = &body[cursor..cursor + byte_len];
        cursor += byte_len;

        atlas_updates.push(AtlasUpdateRef {
            x: update_header.x,
            y: update_header.y,
            width: update_header.width,
            height: update_header.height,
            pixels,
        });
    }

    Ok(WireFrameRef {
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

/// Owned form of [`parse_frame_ref`], for callers that need the frame to
/// outlive the buffer it was read into (the round-trip tests, and any future
/// consumer that isn't uploading straight to the GPU).
fn parse_frame(body: &[u8]) -> io::Result<WireFrame> {
    let borrowed = parse_frame_ref(body)?;
    Ok(WireFrame {
        uniform: borrowed.uniform,
        draws: borrowed.draws.iter().map(|d| d.to_vec()).collect(),
        atlas_reset: borrowed.atlas_reset,
        atlas_updates: borrowed
            .atlas_updates
            .iter()
            .map(|u| AtlasUpdate {
                x: u.x,
                y: u.y,
                width: u.width,
                height: u.height,
                pixels: Arc::from(u.pixels),
            })
            .collect(),
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
                pixels: Arc::from(vec![9u8; 3 * 4 * 4]),
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
    fn frame_body_buffer_is_reused_across_serializations() {
        struct Sink;

        impl Write for Sink {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                Ok(bytes.len())
            }

            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }

        let frame = sample_frame();
        let mut body = Vec::new();
        let mut sink = Sink;

        write_frame_with_buffer(&mut sink, &frame, &mut body).unwrap();
        let first_ptr = body.as_ptr();
        let first_capacity = body.capacity();
        assert!(!body.is_empty());

        write_frame_with_buffer(&mut sink, &frame, &mut body).unwrap();

        assert_eq!(
            body.as_ptr(),
            first_ptr,
            "serializing another frame must reuse the existing body allocation"
        );
        assert_eq!(
            body.capacity(),
            first_capacity,
            "serializing another frame must not shrink the retained body capacity"
        );
    }

    /// The read-side counterpart of
    /// `frame_body_buffer_is_reused_across_serializations`: the child's
    /// message loop reads every frame into one buffer for the life of the
    /// process, and consumes it as borrowed views rather than copying the
    /// draws and atlas pixels back out. Both halves matter -- reading into a
    /// fresh allocation per frame is the same never-reused large-allocation
    /// pattern that exhausted memory on the sending side.
    #[test]
    fn read_body_buffer_is_reused_and_frames_borrow_from_it() {
        let mut encoded = Vec::new();
        let frame = sample_frame();
        write_frame(&mut encoded, &frame).unwrap();
        write_frame(&mut encoded, &frame).unwrap();

        let mut reader = encoded.as_slice();
        let mut body = Vec::new();

        // Addresses are copied out as plain integers so the borrow of `body`
        // ends with the message, leaving `body` inspectable afterwards.
        let (draw_ptr, pixels_ptr) = {
            let message = read_message_into(&mut reader, &mut body).unwrap().unwrap();
            let WireMessageRef::Frame(decoded) = message else {
                panic!("expected a Frame");
            };
            assert_eq!(decoded.draws[0].len(), 2);
            (
                decoded.draws[0].as_ptr() as usize,
                decoded.atlas_updates[0].pixels.as_ptr() as usize,
            )
        };

        let first_ptr = body.as_ptr();
        let first_capacity = body.capacity();
        let body_range = first_ptr as usize..first_ptr as usize + body.len();
        assert!(
            body_range.contains(&draw_ptr),
            "draw instances must borrow from the read buffer, not a copy"
        );
        assert!(
            body_range.contains(&pixels_ptr),
            "atlas pixels must borrow from the read buffer, not a copy"
        );

        {
            let message = read_message_into(&mut reader, &mut body).unwrap().unwrap();
            let WireMessageRef::Frame(decoded) = message else {
                panic!("expected a Frame");
            };
            assert_eq!(decoded.draws[0].len(), 2);
        }

        assert_eq!(
            body.as_ptr(),
            first_ptr,
            "reading another frame must reuse the existing body allocation"
        );
        assert_eq!(
            body.capacity(),
            first_capacity,
            "reading another frame must not shrink the retained body capacity"
        );
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
    fn wire_message_kind_tags_match_the_original_and_adapter_extension() {
        let expected = [
            (WireMessageKind::Frame, 1u32),
            (WireMessageKind::Resize, 2),
            (WireMessageKind::Shutdown, 3),
            (WireMessageKind::AttachSurface, 4),
            (WireMessageKind::Presented, 5),
            (WireMessageKind::Fatal, 6),
            (WireMessageKind::Failed, 7),
            (WireMessageKind::AdapterInfo, 8),
        ];
        for (kind, tag) in expected {
            let mut bytes = Vec::new();
            write_message(&mut bytes, kind, &[]).unwrap();
            assert_eq!(&bytes[..4], &[0, 0, 0, 0]);
            assert_eq!(&bytes[4..8], &tag.to_le_bytes());
        }
    }

    #[test]
    fn adapter_payload_bounds_are_enforced_by_both_readers() {
        let mut malformed = Vec::new();
        write_message(
            &mut malformed,
            WireMessageKind::AdapterInfo,
            &[1, 0, 0, 0, 0xff],
        )
        .unwrap();
        let err = match read_message(&mut malformed.as_slice()) {
            Err(err) => err,
            Ok(_) => panic!("owned reader accepted invalid UTF-8"),
        };
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        let mut malformed = Vec::new();
        write_message(
            &mut malformed,
            WireMessageKind::AdapterInfo,
            &[1, 0, 0, 0, 0xff],
        )
        .unwrap();
        let mut body = Vec::new();
        let err = match read_message_into(&mut malformed.as_slice(), &mut body) {
            Err(err) => err,
            Ok(_) => panic!("borrowed reader accepted invalid UTF-8"),
        };
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);

        let mut body = Vec::new();

        let field_len = (MAX_ADAPTER_INFO_STRING_BYTES + 1) as u32;
        let mut oversized = Vec::new();
        oversized.extend_from_slice(&field_len.to_le_bytes());
        oversized.extend(std::iter::repeat(b'x').take(MAX_ADAPTER_INFO_STRING_BYTES + 1));
        for _ in 0..4 {
            oversized.extend_from_slice(&0u32.to_le_bytes());
        }
        oversized.extend_from_slice(&[0; 8]);
        let mut encoded = Vec::new();
        write_message(&mut encoded, WireMessageKind::AdapterInfo, &oversized).unwrap();
        let err = match read_message(&mut encoded.as_slice()) {
            Err(err) => err,
            Ok(_) => panic!("owned reader accepted oversized adapter string"),
        };
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        let err = match read_message_into(&mut encoded.as_slice(), &mut body) {
            Err(err) => err,
            Ok(_) => panic!("borrowed reader accepted oversized adapter string"),
        };
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);

        let mut truncated = Vec::new();
        write_message(
            &mut truncated,
            WireMessageKind::AdapterInfo,
            &[2, 0, 0, 0, b'x'],
        )
        .unwrap();
        let err = match read_message(&mut truncated.as_slice()) {
            Err(err) => err,
            Ok(_) => panic!("owned reader accepted truncated adapter string"),
        };
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        let err = match read_message_into(&mut truncated.as_slice(), &mut body) {
            Err(err) => err,
            Ok(_) => panic!("borrowed reader accepted truncated adapter string"),
        };
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn unknown_message_bounds_are_enforced_by_both_readers() {
        let mut header = Vec::new();
        header.extend_from_slice(&((MAX_UNKNOWN_MESSAGE_BYTES + 1) as u32).to_le_bytes());
        header.extend_from_slice(&99u32.to_le_bytes());
        let err = match read_message(&mut header.as_slice()) {
            Err(err) => err,
            Ok(_) => panic!("owned reader accepted oversized unknown kind"),
        };
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        let err = match read_message_into(&mut header.as_slice(), &mut Vec::new()) {
            Err(err) => err,
            Ok(_) => panic!("borrowed reader accepted oversized unknown kind"),
        };
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);

        let oversized = vec![0u8; MAX_UNKNOWN_MESSAGE_BYTES + 1];
        let mut encoded = Vec::new();
        write_message(&mut encoded, WireMessageKind::Unknown(99), &oversized).unwrap();
        let err = match read_message(&mut encoded.as_slice()) {
            Err(err) => err,
            Ok(_) => panic!("owned reader accepted oversized unknown payload"),
        };
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        let err = match read_message_into(&mut encoded.as_slice(), &mut Vec::new()) {
            Err(err) => err,
            Ok(_) => panic!("borrowed reader accepted oversized unknown payload"),
        };
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);

        let mut bounded = Vec::new();
        write_message(
            &mut bounded,
            WireMessageKind::Unknown(99),
            &vec![0x5a; MAX_UNKNOWN_MESSAGE_BYTES],
        )
        .unwrap();
        let mut owned_reader = bounded.as_slice();
        match read_message(&mut owned_reader).unwrap().unwrap() {
            WireMessage::Unknown { kind, body } => {
                assert_eq!(kind, 99);
                assert_eq!(body.len(), MAX_UNKNOWN_MESSAGE_BYTES);
                assert!(body.iter().all(|byte| *byte == 0x5a));
            }
            _ => panic!("expected bounded Unknown"),
        }
        let mut borrowed_reader = bounded.as_slice();
        match read_message_into(&mut borrowed_reader, &mut Vec::new())
            .unwrap()
            .unwrap()
        {
            WireMessageRef::Unknown { kind, body } => {
                assert_eq!(kind, 99);
                assert_eq!(body.len(), MAX_UNKNOWN_MESSAGE_BYTES);
                assert!(body.iter().all(|byte| *byte == 0x5a));
            }
            _ => panic!("expected bounded Unknown"),
        }
    }

    #[test]
    fn adapter_and_unknown_interleaving_preserves_frames_on_both_readers() {
        let mut encoded = Vec::new();
        write_presented(&mut encoded, 41).unwrap();
        write_adapter_info(
            &mut encoded,
            &WireAdapterInfo {
                name: "GPU 東京".into(),
                vendor: 7,
                ..Default::default()
            },
        )
        .unwrap();
        write_message(
            &mut encoded,
            WireMessageKind::Unknown(99),
            b"future payload",
        )
        .unwrap();
        write_resize(&mut encoded, 800, 600).unwrap();

        let mut reader = encoded.as_slice();
        match read_message(&mut reader).unwrap().unwrap() {
            WireMessage::Presented(value) => assert_eq!(value.seq, 41),
            _ => panic!("expected Presented"),
        }
        match read_message(&mut reader).unwrap().unwrap() {
            WireMessage::AdapterInfo(value) => {
                assert_eq!(value.name, "GPU 東京");
                assert_eq!(value.vendor, 7);
            }
            _ => panic!("expected AdapterInfo"),
        }
        assert!(
            matches!(read_message(&mut reader).unwrap().unwrap(), WireMessage::Unknown { kind: 99, body } if body == b"future payload")
        );
        match read_message(&mut reader).unwrap().unwrap() {
            WireMessage::Resize(value) => {
                assert_eq!(value.width, 800);
                assert_eq!(value.height, 600);
            }
            _ => panic!("expected Resize"),
        }
        assert!(reader.is_empty());

        let mut reader = encoded.as_slice();
        let mut body = Vec::new();
        match read_message_into(&mut reader, &mut body).unwrap().unwrap() {
            WireMessageRef::Presented(value) => assert_eq!(value.seq, 41),
            _ => panic!("expected Presented"),
        }
        match read_message_into(&mut reader, &mut body).unwrap().unwrap() {
            WireMessageRef::AdapterInfo(value) => {
                assert_eq!(value.name, "GPU 東京");
                assert_eq!(value.vendor, 7);
            }
            _ => panic!("expected AdapterInfo"),
        }
        match read_message_into(&mut reader, &mut body).unwrap().unwrap() {
            WireMessageRef::Unknown { kind, body } => {
                assert_eq!(kind, 99);
                assert_eq!(body, b"future payload");
            }
            _ => panic!("expected Unknown"),
        }
        match read_message_into(&mut reader, &mut body).unwrap().unwrap() {
            WireMessageRef::Resize(value) => {
                assert_eq!(value.width, 800);
                assert_eq!(value.height, 600);
            }
            _ => panic!("expected Resize"),
        }
        assert!(reader.is_empty());
    }

    #[test]
    fn fragmented_reads_decode_complete_messages_and_following_frames() {
        struct Fragmented<'a> {
            bytes: &'a [u8],
            chunk: usize,
        }
        impl Read for Fragmented<'_> {
            fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
                let count = out.len().min(self.chunk).min(self.bytes.len());
                out[..count].copy_from_slice(&self.bytes[..count]);
                self.bytes = &self.bytes[count..];
                Ok(count)
            }
        }

        let mut encoded = Vec::new();
        write_adapter_info(
            &mut encoded,
            &WireAdapterInfo {
                name: "fragmented 🧪".into(),
                vendor: 17,
                ..Default::default()
            },
        )
        .unwrap();
        write_presented(&mut encoded, 1234).unwrap();

        let mut reader = Fragmented {
            bytes: &encoded,
            chunk: 2,
        };
        match read_message(&mut reader).unwrap().unwrap() {
            WireMessage::AdapterInfo(info) => {
                assert_eq!(info.name, "fragmented 🧪");
                assert_eq!(info.vendor, 17);
            }
            _ => panic!("expected AdapterInfo"),
        }
        match read_message(&mut reader).unwrap().unwrap() {
            WireMessage::Presented(presented) => assert_eq!(presented.seq, 1234),
            _ => panic!("expected Presented"),
        }
        assert!(read_message(&mut reader).unwrap().is_none());
    }

    #[test]
    fn adapter_info_writer_rejects_oversized_string() {
        let info = WireAdapterInfo {
            name: "x".repeat(MAX_ADAPTER_INFO_STRING_BYTES + 1),
            ..Default::default()
        };
        let err = match write_adapter_info(&mut Vec::new(), &info) {
            Err(err) => err,
            Ok(()) => panic!("oversized adapter string accepted by writer"),
        };
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn adapter_info_round_trips_empty_unicode_and_bounded_strings() {
        for info in [
            WireAdapterInfo::default(),
            WireAdapterInfo {
                name: "GPU 🖥️ 東京".to_string(),
                device_type: "DiscreteGpu".into(),
                backend: "Vulkan".into(),
                driver: "драйвер".into(),
                driver_info: "版本".into(),
                vendor: 0x1234,
                device: 0xabcd,
            },
            WireAdapterInfo {
                name: "x".repeat(MAX_ADAPTER_INFO_STRING_BYTES),
                ..Default::default()
            },
        ] {
            let mut bytes = Vec::new();
            write_adapter_info(&mut bytes, &info).unwrap();
            let decoded = read_message(&mut bytes.as_slice()).unwrap().unwrap();
            let WireMessage::AdapterInfo(decoded) = decoded else {
                panic!("expected adapter info");
            };
            assert_eq!(decoded, info);
        }
    }

    #[test]
    fn adapter_info_rejects_malformed_and_truncated_payloads() {
        let mut too_long = Vec::new();
        too_long.extend_from_slice(&((MAX_ADAPTER_INFO_STRING_BYTES + 1) as u32).to_le_bytes());
        let mut encoded = Vec::new();
        write_message(&mut encoded, WireMessageKind::AdapterInfo, &too_long).unwrap();
        let err = match read_message(&mut encoded.as_slice()) {
            Err(err) => err,
            Ok(_) => panic!("oversized adapter string accepted"),
        };
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        let mut encoded = Vec::new();
        write_message(
            &mut encoded,
            WireMessageKind::AdapterInfo,
            &[2, 0, 0, 0, b'x'],
        )
        .unwrap();
        assert_eq!(
            read_message(&mut encoded.as_slice()).err().unwrap().kind(),
            io::ErrorKind::InvalidData
        );
        let mut encoded = Vec::new();
        write_message(
            &mut encoded,
            WireMessageKind::AdapterInfo,
            &[1, 0, 0, 0, 0xff],
        )
        .unwrap();
        assert_eq!(
            read_message(&mut encoded.as_slice()).err().unwrap().kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn adapter_info_and_unknown_kind_preserve_interleaved_frames() {
        let mut bytes = Vec::new();
        write_presented(&mut bytes, 8).unwrap();
        write_adapter_info(
            &mut bytes,
            &WireAdapterInfo {
                name: "adapter".into(),
                ..Default::default()
            },
        )
        .unwrap();
        write_message(&mut bytes, WireMessageKind::Unknown(99), b"future").unwrap();
        write_failed(&mut bytes, 9).unwrap();
        let mut cursor = bytes.as_slice();
        assert!(matches!(
            read_message(&mut cursor).unwrap(),
            Some(WireMessage::Presented(_))
        ));
        assert!(matches!(
            read_message(&mut cursor).unwrap(),
            Some(WireMessage::AdapterInfo(_))
        ));
        assert!(
            matches!(read_message(&mut cursor).unwrap(), Some(WireMessage::Unknown { kind: 99, body }) if body == b"future")
        );
        assert!(matches!(
            read_message(&mut cursor).unwrap(),
            Some(WireMessage::Failed(_))
        ));
        let mut cursor = bytes.as_slice();
        let mut body = Vec::new();
        for _ in 0..4 {
            read_message_into(&mut cursor, &mut body).unwrap().unwrap();
        }
        assert!(cursor.is_empty());
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
    fn failed_round_trips() {
        let mut buf = Vec::new();
        write_failed(&mut buf, 7).unwrap();

        let mut cursor = std::io::Cursor::new(buf);
        let decoded = read_message(&mut cursor)
            .unwrap()
            .expect("a message was written");
        let WireMessage::Failed(failed) = decoded else {
            panic!("expected a Failed message");
        };
        assert_eq!(failed.seq, 7);
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

    /// Regression test for the wire-frame draw-buffer pool: simulates the
    /// exact take-fill-serialize-return cycle that `build_wire_frame` (GUI
    /// thread) and `spawn_writer_thread` (writer thread) perform, and
    /// asserts that the pool reuses buffers across frames rather than
    /// allocating fresh ones every time.
    ///
    /// Without the pool fix (i.e. the old `vb.instances.borrow().clone()`
    /// path), each frame allocated a fresh ~300-435 KB `Vec<QuadInstance>`
    /// that was never returned to any pool and whose slightly-varying size
    /// prevented the allocator from reusing its arena -- the root cause of
    /// the OOM crash diagnosed from the PID 64440 dump (see
    /// docs/investigations/2026-08-21-hostprocess-oom-crash-ktav-lang.md).
    ///
    /// Scope, stated honestly: this pins the take/serialize/return contract
    /// of the pool itself -- that a returned buffer really is handed back
    /// out rather than reallocated. It does NOT observe `build_wire_frame`,
    /// which has no test harness (it needs a live `TermWindow`), so a
    /// regression that reverted *that* call site to `.clone()` while leaving
    /// these helpers intact would still pass. That wiring is verified by
    /// reading, the same tradeoff `test_scratch_pool_reuses_capacity` in
    /// `renderstate.rs` makes for the identical idiom.
    ///
    /// Modeled on `test_scratch_pool_reuses_capacity` in `renderstate.rs`.
    #[test]
    fn wire_draw_pool_reuses_buffers_across_frames() {
        let pool = new_draw_pool();

        // --- Frame 1: pool is empty, so pool_take allocates fresh ---
        let mut buf1 = pool_take(&pool);
        // Simulate filling the buffer with quad data (like build_wire_frame
        // does with extend_from_slice from the layer's instances).
        for i in 0..1000 {
            buf1.push(QuadInstance {
                position: [i as f32, 0.0, 0.0, 0.0],
                ..Default::default()
            });
        }
        let ptr1 = buf1.as_ptr();
        let cap1 = buf1.capacity();

        // Build a WireFrame carrying this buffer (simulates what
        // build_wire_frame returns).
        let mut frame = WireFrame {
            uniform: ShaderUniform::default(),
            draws: vec![buf1],
            atlas_reset: None,
            atlas_updates: vec![],
        };

        // Simulate the writer thread: serialize, then return buffers.
        let mut wire_bytes = Vec::new();
        write_frame(&mut wire_bytes, &frame).unwrap();
        pool_return_draws(&pool, &mut frame.draws);

        // The frame's draws should be drained.
        assert!(
            frame.draws.is_empty(),
            "pool_return_draws must drain the frame's draws Vec"
        );
        // The pool should now hold the buffer.
        assert_eq!(
            pool.lock().len(),
            1,
            "pool should hold exactly one returned buffer"
        );

        // --- Frame 2: pool_take should return the SAME buffer ---
        let mut buf2 = pool_take(&pool);
        let ptr2 = buf2.as_ptr();
        let cap2 = buf2.capacity();

        assert_eq!(
            ptr1, ptr2,
            "second pool_take must reuse the same heap allocation (pointer identity), \
             not allocate fresh -- this is the property that prevents OOM"
        );
        assert_eq!(
            cap1, cap2,
            "reused buffer must preserve its capacity from the previous frame"
        );
        assert_eq!(
            buf2.len(),
            0,
            "reused buffer must be cleared (length 0) so the caller can fill it fresh"
        );

        // Fill it with a slightly different count (simulates the terminal
        // content changing between frames -- the exact trigger for the OOM,
        // since the old clone path produced slightly-different-sized
        // allocations that the allocator could never reuse).
        for i in 0..1050 {
            buf2.push(QuadInstance {
                position: [i as f32, 1.0, 0.0, 0.0],
                ..Default::default()
            });
        }

        // Verify the serialized content is correct (same bytes as a fresh
        // allocation would produce -- the pool must not corrupt data).
        let mut frame2 = WireFrame {
            uniform: ShaderUniform::default(),
            draws: vec![buf2],
            atlas_reset: None,
            atlas_updates: vec![],
        };
        let mut wire_bytes2 = Vec::new();
        write_frame(&mut wire_bytes2, &frame2).unwrap();
        pool_return_draws(&pool, &mut frame2.draws);

        // Decode and verify frame 2's content survived the pool round-trip.
        let mut cursor = std::io::Cursor::new(wire_bytes2);
        let decoded = read_message(&mut cursor).unwrap().expect("a frame");
        let WireMessage::Frame(decoded) = decoded else {
            panic!("expected a Frame message");
        };
        assert_eq!(decoded.draws.len(), 1);
        assert_eq!(decoded.draws[0].len(), 1050);
        assert_eq!(
            decoded.draws[0][0].position,
            [0.0, 1.0, 0.0, 0.0],
            "first quad of frame 2 should carry the frame-2 data"
        );
    }

    /// Verifies that `pool_return` preserves capacity but clears content.
    #[test]
    fn pool_return_clears_but_preserves_capacity() {
        let pool = new_draw_pool();
        let mut buf = Vec::with_capacity(500);
        for i in 0..100 {
            buf.push(QuadInstance {
                position: [i as f32; 4],
                ..Default::default()
            });
        }
        let cap = buf.capacity();
        pool_return(&pool, buf);

        let recycled = pool_take(&pool);
        assert_eq!(recycled.len(), 0, "returned buffer must be empty");
        assert_eq!(
            recycled.capacity(),
            cap,
            "returned buffer must preserve its capacity"
        );
    }
}
