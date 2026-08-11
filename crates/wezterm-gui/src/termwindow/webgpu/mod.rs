use config::GpuInfo;
use std::sync::Arc;
use window::bitmaps::Texture2d;
#[cfg(windows)]
use window::raw_window_handle::Win32WindowHandle;
use window::raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
    RawWindowHandle, WindowHandle,
};
use window::{BitmapImage, Rect, Window};

#[repr(C)]
#[derive(Copy, Clone, Default, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ShaderUniform {
    pub foreground_text_hsb: [f32; 3],
    pub milliseconds: u32,
    pub projection: [[f32; 4]; 4],
    // sampler2D atlas_nearest_sampler;
    // sampler2D atlas_linear_sampler;
}

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
    /// Per-window surface state
    pub surface: WindowGpuSurface,
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

    /// Get the HWND for this surface (Windows only)
    #[cfg(windows)]
    pub fn client_hwnd(&self) -> Option<isize> {
        self.surface.client_hwnd()
    }

    /// Get the render pipeline for this window's surface format.
    /// The pipeline is cached in the shared context, keyed by format.
    fn get_render_pipeline(&self) -> wgpu::RenderPipeline {
        let config = self.surface.config.lock();
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
        })
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
mod tests {
    use super::clamp_surface_dimensions;
    use crate::termwindow::webgpu::state_impl::needs_explicit_clear;

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
}

pub use self::context::{ProcessGpuContext, WindowGpuSurface};
mod context;
mod state_impl;
