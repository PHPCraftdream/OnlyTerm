use config::GpuInfo;
pub use onlyterm_gpu_protocol::{wire, ShaderUniform};
use std::sync::Arc;
use window::bitmaps::Texture2d;
#[cfg(windows)]
use window::raw_window_handle::Win32WindowHandle;
use window::raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
    RawWindowHandle, WindowHandle,
};
use window::{BitmapImage, Rect, Window};

/// A single draw call's worth of GPU state, already fully detached from
/// `TermWindow`/`RenderState`: the vertex buffer returned by
/// `WebGpuVertexBuffer::recreate()` is the buffer that was just filled with
/// this frame's vertex data and is no longer referenced by anything else
/// once `recreate()` returns, so it's safe to move around freely (e.g. to
/// hand off to a dedicated render thread in a later task).
pub struct GpuDraw {
    pub vertex_buffer: wgpu::Buffer,
    /// Number of instances to draw (0 for vertex-mode, >0 for instance-mode)
    pub instance_count: u32,
}

/// Everything needed to submit one frame's worth of draw calls, computed
/// without needing further access to `TermWindow`/`RenderState`.
pub struct GpuFrame {
    pub draws: Vec<GpuDraw>,
    pub atlas: wgpu::Texture,
    pub uniform: ShaderUniform,
}

/// WebGpuState is now a composition of a shared process-wide GPU context
/// and per-window surface state.
///
/// The shared context (ProcessGpuContext) is created once per process and
/// reused across all windows. It contains the expensive resources:
/// Instance, Adapter, Device, Queue, shader, layouts, samplers, and a pipeline
/// cache keyed by surface format.
///
/// The per-window surface (WindowGpuSurface) contains the surface itself,
/// its configuration, dimensions, and the HWND it targets.
pub struct WebGpuState {
    /// Shared process-wide GPU context
    pub context: Arc<ProcessGpuContext>,
    /// Per-window surface state. `None` for a window using the
    /// `HostProcessBackend` render backend: that backend's whole point is
    /// that the *parent* process never creates a swapchain or calls
    /// `Surface::configure`/`Surface::present` for this window at all (those
    /// are exactly where every GPU crash diagnosed this session happened) --
    /// the parent still needs a `WebGpuState` for CPU-side work that's
    /// backend-agnostic (`RenderState`/`GlyphCache`'s atlas texture), just
    /// not a real surface. See `new_device_only`.
    pub surface: Option<WindowGpuSurface>,
    /// Liveness flag for this specific window's WebGpuState, checked by the
    /// device-lost callback registered in `new`.
    ///
    /// Since device-lost is now a process-wide event (all windows share the
    /// same device), we still need per-window liveness tracking to avoid
    /// triggering recovery for windows that have already been abandoned.
    /// When a window's WebGpuState is dropped (or superseded by an in-place
    /// rebuild), this flag is set to false.
    is_current: Arc<std::sync::atomic::AtomicBool>,
}

impl WebGpuState {
    /// Convenience accessor for adapter_info (delegates to shared context)
    pub fn adapter_info(&self) -> &wgpu::AdapterInfo {
        &self.context.adapter_info
    }

    /// Convenience accessor for downlevel_caps (delegates to shared context)
    pub fn downlevel_caps(&self) -> &wgpu::DownlevelCapabilities {
        &self.context.downlevel_caps
    }

    /// Convenience accessor for device (delegates to shared context)
    pub fn device(&self) -> &wgpu::Device {
        &self.context.device
    }

    /// Convenience accessor for queue (delegates to shared context)
    pub fn queue(&self) -> &Arc<wgpu::Queue> {
        &self.context.queue
    }

    /// Convenience accessor for shader_uniform_bind_group_layout (delegates to shared context)
    pub fn shader_uniform_bind_group_layout(&self) -> &wgpu::BindGroupLayout {
        &self.context.shader_uniform_bind_group_layout
    }

