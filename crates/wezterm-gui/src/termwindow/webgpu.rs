use crate::quad::Vertex;
use anyhow::anyhow;
use config::{ConfigHandle, GpuInfo, WebGpuPowerPreference};
use parking_lot::Mutex;
use std::sync::Arc;
use wgpu::util::DeviceExt;
use window::bitmaps::Texture2d;
use window::raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
    RawWindowHandle, WindowHandle,
};
#[cfg(windows)]
use window::raw_window_handle::Win32WindowHandle;
use window::{BitmapImage, Dimensions, Rect, Window, WindowOps};

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

fn compute_compatibility_list(
    instance: &wgpu::Instance,
    backends: wgpu::Backends,
    surface: &wgpu::Surface,
) -> Vec<String> {
    instance
        .enumerate_adapters(backends)
        .into_iter()
        .map(|a| {
            let info = adapter_info_to_gpu_info(a.get_info());
            let compatible = a.is_surface_supported(&surface);
            format!(
                "{}, compatible={}",
                info.to_string(),
                if compatible { "yes" } else { "NO" }
            )
        })
        .collect()
}

impl WebGpuState {
    pub async fn new(
        window: &Window,
        dimensions: Dimensions,
        config: &ConfigHandle,
    ) -> anyhow::Result<Self> {
        // On Windows, target the dedicated WebGpu child HWND (see
        // `Window::webgpu_child_hwnd`/task #252) instead of the top-level
        // window's own HWND, so a future in-place renderer rebuild (task
        // #253) has a swapchain-free HWND to recreate against rather than
        // fighting DXGI's one-swapchain-per-HWND rule on the app's own
        // top-level window. If the child window couldn't be created for some
        // reason, fall back to the top-level window directly so WebGpu still
        // works (just without the future rebuild capability).
        #[cfg(windows)]
        let handle = match window.webgpu_child_hwnd() {
            Some(child_hwnd) => RawHandlePair::from_child_hwnd(child_hwnd, window),
            None => RawHandlePair::new(window),
        };
        #[cfg(not(windows))]
        let handle = RawHandlePair::new(window);

        let state = Self::new_impl(handle, dimensions, config).await?;

        // Wire up wgpu's device-lost signal (task #254) to the same
        // in-place-rebuild recovery path task #253 built for a hung render
        // thread. A real Windows TDR (Timeout Detection & Recovery) resets
        // the GPU adapter, which wgpu surfaces by invalidating the device
        // and invoking this callback -- see wgpu-core 25.0.2's
        // `Device::lose` (called from `Device::handle_hal_error` on
        // `hal::DeviceError::Lost`/`ResourceCreationFailed`/`Unexpected`),
        // which runs the callback synchronously, inline, on whatever thread
        // happened to make the wgpu call that observed the failure. In this
        // codebase that's always our own render thread (`submit_one_frame`
        // et al never call into wgpu from any other thread), so this
        // closure -- like `submit_one_frame`'s own `SurfaceError::Other`
        // handling right below it -- runs on the render thread, never on
        // some wgpu-internal thread, and the same `window.notify(...)` +
        // `TermWindowNotif::Apply` bridge back to the GUI thread applies
        // unchanged. `set_device_lost_callback` only requires the closure to
        // be `Fn(..) + Send + 'static`, which a cloned `Window` (itself
        // `Clone + Send`, see `RenderThreadSeed::window`'s existing use)
        // satisfies without any unsafe/FFI.
        //
        // Also capture `state.is_current` (task #267): `set_device_lost_callback`
        // has no way to unregister itself, so this same closure keeps living
        // for as long as `state.device` does, including after this
        // `WebGpuState` has been abandoned by an in-place rebuild
        // (`begin_renderer_rebuild`) or a permanent OpenGL fallback
        // (`begin_opengl_fallback`) -- both of which call `mark_stale()` on
        // the outgoing `WebGpuState` before dropping it. A device-lost event
        // that fires after that point is a *late* signal from a device this
        // window has already moved on from, and must not trigger recovery:
        // mirrors `submit_one_frame`'s existing `window_destroyed` check in
        // `renderthread.rs`, just gated on "is this device still the current
        // one" instead of "is the window still alive".
        let win = window.clone();
        let is_current = Arc::clone(&state.is_current);
        state.device.set_device_lost_callback(move |reason, message| {
            if !is_current.load(std::sync::atomic::Ordering::Acquire) {
                // Stale event from an abandoned device (already superseded
                // by a rebuild, or this window has already permanently
                // fallen back to OpenGL): the recovery machinery this event
                // would otherwise trigger no longer applies to this device,
                // so just log and drop it.
                log::debug!(
                    "wgpu device lost ({:?}): {}; ignoring -- this device was already \
                     abandoned (superseded by a rebuild or a permanent OpenGL fallback)",
                    reason,
                    message
                );
                return;
            }
            log::error!(
                "wgpu device lost ({:?}): {}; this window's GPU adapter/device was reset \
                 (e.g. a Windows TDR) -- triggering an in-place renderer rebuild",
                reason,
                message
            );
            metrics::counter!("gui.render_thread.device_lost").increment(1);
            let win2 = win.clone();
            let reason_msg =
                format!("this window's wgpu device was lost ({:?}): {}", reason, message);
            win.notify(crate::termwindow::TermWindowNotif::Apply(Box::new(
                move |tw| {
                    tw.handle_render_error_recovery(&win2, &reason_msg);
                },
            )));
        });

        Ok(state)
    }

