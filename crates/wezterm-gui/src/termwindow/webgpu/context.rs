use super::*;

use config::{ConfigHandle, WebGpuPowerPreference};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use wgpu::util::DeviceExt;
use window::raw_window_handle::RawWindowHandle;
use window::{Dimensions, Window, WindowOps};

/// Wrapper for the cached process-wide GPU context that tracks staleness.
///
/// When the device is lost, the cached context is marked as stale so that
/// subsequent calls to `get_or_init()` will recreate it instead of reusing
/// the lost device.
struct ContextEntry {
    /// The actual GPU context
    context: Arc<ProcessGpuContext>,
    /// Global version counter incremented on each device loss.
    /// If the cached entry's version doesn't match the global version,
    /// the context is stale and must be recreated.
    version: u64,
}

/// Global version counter incremented on device loss.
/// When this changes, any cached `ContextEntry` with a matching version
/// is stale and `ProcessGpuContext` must be recreated.
static CONTEXT_VERSION: AtomicU64 = AtomicU64::new(0);

/// A subscriber for device-lost events.
///
/// Each window maintains a weak reference to its own `is_current` flag
/// (which is set to false when the window's WebGpuState is abandoned) and
/// a cloned Window handle that can receive recovery notifications via
/// `Window::notify`.
struct DeviceLostSubscriber {
    /// Weak reference to the window's `is_current` flag. If upgrade fails,
    /// the window has been dropped and this entry should be pruned.
    is_current: Arc<AtomicBool>,
    /// Cloned Window handle for sending recovery notifications.
    window: Window,
}

/// Process-wide GPU context shared by all windows.
/// Created once per process on first window creation and reused thereafter.
///
/// Contains resources that are expensive to initialize and don't depend on
/// any specific window: Instance, Adapter, Device, Queue, shader module,
/// bind group layouts, samplers, and a pipeline cache keyed by surface format.
pub struct ProcessGpuContext {
    /// wgpu instance
    pub instance: wgpu::Instance,
    /// Chosen adapter
    pub adapter: wgpu::Adapter,
    /// Adapter info for diagnostics
    pub adapter_info: wgpu::AdapterInfo,
    /// Downlevel capabilities from the adapter
    pub downlevel_caps: wgpu::DownlevelCapabilities,
    /// The device
    pub device: wgpu::Device,
    /// The queue (Arc'd for sharing)
    pub queue: Arc<wgpu::Queue>,
    /// Shader module (same for all windows)
    shader: wgpu::ShaderModule,
    /// Shader uniform bind group layout
    pub shader_uniform_bind_group_layout: wgpu::BindGroupLayout,
    /// Texture bind group layout
    pub texture_bind_group_layout: wgpu::BindGroupLayout,
    /// Nearest sampler
    pub texture_nearest_sampler: wgpu::Sampler,
    /// Linear sampler
    pub texture_linear_sampler: wgpu::Sampler,
    /// Pipeline cache keyed by surface format.
    /// Different windows/monitors can legitimately have different formats,
    /// so we cache pipelines per format rather than building one unconditionally.
    pipeline_cache: Mutex<HashMap<wgpu::TextureFormat, wgpu::RenderPipeline>>,
    /// Subscribers to device-lost events. Each entry holds a weak reference
    /// to a window's `is_current` flag and a cloned Window handle. When the
    /// device is lost, the callback iterates this list and notifies all still-
    /// current windows (skipping dead entries where `is_current` upgrade fails).
    /// Wrapped in Arc so the device-lost callback can access it.
    device_lost_subscribers: Arc<Mutex<Vec<DeviceLostSubscriber>>>,
    /// Static corner buffer for instanced rendering (4 corners).
    /// Used with VertexStepMode::Vertex.
    pub corner_buffer: wgpu::Buffer,
    /// Shared index buffer for all quads (6 indices: 0,1,2,1,2,3).
    pub index_buffer: wgpu::Buffer,
}