    /// Convenience accessor for texture_bind_group_layout (delegates to shared context)
    pub fn texture_bind_group_layout(&self) -> &wgpu::BindGroupLayout {
        &self.context.texture_bind_group_layout
    }

    /// Convenience accessor for texture_nearest_sampler (delegates to shared context)
    pub fn texture_nearest_sampler(&self) -> &wgpu::Sampler {
        &self.context.texture_nearest_sampler
    }

    /// Convenience accessor for texture_linear_sampler (delegates to shared context)
    pub fn texture_linear_sampler(&self) -> &wgpu::Sampler {
        &self.context.texture_linear_sampler
    }

    /// Get the HWND for this surface (Windows only). `None` for a
    /// device-only `WebGpuState` (see `surface`'s doc comment) as well as
    /// for a surface with no dedicated child HWND.
    #[cfg(windows)]
    pub fn client_hwnd(&self) -> Option<isize> {
        self.surface.as_ref().and_then(|s| s.client_hwnd())
    }

    /// Get the render pipeline for this window's surface format.
    /// The pipeline is cached in the shared context, keyed by format.
    ///
    /// Only ever called from `submit_frame`, which a device-only
    /// `WebGpuState` (see `surface`'s doc comment) never runs -- its window
    /// uses `HostProcessBackend`, which submits frames in a child process
    /// instead.
    fn get_render_pipeline(&self) -> wgpu::RenderPipeline {
        let config = self
            .surface
            .as_ref()
            .expect("get_render_pipeline requires a WindowGpuSurface")
            .config
            .lock();
        let supports_reinterpret_view = self
            .context
            .downlevel_caps
            .flags
            .contains(wgpu::DownlevelFlags::SURFACE_VIEW_FORMATS);
        let render_format = if supports_reinterpret_view {
            config.format.remove_srgb_suffix()
        } else {
            config.format
        };
        self.context.get_or_create_pipeline(render_format)
    }

    /// Marks this WebGpuState as abandoned (superseded by a rebuild).
    /// Device-lost events that arrive after this are ignored.
    pub fn mark_stale(&self) {
        self.is_current
            .store(false, std::sync::atomic::Ordering::Release);
    }

    /// Check if this WebGpuState is still the current one for its window.
    pub fn is_current(&self) -> bool {
        self.is_current.load(std::sync::atomic::Ordering::Acquire)
    }
}

pub struct RawHandlePair {
    window: RawWindowHandle,
    display: RawDisplayHandle,
}

impl RawHandlePair {
    fn new(window: &Window) -> Self {
        Self {
            window: window.window_handle().expect("window handle").as_raw(),
            display: window.display_handle().expect("display handle").as_raw(),
        }
    }

    /// Build a handle pair that targets a specific child HWND directly,
    /// rather than `window`'s own top-level HWND.
    ///
    /// Used to point the WebGpu surface at the dedicated `WS_CHILD` window
    /// created alongside the top-level window (see
    /// `window::os::windows::Window::webgpu_child_hwnd`) instead of the
    /// top-level HWND, so that DXGI's one-swapchain-per-HWND rule doesn't
    /// get in the way of a future in-place renderer rebuild (task #253).
    /// The display handle is still taken from `window`, since
    /// `WindowsDisplayHandle` is a zero-sized marker with no HWND of its own
    /// (see `HasDisplayHandle for Window`/`WindowInner`, which construct the
    /// exact same marker regardless of which HWND is involved).
    #[cfg(windows)]
    fn from_child_hwnd(hwnd: isize, window: &Window) -> Self {
        use std::num::NonZeroIsize;

        let mut handle = Win32WindowHandle::new(NonZeroIsize::new(hwnd).expect("non-zero hwnd"));
        // SAFETY: passing `null()` for the module name returns the handle of
        // the current process's exe, which is always valid and non-null; the
        // child window was created (via `CreateWindowExW`) with that same
        // HINSTANCE, so this mirrors the top-level window's own
        // `HasWindowHandle` impl.
        let hinstance = unsafe { winapi::um::libloaderapi::GetModuleHandleW(std::ptr::null()) };
        handle.hinstance = NonZeroIsize::new(hinstance as isize);

        Self {
            window: RawWindowHandle::Win32(handle),
            display: window.display_handle().expect("display handle").as_raw(),
        }
    }
}