    pub async fn new_impl(
        handle: RawHandlePair,
        dimensions: Dimensions,
        config: &ConfigHandle,
    ) -> anyhow::Result<Self> {
        let backends = wgpu::Backends::all();
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends,
            ..Default::default()
        });
        // SAFETY: `create_surface_unsafe` is the only way to build a surface
        // from a raw window handle. `handle` is a `RawHandlePair` whose owned
        // handles are valid (obtained from a live window in
        // `RawHandlePair::new`) and satisfy the `HasWindowHandle` contract for
        // the surface's lifetime; the surface is dropped before the window.
        let surface = unsafe {
            instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::from_window(&handle)?)?
        };

        // `Surface::configure`/`create_surface_unsafe` above have already
        // copied out everything they need from `handle`, so it doesn't need
        // to be kept alive for their sake. We do still need the raw HWND
        // later for the `resize()` `GetClientRect` workaround, so extract
        // just that (a plain `isize`, which is `Send`/`Sync`) rather than
        // storing the whole `!Send`/`!Sync` `RawHandlePair`/`RawWindowHandle`.
        #[cfg(windows)]
        let client_hwnd = match handle.window {
            RawWindowHandle::Win32(h) => Some(h.hwnd.get()),
            _ => None,
        };

        let mut adapter: Option<wgpu::Adapter> = None;

        if let Some(preference) = &config.webgpu_preferred_adapter {
            for a in instance.enumerate_adapters(backends) {
                if !a.is_surface_supported(&surface) {
                    let info = adapter_info_to_gpu_info(a.get_info());
                    log::warn!("{} is not compatible with surface", info.to_string());
                    continue;
                }

                let info = a.get_info();

                if preference.name != info.name {
                    continue;
                }

                if preference.device_type != format!("{:?}", info.device_type) {
                    continue;
                }

                if preference.backend != format!("{:?}", info.backend) {
                    continue;
                }

                if let Some(driver) = &preference.driver {
                    if *driver != info.driver {
                        continue;
                    }
                }
                if let Some(vendor) = &preference.vendor {
                    if *vendor != info.vendor {
                        continue;
                    }
                }
                if let Some(device) = &preference.device {
                    if *device != info.device {
                        continue;
                    }
                }

                adapter.replace(a);
                break;
            }

            if adapter.is_none() {
                let adapters = compute_compatibility_list(&instance, backends, &surface);
                log::warn!(
                    "Your webgpu preferred adapter '{}' was either not \
                     found or is not compatible with your display. Available:\n{}",
                    preference.to_string(),
                    adapters.join("\n")
                );
            }
        }

        if adapter.is_none() {
            adapter = Some(
                instance
                    .request_adapter(&wgpu::RequestAdapterOptions {
                        power_preference: match config.webgpu_power_preference {
                            WebGpuPowerPreference::HighPerformance => {
                                wgpu::PowerPreference::HighPerformance
                            }
                            WebGpuPowerPreference::LowPower => wgpu::PowerPreference::LowPower,
                        },
                        compatible_surface: Some(&surface),
                        force_fallback_adapter: config.webgpu_force_fallback_adapter,
                    })
                    .await?,
            );
        }

        let adapter = adapter.ok_or_else(|| {
            let adapters = compute_compatibility_list(&instance, backends, &surface);
            anyhow!(
                "no compatible adapter found. Available:\n{}",
                adapters.join("\n")
            )
        })?;

        let adapter_info = adapter.get_info();
        log::trace!("Using adapter: {adapter_info:?}");
        let caps = surface.get_capabilities(&adapter);
        log::trace!("caps: {caps:?}");
        let downlevel_caps = adapter.get_downlevel_capabilities();
        log::trace!("downlevel_caps: {downlevel_caps:?}");

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                required_features: wgpu::Features::empty(),
                // WebGL doesn't support all of wgpu's features, so if
                // we're building for the web we'll have to disable some.
                required_limits: if cfg!(target_arch = "wasm32") {
                    wgpu::Limits::downlevel_webgl2_defaults()
                } else {
                    wgpu::Limits::downlevel_defaults()
                }
                .using_resolution(adapter.limits()),
                label: None,
                memory_hints: Default::default(),
                trace: wgpu::Trace::Off,
            })
            .await?;

        let queue = Arc::new(queue);

        // Explicitly request an SRGB format, if available
        let pref_format_srgb = caps.formats[0].add_srgb_suffix();
        let format = if caps.formats.contains(&pref_format_srgb) {
            pref_format_srgb
        } else {
            caps.formats[0]
        };

        // Need to check that this is supported, as trying to set
        // view_formats without it will cause surface.configure
        // to panic
        // <https://github.com/wezterm/wezterm/issues/3565>
        let view_formats = if downlevel_caps
            .flags
            .contains(wgpu::DownlevelFlags::SURFACE_VIEW_FORMATS)
        {
            vec![format.add_srgb_suffix(), format.remove_srgb_suffix()]
        } else {
            vec![]
        };

        let max_dim = device.limits().max_texture_dimension_2d;
        let (init_width, init_height) = clamp_surface_dimensions(
            dimensions.pixel_width as u32,
            dimensions.pixel_height as u32,
            max_dim,
        );
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: init_width,
            height: init_height,
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: if caps
                .alpha_modes
                .contains(&wgpu::CompositeAlphaMode::PostMultiplied)
            {
                wgpu::CompositeAlphaMode::PostMultiplied
            } else if caps
                .alpha_modes
                .contains(&wgpu::CompositeAlphaMode::PreMultiplied)
            {
                wgpu::CompositeAlphaMode::PreMultiplied
            } else {
                wgpu::CompositeAlphaMode::Auto
            },
            view_formats,
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let shader = device.create_shader_module(wgpu::include_wgsl!("../shader.wgsl"));

        let shader_uniform_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
                label: Some("ShaderUniform bind group layout"),
            });

        let texture_nearest_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let texture_linear_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let texture_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            multisampled: false,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
                label: Some("texture bind group layout"),
            });

        let render_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Render Pipeline Layout"),
                bind_group_layouts: &[
                    &shader_uniform_bind_group_layout,
                    &texture_bind_group_layout,
                    &texture_bind_group_layout,
                ],
                push_constant_ranges: &[],
            });

        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Render Pipeline"),
            layout: Some(&render_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[Vertex::desc()],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),

            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview: None,
            cache: None,
        });

        Ok(Self {
            adapter_info,
            downlevel_caps,
            surface,
            device,
            queue,
            config: Mutex::new(config),
            dimensions: Mutex::new(dimensions),
            render_pipeline,
            #[cfg(windows)]
            client_hwnd,
            shader_uniform_bind_group_layout,
            texture_bind_group_layout,
            texture_nearest_sampler,
            texture_linear_sampler,
            is_current: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        })
    }

    /// Marks this device/surface as abandoned: a device-lost event that
    /// arrives after this point (see the `set_device_lost_callback` closure
    /// registered in `new`) is stale and must not trigger recovery. Called
    /// by `TermWindow` (`begin_renderer_rebuild`/`begin_opengl_fallback` in
    /// `termwindow/mod.rs`, task #267) at the moment it takes/drops this
    /// `WebGpuState`, before any replacement device exists.
    pub fn mark_stale(&self) {
        self.is_current
            .store(false, std::sync::atomic::Ordering::Release);
    }

    pub fn create_uniform(&self, uniform: ShaderUniform) -> wgpu::BindGroup {
        let buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("ShaderUniform Buffer"),
                contents: bytemuck::cast_slice(&[uniform]),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            });
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &self.shader_uniform_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
            label: Some("ShaderUniform Bind Group"),
        })
    }

    /// Submit a pre-built `GpuFrame` to the GPU: acquires the current
    /// surface texture, builds the bind groups and one render pass per
    /// `GpuDraw`, submits the command buffer, and presents. This only
    /// needs `&self` (no `TermWindow`/`RenderState` access), which is what
    /// makes it callable from a dedicated render thread in a later task.
    pub fn submit_frame(&self, frame: GpuFrame) -> Result<(), wgpu::SurfaceError> {
        let output = self.surface.get_current_texture()?;
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Render Encoder"),
            });
        let texture_view = frame.atlas.create_view(&wgpu::TextureViewDescriptor::default());

        let texture_linear_bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &self.texture_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.texture_linear_sampler),
                },
            ],
            label: Some("linear bind group"),
        });

        let texture_nearest_bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &self.texture_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.texture_nearest_sampler),
                },
            ],
            label: Some("nearest bind group"),
        });

        let mut cleared = false;

        for draw in &frame.draws {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: if cleared {
                            wgpu::LoadOp::Load
                        } else {
                            wgpu::LoadOp::Clear(wgpu::Color {
                                r: 0.,
                                g: 0.,
                                b: 0.,
                                a: 0.,
                            })
                        },
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            cleared = true;

            let uniforms = self.create_uniform(frame.uniform);

            render_pass.set_pipeline(&self.render_pipeline);
            render_pass.set_bind_group(0, &uniforms, &[]);
            render_pass.set_bind_group(1, &texture_linear_bind_group, &[]);
            render_pass.set_bind_group(2, &texture_nearest_bind_group, &[]);
            render_pass.set_vertex_buffer(0, draw.vertex_buffer.slice(..));
            render_pass.set_index_buffer(draw.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            render_pass.draw_indexed(0..draw.index_count, 0, 0..1);
        }

        // submit will accept anything that implements IntoIter
        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        Ok(())
    }

    /// Resize (and, if the size actually changed, reconfigure) the surface.
    ///
    /// Render-thread-only once a window's render thread is active (task
    /// 221.6): the GUI thread routes through
    /// `TermWindow::resize_webgpu_surface`, which sends a `RenderMsg::Resize`
    /// instead of calling this directly whenever `self.render_thread` is
    /// `Some`. What serializes this against `submit_frame`/`reconfigure`
    /// running concurrently on the same `Arc<WebGpuState>` is that routing
    /// (all three only ever run on the render thread once it exists), not a
    /// lock -- there's deliberately no runtime thread-identity assertion
    /// here, because `resize` is ALSO legitimately called directly from the
    /// GUI thread when there's no render thread for this window (flag off,
    /// non-Windows, or spawn failed), and a blanket assert would be wrong in
    /// that case.
    #[allow(unused_mut)]
    pub fn resize(&self, mut dims: Dimensions) {
        // During a live resize on Windows, the Dimensions that we're processing may be
        // lagging behind the true client size. We have to take the very latest value
        // from the window or else the underlying driver will raise an error about
        // the mismatch, so we need to sneakily read through the handle
        #[cfg(windows)]
        if let Some(hwnd) = self.client_hwnd {
            // SAFETY: `RECT` is a POD with no validity invariants, so
            // `mem::zeroed()` yields a valid all-zero value as an out-param.
            let mut rect = unsafe { std::mem::zeroed() };
            // SAFETY: `hwnd` is the live HWND for this window and
            // `&mut rect` is a valid out-pointer. `GetClientRect` only
            // writes the client rectangle; on failure `rect` stays usable
            // because it was zero-initialised.
            unsafe { winapi::um::winuser::GetClientRect(hwnd as _, &mut rect) };
            dims.pixel_width = (rect.right - rect.left) as usize;
            dims.pixel_height = (rect.bottom - rect.top) as usize;
        }

        if dims == *self.dimensions.lock() {
            return;
        }
        // Store the unclamped dims: this field is only used to dedup resize
        // events against the size the window actually requested. Clamping is
        // applied below, only to the values handed to Surface::configure.
        *self.dimensions.lock() = dims;
        let mut config = self.config.lock();
        let max = self.device.limits().max_texture_dimension_2d;
        let (clamped_width, clamped_height) =
            clamp_surface_dimensions(dims.pixel_width as u32, dims.pixel_height as u32, max);
        config.width = clamped_width;
        config.height = clamped_height;
        if config.width > 0 && config.height > 0 {
            // Avoid reconfiguring with a 0 sized surface, as webgpu will
            // panic in that case
            // <https://github.com/wezterm/wezterm/issues/2881>
            self.surface.configure(&self.device, &config);
        }
    }

    /// Re-runs `surface.configure` with the currently stored config,
    /// unconditionally -- bypasses `resize`'s dedup-against-previous-size
    /// check. Used to recover from `SurfaceError::Lost`/`Outdated`, where the
    /// swapchain needs to be recreated even though the requested size hasn't
    /// changed.
    pub fn reconfigure(&self) {
        let config = self.config.lock();
        if config.width > 0 && config.height > 0 {
            self.surface.configure(&self.device, &config);
        }
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
