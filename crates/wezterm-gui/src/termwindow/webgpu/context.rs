use super::*;

use config::{ConfigHandle, WebGpuPowerPreference};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};
use wgpu::util::DeviceExt;
use window::raw_window_handle::RawWindowHandle;
use window::{Dimensions, Window, WindowOps};

/// A subscriber for device-lost events.
///
/// Each window maintains a weak reference to its own `is_current` flag
/// (which is set to false when the window's WebGpuState is abandoned) and
/// a cloned Window handle that can receive recovery notifications via
/// `Window::notify`.
struct DeviceLostSubscriber {
    /// Weak reference to the window's `is_current` flag. If upgrade fails,
    /// the window has been dropped and this entry should be pruned.
    is_current: Weak<AtomicBool>,
    /// Cloned Window handle for sending recovery notifications.
    window: Window,
}

/// Whether a `DeviceLostSubscriber` should still be notified (and kept in
/// the registry) on the next device-lost event: its owning `Arc<AtomicBool>`
/// must still be alive (the window wasn't dropped) and currently read
/// `true` (the window hasn't abandoned this subscription). `is_current` is
/// stored as `Weak` specifically so that dropping a window's owning `Arc`
/// is what makes this return `false` -- if the registry held a strong
/// `Arc` instead, no window could ever actually become unreachable while
/// still registered, which was the leak this predicate exists to prevent.
fn subscriber_is_alive(is_current: &Weak<AtomicBool>) -> bool {
    is_current
        .upgrade()
        .map(|flag| flag.load(Ordering::Acquire))
        .unwrap_or(false)
}