impl HasWindowHandle for RawHandlePair {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        // SAFETY: `borrow_raw` requires the handle to outlive the returned
        // borrow. `self.window` is an owned `RawWindowHandle` living as long as
        // `&self`, matching the returned `WindowHandle<'_>` lifetime.
        unsafe { Ok(WindowHandle::borrow_raw(self.window)) }
    }
}

impl HasDisplayHandle for RawHandlePair {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        // SAFETY: `borrow_raw` requires the handle to outlive the returned
        // borrow. `self.display` is an owned `RawDisplayHandle` living as long
        // as `&self`, matching the returned `DisplayHandle<'_>` lifetime.
        unsafe { Ok(DisplayHandle::borrow_raw(self.display)) }
    }
}

pub struct WebGpuTexture {
    texture: wgpu::Texture,
    width: u32,
    height: u32,
    queue: Arc<wgpu::Queue>,
    /// Whether `write()` records only into the CPU replay log, for a
    /// window using `webgpu_engine: HostProcess` to mirror this atlas in a
    /// child process without ever sharing the GPU resource itself -- see
    /// `termwindow::webgpu::wire`. `false` for every other window, at the
    /// cost of one relaxed atomic load per `write()` and no allocation.
    ///
    /// Present as an always-there `AtomicBool` (toggled via `&self`) rather
    /// than an `Option` set once via `&mut self`: an atlas texture is only
    /// ever reachable through `Rc<dyn Texture2d>` (`Atlas::texture()`),
    /// which never grants exclusive access, so a toggle requiring `&mut
    /// self` could never actually be reached through the glyph cache's
    /// normal API (found while wiring up the caller in `renderstate.rs`).
    mirroring_enabled: std::sync::atomic::AtomicBool,
    mirror: parking_lot::Mutex<AtlasMirrorLog>,
}

/// `(x, y, width, height)` of one atlas write, in texels.
type AtlasRect = (u32, u32, u32, u32);

/// Keep the parent-side replay log bounded.  The log is deliberately capped
/// below the size at which a full wire frame would become an unsafe transient
/// allocation (the frame body and the child each need their own copy).  If a
/// window needs more atlas memory than this, the host-process backend falls
/// back to the in-process renderer rather than allowing an unbounded Rust OOM
/// abort on the GUI thread.
const MAX_ATLAS_MIRROR_BYTES: usize = 128 * 1024 * 1024;

/// The record `WebGpuTexture::write` keeps while mirroring is enabled.
///
/// Two levels, because a mirror needs both:
/// * `pending` -- what changed since the last frame was shipped, which is
///   all a child that is already up to date needs.
/// * `written` -- the whole atlas as a replayable set of writes, which is
///   what a *freshly spawned* child needs: its mirror texture is brand new
///   and blank, while this process's atlas has been accumulating sprites
///   since the window opened. Without this, every respawn produced a child
///   that could only ever learn about glyphs rasterized *after* it started,
///   i.e. a window that lost all its text the moment its GPU-host child was
///   replaced.
///
/// Keyed by rect so that repeatedly rewriting the same region (animated
/// images, the cursor sprite) replaces rather than accumulates, which bounds
/// this by the atlas's occupancy instead of by how long the window has been
/// open. Full replay is sorted by rect and pending deltas retain first-write
/// order. Atlas allocations never overlap (`guillotiere` hands out disjoint
/// rectangles, and a regrow builds a whole new texture with its own fresh
/// log), so the only rects that can collide are identical ones, which are
/// collapsed here to their latest content anyway.
// Rects come from our own `guillotiere` atlas allocator, not remote/attacker
// input, so ahash's faster (non-cryptographic) hashing is the right
// trade-off here -- same reasoning as the GUI's glyph caches.
type AtlasRectMap = std::collections::HashMap<AtlasRect, std::sync::Arc<[u8]>, ahash::RandomState>;
type AtlasRectSet = std::collections::HashSet<AtlasRect, ahash::RandomState>;