impl ProcessGpuContext {
    /// Get or create the process-wide GPU context.
    ///
    /// The lock is released before the async init `await`s (a `MutexGuard`
    /// can't be held across an `.await`), so two callers can briefly race
    /// into the slow path. The version check after init ensures only one
    /// result is cached if a device loss occurs during initialization.
    pub async fn get_or_init(config: &ConfigHandle) -> anyhow::Result<Arc<Self>> {
        use std::sync::Mutex;
        static CONTEXT_LOCK: Mutex<Option<ContextEntry>> = Mutex::new(None);
        let expected_version = CONTEXT_VERSION.load(Ordering::Acquire);
        {
            let guard = CONTEXT_LOCK.lock().unwrap();
            if let Some(ref entry) = *guard {
                // Return cached context only if it's still valid (version matches)
                if entry.version == expected_version {
                    return Ok(Arc::clone(&entry.context));
                }
            }
        }
        // Cache was empty or stale; create a fresh context
        let context = Arc::new(Self::new(config).await?);
        // Re-check version after async init; if it changed, another thread
        // already replaced the cache and we should drop our new context.
        let current_version = CONTEXT_VERSION.load(Ordering::Acquire);
        let mut guard = CONTEXT_LOCK.lock().unwrap();
        if let Some(ref existing) = *guard {
            if existing.version == current_version && current_version >= expected_version {
                // Another thread beat us to the update; use their result
                return Ok(Arc::clone(&existing.context));
            }
        }
        // Our new context is authoritative; cache it
        *guard = Some(ContextEntry {
            context: Arc::clone(&context),
            version: current_version,
        });
        Ok(context)
    }

    /// Create a new ProcessGpuContext.
    /// This is called on the first window creation via get_or_init.
    pub async fn new(config: &ConfigHandle) -> anyhow::Result<Self> {
        use super::state_impl::run_on_background_thread;

        let primary_backends = wgpu::Backends::DX12;
        let all_backends = wgpu::Backends::all();

        let config_for_thread = config.clone();
        let (instance, _backends, adapter, device, queue) =
            run_on_background_thread(
                move || -> Result<(wgpu::Instance, wgpu::Backends, wgpu::Adapter, wgpu::Device, wgpu::Queue), anyhow::Error> {
                    for backends in [primary_backends, all_backends] {
                    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
                        backends,
                        ..Default::default()
                    });

                    let mut adapter: Option<wgpu::Adapter> = None;

                    if let Some(preference) = &config_for_thread.webgpu_preferred_adapter {
                        for a in instance.enumerate_adapters(backends) {
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

                        if adapter.is_none() && backends == all_backends {
                            log::warn!(
                                "Your webgpu preferred adapter '{}' was not found among the \
                                 enumerated adapters",
                                preference,
                            );
                        }
                    }

                    let may_settle_for_any_adapter =
                        config_for_thread.webgpu_preferred_adapter.is_none()
                            || backends == all_backends;

                    if adapter.is_none() && may_settle_for_any_adapter {
                        adapter = futures::executor::block_on(instance.request_adapter(
                            &wgpu::RequestAdapterOptions {
                                power_preference: match config_for_thread.webgpu_power_preference {
                                    WebGpuPowerPreference::HighPerformance => {
                                        wgpu::PowerPreference::HighPerformance
                                    }
                                    WebGpuPowerPreference::LowPower => wgpu::PowerPreference::LowPower,
                                },
                                compatible_surface: None,
                                force_fallback_adapter: config_for_thread.webgpu_force_fallback_adapter,
                            },
                        ))
                        .ok();
                    }

                    if let Some(adapter) = adapter {
                        let (device, queue) = futures::executor::block_on(
                            adapter.request_device(&wgpu::DeviceDescriptor {
                                required_features: wgpu::Features::empty(),
                                required_limits: if cfg!(target_arch = "wasm32") {
                                    wgpu::Limits::downlevel_webgl2_defaults()
                                } else {
                                    wgpu::Limits::downlevel_defaults()
                                }
                                .using_resolution(adapter.limits()),
                                label: None,
                                memory_hints: Default::default(),
                                trace: wgpu::Trace::Off,
                            }),
                        )?;
                        return Ok((instance, backends, adapter, device, queue));
                    }

                    log::debug!(
                        "no compatible adapter found among backends {backends:?}; \
                         widening backend search",
                    );
                }

                Err(anyhow::anyhow!("no compatible adapter found (enumeration was empty)"))
            })
            .await??;

        let adapter_info = adapter.get_info();
        log::trace!("Using adapter: {adapter_info:?}");
        let downlevel_caps = adapter.get_downlevel_capabilities();
        log::trace!("downlevel_caps: {downlevel_caps:?}");

        let queue = Arc::new(queue);

        let shader = device.create_shader_module(wgpu::include_wgsl!("../../shader.wgsl"));

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

