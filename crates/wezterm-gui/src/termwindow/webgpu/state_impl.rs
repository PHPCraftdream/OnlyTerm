use super::*;

use anyhow::anyhow;
use config::ConfigHandle;
use std::sync::Arc;
use wgpu::util::DeviceExt;
use window::{Dimensions, Window};

/// Helper function to compute a compatibility list for error messages.
pub fn compute_compatibility_list(
    instance: &wgpu::Instance,
    backends: wgpu::Backends,
    surface: &wgpu::Surface,
) -> Vec<String> {
    use super::adapter_info_to_gpu_info;
    instance
        .enumerate_adapters(backends)
        .into_iter()
        .map(|a| {
            let info = adapter_info_to_gpu_info(a.get_info());
            let compatible = a.is_surface_supported(surface);
            format!(
                "{}, compatible={}",
                info,
                if compatible { "yes" } else { "NO" }
            )
        })
        .collect()
}

/// Runs `f` on a dedicated, one-shot background OS thread and returns a
/// `promise::Future` that resolves with its result once the thread finishes.
///
/// This exists specifically so `ProcessGpuContext::new` can move
/// `Instance::new`/adapter selection (measured ~4s combined on some hardware
/// -- see task #311) off the GUI thread. `promise::spawn`'s executor (backed
/// by `window::spawn::SPAWN_QUEUE`, drained once per message-loop iteration
/// in `run_message_loop`) is cooperative, not preemptive: an `.await` inside
/// a GUI-thread task only yields control back to the message loop at a point
/// that resolves via a waker, so a future that is itself synchronously
/// CPU/driver-bound for seconds (as `Instance::new` is, since it is not a
/// truly asynchronous operation, just one wrapped in `Future` for
/// portability to wasm) blocks the entire message loop for that whole
/// duration, freezing every window in the process. Running `f` on a real OS
/// thread instead gives the message loop back to the OS scheduler for the
/// duration of `f`, and `promise::Promise::result`'s `waker.wake()` call
/// re-schedules the awaiting continuation onto `SPAWN_QUEUE` from that
/// thread, which is exactly the cross-thread wakeup this whole mechanism
/// depends on (`Promise`/`Future`'s shared `Core` is behind a `Mutex`, so
/// this is race-free regardless of which thread calls `result`/`poll`
/// first).
pub(super) fn run_on_background_thread<T, F>(f: F) -> promise::Future<T>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    let mut promise = promise::Promise::new();
    let future = promise.get_future().expect("promise was just created");
    std::thread::Builder::new()
        .name("webgpu-init".to_string())
        .spawn(move || {
            // `Promise` has no `Drop` impl that resolves a pending future, so
            // a panic escaping `f` would drop the promise silently and leave
            // the awaiting GUI-thread task parked forever -- a window stuck
            // blank with nothing logged. Before this work ran off-thread the
            // same panic surfaced immediately on the GUI thread, and losing
            // that would be a strictly worse failure mode, especially here:
            // `f` calls into GPU drivers, which is exactly the code most
            // likely to blow up on a broken machine.
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
                Ok(result) => promise.ok(result),
                Err(panic) => {
                    let msg = panic
                        .downcast_ref::<&str>()
                        .map(|s| s.to_string())
                        .or_else(|| panic.downcast_ref::<String>().cloned())
                        .unwrap_or_else(|| "non-string panic payload".to_string());
                    promise.err(anyhow!("webgpu initialization panicked: {msg}"))
                }
            };
        })
        .expect("failed to spawn webgpu-init thread");
    future
}