struct AtlasMirrorLog {
    written: AtlasRectMap,
    pending: Vec<AtlasRect>,
    pending_set: AtlasRectSet,
    bytes: usize,
    max_bytes: usize,
    over_budget: bool,
}

impl Default for AtlasMirrorLog {
    fn default() -> Self {
        Self::with_limit(MAX_ATLAS_MIRROR_BYTES)
    }
}

impl AtlasMirrorLog {
    fn with_limit(max_bytes: usize) -> Self {
        Self {
            written: AtlasRectMap::default(),
            pending: Vec::new(),
            pending_set: AtlasRectSet::default(),
            bytes: 0,
            max_bytes,
            over_budget: false,
        }
    }

    fn record(&mut self, rect: AtlasRect, pixels: &[u8]) {
        if self.over_budget {
            return;
        }
        let old_len = self.written.get(&rect).map_or(0, |old| old.len());
        let Some(bytes) = self
            .bytes
            .checked_sub(old_len)
            .and_then(|bytes| bytes.checked_add(pixels.len()))
        else {
            self.over_budget = true;
            return;
        };
        if bytes > self.max_bytes {
            self.over_budget = true;
            log::warn!(
                "HostProcess atlas mirror exceeded its {} MiB budget; falling back to in-process rendering",
                self.max_bytes / (1024 * 1024),
            );
            return;
        }

        self.written.insert(rect, std::sync::Arc::from(pixels));
        self.bytes = bytes;
        if self.pending_set.insert(rect) {
            self.pending.push(rect);
        }
    }

    fn updates_for(written: &AtlasRectMap, rect: AtlasRect) -> Option<wire::AtlasUpdate> {
        written.get(&rect).map(|pixels| wire::AtlasUpdate {
            x: rect.0,
            y: rect.1,
            width: rect.2,
            height: rect.3,
            pixels: std::sync::Arc::clone(pixels),
        })
    }

    fn is_over_budget(&self) -> bool {
        self.over_budget
    }
}

impl std::ops::Deref for WebGpuTexture {
    type Target = wgpu::Texture;
    fn deref(&self) -> &Self::Target {
        &self.texture
    }
}

impl Texture2d for WebGpuTexture {
    fn write(&self, rect: Rect, im: &dyn BitmapImage) {
        let (im_width, im_height) = im.image_dimensions();

        if self
            .mirroring_enabled
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            self.mirror.lock().record(
                (
                    rect.min_x() as u32,
                    rect.min_y() as u32,
                    im_width as u32,
                    im_height as u32,
                ),
                im.pixel_data_slice(),
            );
            return;
        }

        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: rect.min_x() as u32,
                    y: rect.min_y() as u32,
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            im.pixel_data_slice(),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(im_width as u32 * 4),
                rows_per_image: Some(im_height as u32),
            },
            wgpu::Extent3d {
                width: im_width as u32,
                height: im_height as u32,
                depth_or_array_layers: 1,
            },
        );
    }

    fn read(&self, _rect: Rect, _im: &mut dyn BitmapImage) {
        unimplemented!();
    }

    fn width(&self) -> usize {
        self.width as usize
    }

    fn height(&self) -> usize {
        self.height as usize
    }
}

impl WebGpuTexture {
    pub fn texture(&self) -> &wgpu::Texture {
        &self.texture
    }