/// Installs a `ProcessGpuContext`'s device-lost callback: marks `lost` and
/// notifies still-current subscribers.
///
/// Extracted so `ProcessGpuContext::new` can call this immediately after
/// `request_device` succeeds, inside the background-thread closure that
/// creates the device -- installing it any later (e.g. back on the async
/// fn's thread, after several more GPU calls building shaders/buffers/
/// samplers) leaves a real window where a device lost in the meantime is
/// silently swallowed forever: wgpu's `set_device_lost_callback` only fills
/// an empty closure slot, it does not replay a loss that already consumed
/// the slot before a callback was ever registered.
fn install_device_lost_callback(
    device: &wgpu::Device,
    lost: Arc<AtomicBool>,
    subscribers: Arc<Mutex<Vec<DeviceLostSubscriber>>>,
) {
    device.set_device_lost_callback(move |reason, message| {
        log::error!(
            "Shared GPU context device lost ({:?}): {}; notifying all active windows",
            reason,
            message
        );
        metrics::counter!("gui.render_thread.device_lost").increment(1);

        // Mark this context's own device as lost. `get_or_init` checks
        // this flag (not a global counter) so that even a device lost
        // before this `Self::new()` call returns is still correctly
        // seen as stale by the next caller.
        lost.store(true, Ordering::Release);

        // Iterate over subscribers, notifying each still-current window.
        // Prune dead entries: either the owning window was dropped (weak
        // upgrade fails) or it was abandoned (`is_current` reads false).
        let mut guard = subscribers.lock();
        let mut alive_subscribers = Vec::new();
        for subscriber in guard.drain(..) {
            if !subscriber_is_alive(&subscriber.is_current) {
                continue;
            }
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
        *guard = alive_subscribers;
    });
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
    /// Set to `true` by this context's own device-lost callback when its
    /// `device` is lost. Scoped to this specific context (not a global
    /// counter) so that a device lost during `Self::new()` -- before this
    /// `ProcessGpuContext` is even returned and cached -- still correctly
    /// marks itself stale for `get_or_init`'s next caller, with no window
    /// where the loss can be mistaken for having happened to some other,
    /// already-superseded context.
    lost: Arc<AtomicBool>,
    /// Static corner buffer for instanced rendering (4 corners).
    /// Used with VertexStepMode::Vertex.
    pub corner_buffer: wgpu::Buffer,
    /// Shared index buffer for all quads (6 indices: 0,1,2,1,2,3).
    pub index_buffer: wgpu::Buffer,
}

impl ProcessGpuContext {
    /// Get or create the process-wide GPU context.
    ///
    /// The lock is an async-aware `smol::lock::Mutex` (unlike a
    /// `std`/`parking_lot` `MutexGuard`, its guard can be held across an
    /// `.await`), so it's held for the *entire* check-init-cache sequence,
    /// including the `Self::new(config).await` call. A second caller
    /// arriving while initialization is already in progress waits for it to
    /// finish and then observes the freshly cached result, rather than
    /// racing into its own duplicate `Instance`/`Adapter`/`Device`
    /// enumeration.
    pub async fn get_or_init(config: &ConfigHandle) -> anyhow::Result<Arc<Self>> {
        static CONTEXT_LOCK: smol::lock::Mutex<Option<Arc<ProcessGpuContext>>> =
            smol::lock::Mutex::new(None);
        let mut guard = CONTEXT_LOCK.lock().await;
        if let Some(ref context) = *guard {
            // Return the cached context only if its own device hasn't been
            // lost. Checking the context's own flag (rather than a global
            // version counter) means this is correct even if the device was
            // lost while this very context was still being built inside
            // `Self::new()` below -- the flag it carries out is already the
            // truth, so there's no window where a freshly-lost context can
            // be stamped "fresh" by a caller that raced its own callback.
            if !context.lost.load(Ordering::Acquire) {
                return Ok(Arc::clone(context));
            }
        }
        // Cache was empty or its device was lost; create a fresh context.
        // The lock stays held across this await, so no other caller can
        // observe (or itself trigger) a second concurrent initialization.
        let context = Arc::new(Self::new(config).await?);
        *guard = Some(Arc::clone(&context));
        Ok(context)
    }

    /// Create a new ProcessGpuContext.
    /// This is called on the first window creation via get_or_init.
    pub async fn new(config: &ConfigHandle) -> anyhow::Result<Self> {
        use super::state_impl::run_on_background_thread;

        let primary_backends = wgpu::Backends::DX12;
        let all_backends = wgpu::Backends::all();

        // Created before the background thread that will create the device
        // itself, and cloned into that thread's closure below, so the
        // device-lost callback can be installed the instant `request_device`
        // succeeds -- see `install_device_lost_callback`'s doc comment for
        // why installing it any later leaves a real gap.
        let subscribers = Arc::new(Mutex::new(Vec::<DeviceLostSubscriber>::new()));
        let lost = Arc::new(AtomicBool::new(false));

        let config_for_thread = config.clone();
        let subscribers_for_thread = Arc::clone(&subscribers);
        let lost_for_thread = Arc::clone(&lost);
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
                        // Install the device-lost callback immediately, on
                        // this same background thread, before doing any
                        // further GPU work (shaders/buffers/samplers) that
                        // could give a loss room to happen in an
                        // uncovered gap.
                        install_device_lost_callback(
                            &device,
                            Arc::clone(&lost_for_thread),
                            Arc::clone(&subscribers_for_thread),
                        );
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
            lost,
            corner_buffer,
            index_buffer,
        })
    }

    /// Register a new window as a subscriber to device-lost events.
    ///
    /// When the shared device is lost, the callback will notify all still-
    /// current windows by calling `Window::notify` with a recovery action.
    /// Windows that have been abandoned (their `is_current` flag is false)
    /// or dropped (weak reference upgrade fails) are skipped and their
    /// entries are pruned from the registry.
    pub fn register_device_lost_subscriber(&self, window: Window, is_current: Arc<AtomicBool>) {
        let subscriber = DeviceLostSubscriber {
            window,
            is_current: Arc::downgrade(&is_current),
        };
        let mut subscribers = self.device_lost_subscribers.lock();
        // Prune dead entries here too, not just in the device-lost callback:
        // a device-lost event may never fire during the process lifetime,
        // in which case the registry would otherwise grow by one entry per
        // window ever opened and closed, for as long as the process runs.
        subscribers.retain(|s| subscriber_is_alive(&s.is_current));
        subscribers.push(subscriber);
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

#[cfg(test)]
mod device_lost_subscriber_test {
    use super::*;

    #[test]
    fn dropped_owner_is_not_alive() {
        let is_current = Arc::new(AtomicBool::new(true));
        let weak = Arc::downgrade(&is_current);
        assert!(subscriber_is_alive(&weak));

        drop(is_current);
        assert!(
            !subscriber_is_alive(&weak),
            "a subscriber whose owning Arc<AtomicBool> was dropped (window closed) \
             must not be reported alive, or it can never be pruned from the registry"
        );
    }

    #[test]
    fn abandoned_but_still_alive_owner_is_not_alive() {
        let is_current = Arc::new(AtomicBool::new(true));
        let weak = Arc::downgrade(&is_current);
        assert!(subscriber_is_alive(&weak));

        is_current.store(false, Ordering::Release);
        assert!(
            !subscriber_is_alive(&weak),
            "a subscriber whose owner is still alive but marked itself abandoned \
             (is_current = false) must not be reported alive"
        );

        // The Arc itself is still alive here; only its flag flipped. Keep it
        // alive across the assertion so this test exercises the flag check,
        // not upgrade() failing for an unrelated reason.
        drop(is_current);
    }
}