/// Determines whether an explicit clear render pass is needed.
///
/// After iterating through all draw calls, `cleared` indicates whether
/// any render pass was created. If `cleared` is `false`, the surface texture
/// was never written to, so we must add a clear-only render pass before
/// presenting to avoid showing stale swapchain contents.
///
/// This is a pure function extracted for testability: constructing a real
/// wgpu device/encoder for an end-to-end test is impractical in the test
/// harness, so we test this decision logic directly instead.
#[cfg_attr(test, allow(dead_code))]
pub fn needs_explicit_clear(cleared: bool) -> bool {
    !cleared
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
        let handle = match window.webgpu_child_hwnd() {
            Some(child_hwnd) => RawHandlePair::from_child_hwnd(child_hwnd, window),
            None => RawHandlePair::new(window),
        };

        // Get or create the shared process-wide GPU context.
        // This is the expensive part (Instance, adapter, device creation)
        // that only happens once per process.
        let context = ProcessGpuContext::get_or_init(config).await?;

        // Create the per-window surface
        let surface = WindowGpuSurface::new(&context, &handle, dimensions, config).await?;

        let is_current = Arc::new(std::sync::atomic::AtomicBool::new(true));

        // Register this window as a subscriber to device-lost events.
        // The shared ProcessGpuContext has exactly one device-lost callback
        // that fans out to all still-current windows via the subscriber registry.
        context.register_device_lost_subscriber(window.clone(), Arc::clone(&is_current));

        Ok(Self {
            context,
            surface,
            is_current,
        })
    }

    pub fn create_uniform(&self, uniform: ShaderUniform) -> wgpu::BindGroup {
        let buffer = self
            .device()
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("ShaderUniform Buffer"),
                contents: bytemuck::cast_slice(&[uniform]),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            });
        self.device().create_bind_group(&wgpu::BindGroupDescriptor {
            layout: self.shader_uniform_bind_group_layout(),
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
        let output = self.surface.surface.get_current_texture()?;
        // Reinterpret as the non-sRGB sibling format when the adapter
        // supports it, matching `render_format` in pipeline creation.
        // `None` (the texture's own, sRGB, format) is the correct fallback on adapters
        // that lack `DownlevelFlags::SURFACE_VIEW_FORMATS`, since `format`
        // wasn't added to `view_formats` for them either.
        let render_view_format = if self
            .context
            .downlevel_caps
            .flags
            .contains(wgpu::DownlevelFlags::SURFACE_VIEW_FORMATS)
        {
            Some(self.surface.config.lock().format.remove_srgb_suffix())
        } else {
            None
        };
        let view = output.texture.create_view(&wgpu::TextureViewDescriptor {
            format: render_view_format,
            ..Default::default()
        });
        let mut encoder = self
            .device()
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Render Encoder"),
            });
        let texture_view = frame
            .atlas
            .create_view(&wgpu::TextureViewDescriptor::default());

        let texture_linear_bind_group =
            self.device().create_bind_group(&wgpu::BindGroupDescriptor {
                layout: self.texture_bind_group_layout(),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&texture_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(self.texture_linear_sampler()),
                    },
                ],
                label: Some("linear bind group"),
            });

        let texture_nearest_bind_group =
            self.device().create_bind_group(&wgpu::BindGroupDescriptor {
                layout: self.texture_bind_group_layout(),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&texture_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(self.texture_nearest_sampler()),
                    },
                ],
                label: Some("nearest bind group"),
            });

        let mut cleared = false;

        // `frame.uniform` is a `Copy` struct that is byte-for-byte identical
        // for every draw call within this frame (projection/viewport etc. are
        // only ever recomputed once per `submit_frame` call, never per draw),
        // so the uniform buffer and its bind group only need to be built once
        // per frame rather than once per draw.
        let uniforms = self.create_uniform(frame.uniform);

        let render_pipeline = self.get_render_pipeline();

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

            render_pass.set_pipeline(&render_pipeline);
            render_pass.set_bind_group(0, &uniforms, &[]);
            render_pass.set_bind_group(1, &texture_linear_bind_group, &[]);
            render_pass.set_bind_group(2, &texture_nearest_bind_group, &[]);
            // Set corner buffer (vertex step mode, binding 0)
            render_pass.set_vertex_buffer(0, self.context.corner_buffer.slice(..));
            // Set instance buffer (instance step mode, binding 1)
            render_pass.set_vertex_buffer(1, draw.vertex_buffer.slice(..));
            // Use shared index buffer from context
            render_pass.set_index_buffer(
                self.context.index_buffer.slice(..),
                wgpu::IndexFormat::Uint32,
            );
            // Draw with instancing: instance_count from draw, 6 indices from shared buffer
            render_pass.draw_indexed(0..6, 0, 0..draw.instance_count);
        }

        // If no draws were issued, we still need to clear the surface texture
        // before presenting it; otherwise, we'd present stale contents from a
        // previous frame in the swapchain rotation.
        if needs_explicit_clear(cleared) {
            let _clear_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Clear Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.,
                            g: 0.,
                            b: 0.,
                            a: 0.,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });
        }

        // submit will accept anything that implements IntoIter
        self.queue().submit(std::iter::once(encoder.finish()));
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
        use super::state_impl::clamp_surface_dimensions;

        // During a live resize on Windows, the Dimensions that we're processing may be
        // lagging behind the true client size. We have to take the very latest value
        // from the window or else the underlying driver will raise an error about
        // the mismatch, so we need to sneakily read through the handle
        #[cfg(windows)]
        if let Some(hwnd) = self.surface.client_hwnd() {
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

        if dims == *self.surface.dimensions.lock() {
            return;
        }
        // Store the unclamped dims: this field is only used to dedup resize
        // events against the size the window actually requested. Clamping is
        // applied below, only to the values handed to Surface::configure.
        *self.surface.dimensions.lock() = dims;
        let mut config = self.surface.config.lock();
        let max = self.device().limits().max_texture_dimension_2d;
        let (clamped_width, clamped_height) =
            clamp_surface_dimensions(dims.pixel_width as u32, dims.pixel_height as u32, max);
        config.width = clamped_width;
        config.height = clamped_height;
        if config.width > 0 && config.height > 0 {
            // Avoid reconfiguring with a 0 sized surface, as webgpu will
            // panic in that case
            // <https://github.com/wezterm/wezterm/issues/2881>
            self.surface.surface.configure(self.device(), &config);
        }
    }

    /// Re-runs `surface.configure` with the currently stored config,
    /// unconditionally -- bypasses `resize`'s dedup-against-previous-size
    /// check. Used to recover from `SurfaceError::Lost`/`Outdated`, where the
    /// swapchain needs to be recreated even though the requested size hasn't
    /// changed.
    pub fn reconfigure(&self) {
        let config = self.surface.config.lock();
        if config.width > 0 && config.height > 0 {
            self.surface.surface.configure(self.device(), &config);
        }
    }
}