    pub fn new(width: u32, height: u32, state: &WebGpuState) -> anyhow::Result<Self> {
        let limit = state.device().limits().max_texture_dimension_2d;

        if width > limit || height > limit {
            // Ideally, wgpu would have a fallible create_texture method,
            // but it doesn't: instead it will panic if the requested
            // dimension is too large.
            // So we check the limit ourselves here.
            // <https://github.com/wezterm/wezterm/issues/3713>
            anyhow::bail!(
                "texture dimensions {width}x{height} exceeed the \
                 max dimension {limit} supported by your GPU"
            );
        }

        let format = wgpu::TextureFormat::Rgba8UnormSrgb;
        let view_formats = if state
            .downlevel_caps()
            .flags
            .contains(wgpu::DownlevelFlags::SURFACE_VIEW_FORMATS)
        {
            vec![format, format.remove_srgb_suffix()]
        } else {
            vec![]
        };
        let texture = state.device().create_texture(&wgpu::TextureDescriptor {
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            label: Some("Texture Atlas"),
            view_formats: &view_formats,
        });
        Ok(Self {
            texture,
            width,
            height,
            queue: Arc::clone(state.queue()),
            mirroring_enabled: std::sync::atomic::AtomicBool::new(false),
            mirror: parking_lot::Mutex::new(AtlasMirrorLog::default()),
        })
    }

    /// Turns on atlas-write mirroring for a `HostProcess` backend: every
    /// subsequent `write()` appends its `(rect, pixel bytes)` only to the
    /// internal log, which `drain_dirty_updates` hands to the caller (e.g.
    /// once per frame, before shipping a `wire::WireFrame` to the child
    /// process). Idempotent. Enable immediately after the atlas's initial
    /// clear and before writing any glyphs: earlier glyphs cannot be replayed.
    /// The local texture is not used for rendering in this mode; switching
    /// backends must create a fresh atlas, as the GUI rebuild path does.
    pub fn enable_mirroring(&self) {
        if !self
            .mirroring_enabled
            .swap(true, std::sync::atomic::Ordering::Relaxed)
        {
            // Atlas::new queued its initial clear before mirroring was enabled.
            // HostProcess windows never submit local draws to drain that upload.
            self.queue.submit(std::iter::empty());
        }
    }

    /// Takes everything recorded since the last call (or since
    /// `enable_mirroring`, if this is the first). Returns an empty `Vec` if
    /// mirroring was never enabled.
    pub fn drain_dirty_updates(&self) -> Vec<wire::AtlasUpdate> {
        let mut mirror = self.mirror.lock();
        let mirror = &mut *mirror;
        if mirror.is_over_budget() {
            return vec![];
        }
        let pending = std::mem::take(&mut mirror.pending);
        mirror.pending_set.clear();
        pending
            .into_iter()
            .filter_map(|rect| AtlasMirrorLog::updates_for(&mirror.written, rect))
            .collect()
    }

    /// Every write recorded since `enable_mirroring`, not just the ones
    /// since the last drain -- what a mirror that is starting from a blank
    /// texture needs in order to catch up in a single frame (a first attach,
    /// or a reattach after the GPU-host child process was respawned; see
    /// `FrameForm::Wire`'s `full_resync`). Also clears the pending delta,
    /// since everything in it is included here.
    pub fn full_atlas_updates(&self) -> Vec<wire::AtlasUpdate> {
        let mut mirror = self.mirror.lock();
        if mirror.is_over_budget() {
            return vec![];
        }
        mirror.pending.clear();
        mirror.pending_set.clear();
        let mut updates: Vec<_> = mirror
            .written
            .iter()
            .map(|(rect, pixels)| wire::AtlasUpdate {
                x: rect.0,
                y: rect.1,
                width: rect.2,
                height: rect.3,
                pixels: std::sync::Arc::clone(pixels),
            })
            .collect();
        // Keep full resyncs deterministic, matching the former BTreeMap
        // iteration order. Delta updates retain first-write order in `pending`.
        updates.sort_unstable_by_key(|update| (update.x, update.y, update.width, update.height));
        updates
    }

    /// Whether the replay log has stopped accepting writes because retaining
    /// another atlas rect would exceed its bounded memory budget.
    pub fn mirroring_failed(&self) -> bool {
        self.mirror.lock().is_over_budget()
    }
}

