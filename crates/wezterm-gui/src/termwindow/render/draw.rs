use crate::termwindow::webgpu::{GpuDraw, GpuFrame, ShaderUniform};
use crate::termwindow::RenderFrame;
use std::time::Instant;

impl crate::TermWindow {
    pub fn call_draw(&mut self, frame: &mut RenderFrame) -> anyhow::Result<()> {
        match frame {
            RenderFrame::WebGpu => self.call_draw_webgpu(),
        }
    }

    fn call_draw_webgpu(&mut self) -> anyhow::Result<()> {
        let frame = self.build_webgpu_frame()?;
        if let Some(rt) = self.render_thread.as_ref() {
            rt.send_frame(frame);
        } else {
            self.webgpu.as_ref().unwrap().submit_frame(frame)?;
        }
        Ok(())
    }

    /// Builds a `GpuFrame`: everything that needs `self`/`render_state`
    /// (uniform inputs, and detaching this frame's vertex/index buffers
    /// from each layer's `TripleVertexBuffer`), but does none of the
    /// actual GPU submission -- that's `WebGpuState::submit_frame`'s job,
    /// which only needs `&WebGpuState` and can eventually run off the GUI
    /// thread.
    fn build_webgpu_frame(&mut self) -> anyhow::Result<GpuFrame> {
        use crate::termwindow::webgpu::WebGpuTexture;

        let render_state = self.render_state.as_ref().unwrap();
        let webgpu_state = self.webgpu.as_ref().unwrap();

        let tex = render_state.glyph_cache.borrow().atlas.texture();
        let tex = tex.downcast_ref::<WebGpuTexture>().unwrap();
        let atlas = wgpu::Texture::clone(tex.texture());

        let foreground_text_hsb = self.config.foreground_text_hsb;
        let foreground_text_hsb = [
            foreground_text_hsb.hue,
            foreground_text_hsb.saturation,
            foreground_text_hsb.brightness,
        ];

        let milliseconds = self.created.elapsed().as_millis() as u32;
        let projection = euclid::Transform3D::<f32, f32, f32>::ortho(
            -(self.dimensions.pixel_width as f32) / 2.0,
            self.dimensions.pixel_width as f32 / 2.0,
            self.dimensions.pixel_height as f32 / 2.0,
            -(self.dimensions.pixel_height as f32) / 2.0,
            -1.0,
            1.0,
        )
        .to_arrays_transposed();

        let mut draws = Vec::new();

        // Instrumentation only (see docs/plans/2026-07-31-remaining-followups.md,
        // section 1 / item 7): each `recreate()` below allocates a brand new
        // GPU buffer at the sub-layer's full *capacity* (not the number of
        // quads actually in use), which costs a `create_buffer` plus a
        // full-capacity staging-buffer memset + copy, on the GUI thread, once
        // per non-empty sub-layer per frame. These two histograms measure the
        // real-world cost so that a future decision is data-driven rather
        // than estimated: only pursue a buffer-pooling redesign of this loop
        // if `gui.webgpu_frame.vertex_recreate.size` regularly exceeds ~8MB
        // per frame, or `gui.webgpu_frame.vertex_recreate.latency` regularly
        // exceeds ~2ms, on real workloads.
        let vertex_recreate_start = Instant::now();
        let mut vertex_recreate_bytes: u64 = 0;

        for layer in render_state.layers.borrow().iter() {
            for idx in 0..3 {
                let vb = &layer.vb.borrow()[idx];
                let instance_count = vb.instance_count();
                if instance_count > 0 {
                    // Upload everything painting accumulated for this layer
                    // this frame (write_instances_to_gpu does the actual
                    // Queue::write_buffer) and use the buffer/count it
                    // hands back -- `current_vb_mut()` alone would only
                    // return whatever buffer object already exists, without
                    // ever writing this frame's instances into it.
                    let (instance_buffer, actual_count) = vb.write_instances_to_gpu();
                    vertex_recreate_bytes += (actual_count as usize
                        * std::mem::size_of::<crate::quad::QuadInstance>())
                        as u64;

                    // Use shared index buffer from the WebGpu context
                    let index_buffer = wgpu::Buffer::clone(&webgpu_state.context.index_buffer);
                    draws.push(GpuDraw {
                        vertex_buffer: instance_buffer,
                        index_buffer,
                        index_count: actual_count * 6, // 6 indices per quad
                        instance_count: actual_count,
                    });
                }

                vb.next_index();
            }
        }

        metrics::histogram!("gui.webgpu_frame.vertex_recreate.latency")
            .record(vertex_recreate_start.elapsed());
        metrics::histogram!("gui.webgpu_frame.vertex_recreate.size")
            .record(vertex_recreate_bytes as f64);

        Ok(GpuFrame {
            draws,
            atlas,
            uniform: ShaderUniform {
                foreground_text_hsb,
                milliseconds,
                projection,
            },
        })
    }
}