        // Create static corner buffer for instanced rendering
        use crate::quad::CornerVertex;
        let corners = CornerVertex::static_corners();
        let corner_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Corner Buffer (instanced rendering)"),
            usage: wgpu::BufferUsages::VERTEX,
            contents: bytemuck::cast_slice(&corners),
        });

        // Create shared index buffer (6 indices per quad: 0,1,2, 1,2,3)
        let indices: [u32; 6] = [0, 1, 2, 1, 2, 3];
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Index Buffer (shared)"),
            usage: wgpu::BufferUsages::INDEX,
            contents: bytemuck::cast_slice(&indices),
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

        // Device-lost callback for the shared context.
        // Since the device is shared across all windows, this is a single,
        // process-wide callback that fans out to all active windows via the
        // subscriber registry.
        // We need to wrap the Mutex in an Arc so the callback can access it.
        let subscribers = Arc::new(Mutex::new(Vec::<DeviceLostSubscriber>::new()));
        let subscribers_for_callback = Arc::clone(&subscribers);
        device.set_device_lost_callback(move |reason, message| {
            log::error!(
                "Shared GPU context device lost ({:?}): {}; notifying all active windows",
                reason,
                message
            );
            metrics::counter!("gui.render_thread.device_lost").increment(1);

            // Increment version to invalidate the cached context.
            // Any subsequent `get_or_init()` call will see the version mismatch
            // and recreate the ProcessGpuContext with a fresh device.
            CONTEXT_VERSION.fetch_add(1, Ordering::Release);

            // Iterate over subscribers, notifying each still-current window.
            // Prune dead entries (those where `is_current` upgrade fails).
            let mut guard = subscribers_for_callback.lock();
            let mut alive_subscribers = Vec::new();
            for subscriber in guard.drain(..) {
                // Check if this window is still current. If the upgrade fails,
                // the window has been dropped and we skip it.
                if subscriber
                    .is_current
                    .load(std::sync::atomic::Ordering::Acquire)
                {
                    log::error!(
                        "wgpu device lost ({:?}): {}; this window's GPU adapter/device was reset \
                         (e.g. a Windows TDR) -- triggering an in-place renderer rebuild",
                        reason,
                        message
                    );
                    let win = subscriber.window.clone();
                    let win2 = win.clone();
                    let reason_msg = format!(
                        "this window's wgpu device was lost ({:?}): {}",
                        reason, message
                    );
                    win.notify(crate::termwindow::TermWindowNotif::Apply(Box::new(
                        move |tw| {
                            tw.handle_render_error_recovery(&win2, &reason_msg);
                        },
                    )));
                    alive_subscribers.push(subscriber);
                }
                // If `is_current` is false, this window has been abandoned;
                // we drop the entry rather than keeping it around.
            }
            *guard = alive_subscribers;
        });

        Ok(Self {
            instance,
            adapter,
            adapter_info,
            downlevel_caps,
            device,
            queue,
            shader,
            shader_uniform_bind_group_layout,
            texture_bind_group_layout,
            texture_nearest_sampler,
            texture_linear_sampler,
            pipeline_cache: Mutex::new(HashMap::new()),
            device_lost_subscribers: subscribers,
            corner_buffer,
            index_buffer,
        })
    }

    /// Register a new window as a subscriber to device-lost events.
    ///
    /// When the shared device is lost, the callback will notify all still-
    /// current windows by calling `Window::notify` with a recovery action.
    /// Windows that have been abandoned (their `is_current` flag is false)
    /// are skipped and their entries are pruned from the registry.
    pub fn register_device_lost_subscriber(&self, window: Window, is_current: Arc<AtomicBool>) {
        let subscriber = DeviceLostSubscriber { window, is_current };
        self.device_lost_subscribers.lock().push(subscriber);
    }

    /// Get or create a render pipeline for the given surface format.
    /// Pipelines are cached per format since different windows/monitors can
    /// legitimately have different formats.
    pub fn get_or_create_pipeline(&self, format: wgpu::TextureFormat) -> wgpu::RenderPipeline {
        let mut cache = self.pipeline_cache.lock();
        if let Some(pipeline) = cache.get(&format) {
            return pipeline.clone();
        }

        let render_pipeline_layout =
            self.device
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("Render Pipeline Layout"),
                    bind_group_layouts: &[
                        &self.shader_uniform_bind_group_layout,
                        &self.texture_bind_group_layout,
                        &self.texture_bind_group_layout,
                    ],
                    push_constant_ranges: &[],
                });

        let render_pipeline = self
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Render Pipeline"),
                layout: Some(&render_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &self.shader,
                    entry_point: Some("vs_main"),
                    buffers: &[
                        crate::quad::CornerVertex::desc(),
                        crate::quad::QuadInstance::desc(),
                    ],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &self.shader,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
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

        cache.insert(format, render_pipeline.clone());
        render_pipeline
    }
}