pub fn adapter_info_to_gpu_info(info: wgpu::AdapterInfo) -> GpuInfo {
    GpuInfo {
        name: info.name,
        vendor: Some(info.vendor),
        device: Some(info.device),
        device_type: format!("{:?}", info.device_type),
        driver: if info.driver.is_empty() {
            None
        } else {
            Some(info.driver)
        },
        driver_info: if info.driver_info.is_empty() {
            None
        } else {
            Some(info.driver_info)
        },
        backend: format!("{:?}", info.backend),
    }
}

/// Clamp a requested surface size to a maximum.
/// (e.g. the GPU adapter's maximum texture dimension)
///
/// This is necessary before `Surface::configure` as it raises a validation error if either
/// dimension exceeds the GPU supported max dimension.
///
/// This can happen on macOS with tiling window managers that transiently compute
/// window geometry spanning multiple Retina displays.
///  (which becomes a fatal panic when called from an Objective-C -> Rust FFI callback)
/// See https://github.com/wezterm/wezterm/issues/7819.
fn clamp_surface_dimensions(width: u32, height: u32, max_texture_dimension_2d: u32) -> (u32, u32) {
    // A degenerate adapter reporting max_texture_dimension_2d == 0 would clamp
    // everything to zero, bypassing the > 0 guard that skips surface.configure.
    // Pass through unchanged in that case and let wgpu surface validation
    // surface the error.
    if max_texture_dimension_2d == 0 {
        return (width, height);
    }
    let clamped_w = width.min(max_texture_dimension_2d);
    let clamped_h = height.min(max_texture_dimension_2d);
    if clamped_w != width || clamped_h != height {
        log::warn!(
            "Clamped surface size from {}x{} to {}x{} (max_texture_dimension_2d={})",
            width,
            height,
            clamped_w,
            clamped_h,
            max_texture_dimension_2d
        );
    }
    (clamped_w, clamped_h)
}

// Compile-time regression guard: `WebGpuState` must be `Send + Sync` so that
// it can eventually live behind an `Arc` on a dedicated render thread. This
// fails to compile if that ever regresses (e.g. a `!Send`/`!Sync` field is
// added back, such as a raw `RawWindowHandle`/`RawDisplayHandle` or a
// `RefCell`).
const _: fn() = || {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<WebGpuState>();
    assert_send_sync::<ProcessGpuContext>();
    assert_send_sync::<WindowGpuSurface>();
};

// Compile-time regression guard for this task specifically: every call site
// now wraps `WebGpuState` in `Arc` (rather than `Rc`, which is not `Send`)
// precisely so that a clone of the `Arc` can be handed to a dedicated render
// thread later. Confirm that wrapping actually yields a `Send` value.
const _: fn() = || {
    fn assert_send<T: Send>() {}
    assert_send::<std::sync::Arc<WebGpuState>>();
};

// Compile-time regression guard: `GpuFrame` bundles up everything needed to
// submit a frame (detached `wgpu::Buffer`s, an owned `wgpu::Texture` clone,
// and a plain `ShaderUniform` value) without holding on to anything tied to
// `TermWindow`/`RenderState`, precisely so it can be handed across a thread
// boundary in a later task.
const _: fn() = || {
    fn assert_send<T: Send>() {}
    assert_send::<GpuFrame>();
};

#[cfg(test)]
mod atlas_upload_test;

#[cfg(test)]
mod tests {
    use super::{clamp_surface_dimensions, AtlasMirrorLog};
    use crate::webgpu::state_impl::needs_explicit_clear;

    const SAMPLE_MAX_TEXTURE_DIMENSION_2D: u32 = 16384;

    #[test]
    fn no_clamp_when_within_limit() {
        assert_eq!(
            clamp_surface_dimensions(1920, 1080, SAMPLE_MAX_TEXTURE_DIMENSION_2D),
            (1920, 1080)
        );
    }

    #[test]
    fn clamps_width_exceeding_max() {
        assert_eq!(
            clamp_surface_dimensions(19872, 2260, SAMPLE_MAX_TEXTURE_DIMENSION_2D),
            (SAMPLE_MAX_TEXTURE_DIMENSION_2D, 2260)
        );
    }

