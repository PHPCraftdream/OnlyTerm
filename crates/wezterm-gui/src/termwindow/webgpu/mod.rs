use config::GpuInfo;
use parking_lot::Mutex;
use std::sync::Arc;
use window::bitmaps::Texture2d;
#[cfg(windows)]
use window::raw_window_handle::Win32WindowHandle;
use window::raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
    RawWindowHandle, WindowHandle,
};
use window::{BitmapImage, Dimensions, Rect, Window};

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
    pub index_buffer: wgpu::Buffer,
    pub index_count: u32,
}

/// Everything needed to submit one frame's worth of draw calls, computed
/// without needing further access to `TermWindow`/`RenderState`.
pub struct GpuFrame {
    pub draws: Vec<GpuDraw>,
    pub atlas: wgpu::Texture,
    pub uniform: ShaderUniform,
}

pub struct WebGpuState {
    pub adapter_info: wgpu::AdapterInfo,
    pub downlevel_caps: wgpu::DownlevelCapabilities,
    pub surface: wgpu::Surface<'static>,
    pub device: wgpu::Device,
    pub queue: Arc<wgpu::Queue>,
    // Lock ordering: never hold either lock across a call to
    // `self.surface.configure(...)`; if both are needed, take `dimensions`
    // before `config`.
    pub config: Mutex<wgpu::SurfaceConfiguration>,
    pub dimensions: Mutex<Dimensions>,
    pub render_pipeline: wgpu::RenderPipeline,
    shader_uniform_bind_group_layout: wgpu::BindGroupLayout,
    pub texture_bind_group_layout: wgpu::BindGroupLayout,
    pub texture_nearest_sampler: wgpu::Sampler,
    pub texture_linear_sampler: wgpu::Sampler,
    /// The live HWND that the WebGpu surface actually targets, sampled from
    /// the `RawWindowHandle` at construction time.
    ///
    /// This used to be (and still is) used by `resize()`'s `GetClientRect`
    /// workaround, but its role has expanded: since task #252, the surface
    /// is no longer created against the application's own top-level HWND
    /// directly -- it targets a dedicated `WS_CHILD` window (see
    /// `window::os::windows::Window::webgpu_child_hwnd` /
    /// `create_webgpu_child_window`) that exactly overlays the top-level
    /// window's client area and is input-transparent
    /// (`WM_NCHITTEST`->`HTTRANSPARENT`). This exists because DXGI only
    /// allows one swapchain per HWND: putting the surface on its own child
    /// HWND is what will let a future in-place renderer rebuild (task #253,
    /// not yet implemented) tear down and recreate the surface without
    /// fighting the top-level window's swapchain lifetime. So this field is
    /// now simultaneously "the resize workaround's HWND" AND "the actual
    /// surface target HWND" -- they're the same child HWND, not the
    /// top-level one. We deliberately don't keep the `RawHandlePair` itself
    /// around: `raw-window-handle`'s enums are `!Send`/`!Sync` on account of
    /// non-Windows variants, even though we only ever hold a Windows one.
    #[cfg(windows)]
    client_hwnd: Option<isize>,
    /// Liveness flag for this specific device/surface instance, checked by
    /// the `set_device_lost_callback` closure registered in `new` (task
    /// #254) before it acts on a device-lost event (task #267).
    ///
    /// `wgpu::Device::set_device_lost_callback` fires the callback for
    /// however long the underlying `wgpu::Device` handle is alive, with no
    /// way to unregister it -- and, since it's invoked from
    /// `Device::handle_hal_error`/`Device::lose` on whatever thread
    /// happened to make the wgpu call that observed the failure (see the
    /// call site in `new` below), a device that this window has already
    /// abandoned (superseded by an in-place rebuild, task #253, or replaced
    /// entirely by a permanent OpenGL fallback, task #255) can still fire a
    /// *late* device-lost event -- exactly what a real TDR produces on the
    /// very device this whole recovery machinery exists to escape from.
    /// Without this flag, that late event would reach
    /// `TermWindow::handle_render_error_recovery` and either charge a
    /// spurious rebuild attempt against a perfectly healthy *new* WebGpu
    /// device (rebuild case) or, worse, drag a window that has permanently
    /// moved to OpenGL back into the WebGpu rebuild dance (fallback case).
    ///
    /// Set to `true` for the lifetime of this instance; the `TermWindow`
    /// call sites that abandon this `WebGpuState` (`begin_renderer_rebuild`
    /// and `begin_opengl_fallback`, both in `termwindow/mod.rs`) flip it to
    /// `false` at the moment they take/drop it, *before* the replacement
    /// device (if any) exists -- so the callback's check can never
    /// race-observe stale-but-not-yet-marked-stale state from the GUI
    /// thread's perspective (both the flip and the `notify()` re-entry that
    /// reads it are serialized through the GUI thread's `TermWindowNotif`
    /// dispatch).
    is_current: Arc<std::sync::atomic::AtomicBool>,
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
        let limit = state.device.limits().max_texture_dimension_2d;

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
            .downlevel_caps
            .flags
            .contains(wgpu::DownlevelFlags::SURFACE_VIEW_FORMATS)
        {
            vec![format, format.remove_srgb_suffix()]
        } else {
            vec![]
        };
        let texture = state.device.create_texture(&wgpu::TextureDescriptor {
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
            queue: Arc::clone(&state.queue),
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
}

mod state_impl;