/// Per-window GPU surface state.
/// Created for each window and contains the surface, its configuration,
/// dimensions, and the HWND it targets.
pub struct WindowGpuSurface {
    /// The wgpu surface for this window
    pub surface: wgpu::Surface<'static>,
    /// Surface configuration (mutex for thread-safe access)
    pub config: Mutex<wgpu::SurfaceConfiguration>,
    /// Window dimensions (mutex for thread-safe access)
    pub dimensions: Mutex<Dimensions>,
    /// The HWND this surface targets (for resize workaround on Windows)
    #[cfg(windows)]
    client_hwnd: Option<isize>,
}

impl WindowGpuSurface {
    /// Create a new WindowGpuSurface.
    ///
    /// This is called for each window. The surface format is determined from
    /// the adapter's capabilities for this specific surface.
    pub async fn new(
        context: &Arc<ProcessGpuContext>,
        handle: &RawHandlePair,
        dimensions: Dimensions,
        config: &ConfigHandle,
    ) -> anyhow::Result<Self> {
        use super::clamp_surface_dimensions;
        use super::state_impl::compute_compatibility_list;

        // SAFETY: `create_surface_unsafe` is the only way to build a surface
        // from a raw window handle. `handle` is a `RawHandlePair` whose owned
        // handles are valid (obtained from a live window in `RawHandlePair::new`)
        // and satisfy the `HasWindowHandle` contract for the surface's lifetime.
        let surface = unsafe {
            context
                .instance
                .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::from_window(handle)?)?
        };

        // Extract the HWND for the resize workaround on Windows
        #[cfg(windows)]
        let client_hwnd = match handle.window {
            RawWindowHandle::Win32(h) => Some(h.hwnd.get()),
            _ => None,
        };

        // The adapter was chosen without knowledge of this surface.
        // Confirm it's actually compatible; if not, this is an error since
        // we're sharing the adapter across all windows.
        // TODO: multi-adapter support would require a more sophisticated approach here.
        if !context.adapter.is_surface_supported(&surface) {
            let adapters =
                compute_compatibility_list(&context.instance, wgpu::Backends::all(), &surface);
            anyhow::bail!(
                "Shared adapter {} is not compatible with this window surface. \
                 Available:\n{}",
                adapter_info_to_gpu_info(context.adapter_info.clone()),
                adapters.join("\n")
            );
        }

        let caps = surface.get_capabilities(&context.adapter);
        log::trace!("surface caps: {caps:?}");

        // Explicitly request an SRGB format, if available
        let pref_format_srgb = caps.formats[0].add_srgb_suffix();
        let format = if caps.formats.contains(&pref_format_srgb) {
            pref_format_srgb
        } else {
            caps.formats[0]
        };

        // Need to check that this is supported, as trying to set
        // view_formats without it will cause surface.configure to panic
        let supports_reinterpret_view = context
            .downlevel_caps
            .flags
            .contains(wgpu::DownlevelFlags::SURFACE_VIEW_FORMATS);
        let view_formats = if supports_reinterpret_view {
            vec![format.add_srgb_suffix(), format.remove_srgb_suffix()]
        } else {
            vec![]
        };

        let max_dim = context.device.limits().max_texture_dimension_2d;
        let (init_width, init_height) = clamp_surface_dimensions(
            dimensions.pixel_width as u32,
            dimensions.pixel_height as u32,
            max_dim,
        );

        let wants_transparency = config.window_background_opacity != 1.0
            || config.window_background_image.is_some()
            || config.window_background_gradient.is_some()
            || !config.background.is_empty();
        let alpha_mode = if wants_transparency {
            if caps
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
            }
        } else if caps.alpha_modes.contains(&wgpu::CompositeAlphaMode::Opaque) {
            wgpu::CompositeAlphaMode::Opaque
        } else {
            wgpu::CompositeAlphaMode::Auto
        };

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: init_width,
            height: init_height,
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode,
            view_formats,
            desired_maximum_frame_latency: 2,
        };

        surface.configure(&context.device, &surface_config);

        Ok(Self {
            surface,
            config: Mutex::new(surface_config),
            dimensions: Mutex::new(dimensions),
            #[cfg(windows)]
            client_hwnd,
        })
    }

    /// Get the HWND for this surface (Windows only).
    #[cfg(windows)]
    pub fn client_hwnd(&self) -> Option<isize> {
        self.client_hwnd
    }
}