    #[test]
    fn clamps_height_exceeding_max() {
        assert_eq!(
            clamp_surface_dimensions(1920, 20000, SAMPLE_MAX_TEXTURE_DIMENSION_2D),
            (1920, SAMPLE_MAX_TEXTURE_DIMENSION_2D)
        );
    }

    #[test]
    fn clamps_both_dimensions() {
        assert_eq!(
            clamp_surface_dimensions(20000, 20000, SAMPLE_MAX_TEXTURE_DIMENSION_2D),
            (
                SAMPLE_MAX_TEXTURE_DIMENSION_2D,
                SAMPLE_MAX_TEXTURE_DIMENSION_2D
            )
        );
    }

    #[test]
    fn zero_dimensions_pass_through() {
        assert_eq!(
            clamp_surface_dimensions(0, 0, SAMPLE_MAX_TEXTURE_DIMENSION_2D),
            (0, 0)
        );
    }

    #[test]
    fn degenerate_max_zero_passes_through() {
        assert_eq!(clamp_surface_dimensions(1920, 1080, 0), (1920, 1080));
    }

    #[test]
    fn exact_limit_is_not_clamped() {
        assert_eq!(
            clamp_surface_dimensions(
                SAMPLE_MAX_TEXTURE_DIMENSION_2D,
                SAMPLE_MAX_TEXTURE_DIMENSION_2D,
                SAMPLE_MAX_TEXTURE_DIMENSION_2D
            ),
            (
                SAMPLE_MAX_TEXTURE_DIMENSION_2D,
                SAMPLE_MAX_TEXTURE_DIMENSION_2D
            )
        );
    }

    #[test]
    fn needs_clear_when_no_draws_issued() {
        // When cleared=false (no render passes were created in the draw loop),
        // we need an explicit clear pass to avoid presenting stale swapchain contents.
        assert!(
            needs_explicit_clear(false),
            "Should need explicit clear when no draws were issued"
        );
    }

    #[test]
    fn no_clear_needed_when_at_least_one_draw_issued() {
        // When cleared=true (at least one render pass was created),
        // the first render pass already cleared the surface, so no extra clear pass needed.
        assert!(
            !needs_explicit_clear(true),
            "Should not need explicit clear when at least one draw was issued"
        );
    }

    #[test]
    fn atlas_mirror_replacing_a_rect_does_not_accumulate_bytes() {
        let rect = (0, 0, 1, 1);
        let mut mirror = AtlasMirrorLog::with_limit(8);

        mirror.record(rect, &[1, 2, 3, 4]);
        mirror.record(rect, &[5, 6]);

        assert_eq!(mirror.bytes, 2);
        assert_eq!(mirror.written.len(), 1);
        assert_eq!(mirror.pending.len(), 1);
        assert_eq!(mirror.pending_set.len(), 1);
        assert!(!mirror.is_over_budget());
    }

    #[test]
    fn atlas_mirror_rejects_a_write_before_allocating_over_budget() {
        let mut mirror = AtlasMirrorLog::with_limit(4);

        mirror.record((0, 0, 1, 1), &[1, 2, 3]);
        mirror.record((1, 0, 1, 1), &[4, 5]);

        assert_eq!(mirror.bytes, 3);
        assert_eq!(mirror.written.len(), 1);
        assert_eq!(mirror.pending.len(), 1);
        assert!(mirror.is_over_budget());
    }

    #[test]
    fn atlas_mirror_updates_share_retained_payload() {
        let rect = (0, 0, 1, 1);
        let mut mirror = AtlasMirrorLog::with_limit(8);
        mirror.record(rect, &[1, 2, 3, 4]);

        let update = AtlasMirrorLog::updates_for(&mirror.written, rect).unwrap();
        assert_eq!(
            std::sync::Arc::strong_count(mirror.written.get(&rect).unwrap()),
            2
        );
        assert_eq!(&*update.pixels, &[1, 2, 3, 4]);
    }
}

pub use self::context::{ProcessGpuContext, WindowGpuSurface};
mod context;
mod state_impl;
