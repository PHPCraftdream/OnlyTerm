use crate::quad::QuadInstance;
use crate::termwindow::webgpu::{GpuDraw, GpuFrame, ShaderUniform};
use crate::termwindow::RenderFrame;
use std::hash::{BuildHasher, Hash, Hasher};
use std::time::Instant;
use window::WindowOps;

/// Computes a frame signature from its constituent parts.
/// This is a pure function suitable for unit testing, extracted from
/// `TermWindow::compute_frame_signature` (which is a thin wrapper around this).
///
/// The signature includes:
/// - Window/surface dimensions (a resize with no content change still needs redraw)
/// - foreground_text_hsb (changes on config reload)
/// - projection matrix (changes on window resize)
/// - All layer QuadInstance data (position/color/tex/hsv/flags)
///
/// Uses a fixed/deterministic hasher seed so that two calls with byte-identical
/// inputs (across separate calls, i.e. separate frames) produce the same u64 output.
/// This is critical for frame-to-frame equality comparison in `call_draw_webgpu`.
///
/// Explicitly does NOT accept a `milliseconds` parameter, guaranteeing that
/// field's exclusion from the signature at the type level. This is critical:
/// if `milliseconds` were included, every frame would hash differently and
/// nothing would ever be skipped. The shader never reads milliseconds
/// (verified by grep in shader.wgsl), so excluding it cannot cause stale-pixel bugs.
///
/// Note on ahash constructor choice:
/// - `ahash::RandomState::new()` is "Each instance unique" - every call produces
///   a different random seed via process-wide counter (verified in ahash source).
///   This would defeat frame-to-frame comparison entirely.
/// - `ahash::RandomState::with_seeds(k0, k1, k2, k3)` is "Fixed" - deterministic,
///   identical calls produce identical hashers. This is the correct choice here.
pub fn compute_frame_signature_from_parts(
    pixel_width: usize,
    pixel_height: usize,
    foreground_text_hsb: &[f32; 3],
    projection: &[[f32; 4]; 4],
    layer_instances: &[&[QuadInstance]],
) -> u64 {
    // Use fixed/deterministic seeds so identical frames across calls produce identical signatures.
    // These constants are arbitrary but must be fixed/literal for determinism.
    const SIGNATURE_SEED_K0: u64 = 0x517cc1b727220a95;
    const SIGNATURE_SEED_K1: u64 = 0x8123456789abcdef;
    const SIGNATURE_SEED_K2: u64 = 0x0fedcba987654321;
    const SIGNATURE_SEED_K3: u64 = 0x1a2b3c4d5e6f7890;

    let state = ahash::RandomState::with_seeds(
        SIGNATURE_SEED_K0,
        SIGNATURE_SEED_K1,
        SIGNATURE_SEED_K2,
        SIGNATURE_SEED_K3,
    );
    let mut hasher = state.build_hasher();

    // Hash window/surface dimensions
    pixel_width.hash(&mut hasher);
    pixel_height.hash(&mut hasher);

    // Hash uniform fields that affect rendering (note: no milliseconds parameter)
    let hsb_bytes = bytemuck::cast_slice::<f32, u8>(foreground_text_hsb);
    hasher.write(hsb_bytes);

    // Hash the projection matrix
    let proj_bytes = bytemuck::cast_slice::<f32, u8>(projection.as_flattened());
    hasher.write(proj_bytes);

    // Hash each layer's QuadInstance data
    for instances in layer_instances {
        let bytes = bytemuck::cast_slice::<QuadInstance, u8>(instances);
        hasher.write(bytes);
    }

    hasher.finish()
}

/// Extracts what a `HeapQuadAllocator::apply_to` call actually wrote into
/// layer 0 of a fresh `BorrowedLayers` destination, as a `Vec<QuadInstance>`
/// suitable for `compute_frame_signature_from_parts`.
///
/// `apply_to` writes into the destination `MappedQuadsView`s themselves
/// (`BorrowedLayers::extend_from_slice`), NOT into the `TripleVertexBuffer`
/// that `map_instances()` was called on to construct those views -- that
/// buffer's own `instances` field only gets populated by a later,
/// explicit `accumulate_instances()` call, which `apply_to` does not make.
/// Reading `vb.instances` here would silently observe an empty Vec
/// regardless of what was applied; this helper reads the actual
/// destination instead.
#[cfg(test)]
fn applied_layer0_instances(
    heap: &crate::quad::HeapQuadAllocator,
    capacity: usize,
) -> Vec<QuadInstance> {
    use crate::quad::TripleLayerQuadAllocator;
    use crate::renderstate::BorrowedLayers;
    use crate::renderstate::TripleVertexBuffer;

    let vb = TripleVertexBuffer::new(vec![], capacity);
    let borrowed_layers = BorrowedLayers {
        layers: [vb.map_instances(), vb.map_instances(), vb.map_instances()],
    };
    let mut dest = TripleLayerQuadAllocator::Gpu(borrowed_layers);
    heap.apply_to(&mut dest).unwrap();

    let TripleLayerQuadAllocator::Gpu(borrowed_layers) = dest else {
        panic!("expected Gpu variant");
    };
    let [layer0, _layer1, _layer2] = borrowed_layers.layers;
    layer0.into_instances()
}

/// Test that reemitting the same row quads yields the same signature.
/// This pins that retained-row reuse is byte-stable so task #450 still skips truly-unchanged frames.
#[cfg(test)]
#[test]
fn test_reemitting_same_row_quads_yields_same_signature() {
    use crate::quad::{HeapQuadAllocator, TripleLayerQuadAllocatorTrait};
    let mut heap1 = HeapQuadAllocator::default();
    for i in 0..3 {
        let mut q = QuadInstance::default();
        q.position[0] = i as f32;
        heap1.extend_with_instance(0, q);
    }

    let mut heap2 = HeapQuadAllocator::default();
    for i in 0..3 {
        let mut q = QuadInstance::default();
        q.position[0] = i as f32;
        heap2.extend_with_instance(0, q);
    }

    let layer_instances1 = [applied_layer0_instances(&heap1, 10)];
    let layer_instances2 = [applied_layer0_instances(&heap2, 10)];
    // Sanity check the fixture actually captured non-empty data -- if this
    // ever fails, the test below would otherwise be vacuously comparing two
    // empty Vecs and always pass regardless of whether reuse is byte-stable.
    assert_eq!(layer_instances1[0].len(), 3);
    assert_eq!(layer_instances2[0].len(), 3);

    // Compute signatures for both using the real production function.
    let hsb = [0.0, 0.0, 0.0];
    let projection = [[0.0; 4]; 4];
    let refs1: Vec<&[QuadInstance]> = layer_instances1.iter().map(|v| v.as_slice()).collect();
    let refs2: Vec<&[QuadInstance]> = layer_instances2.iter().map(|v| v.as_slice()).collect();
    let sig1 = compute_frame_signature_from_parts(100, 100, &hsb, &projection, &refs1);
    let sig2 = compute_frame_signature_from_parts(100, 100, &hsb, &projection, &refs2);

    // Both must produce identical hashes.
    assert_eq!(
        sig1, sig2,
        "reemitting same quads must yield same signature"
    );
}

/// Test that emission order affects the signature.
/// This documents/pins WHY the sweep's row-order emission invariant matters:
/// if two frames emit the same underlying quads in a different order, the byte
/// sequence TripleVertexBuffer::instances accumulates changes, changing the
/// content hash for what's actually a pixel-identical frame -- silently defeating
/// task #450's frame-skipping optimization. Production code maintains row order
/// (see render/pane.rs::render_lines, which processes slots in order 0..n_rows
/// regardless of RowAction) -- this test just pins that the hash function itself
/// is order-sensitive, so that invariant is actually load-bearing.
#[cfg(test)]
#[test]
fn test_emission_order_affects_the_signature() {
    use crate::quad::{HeapQuadAllocator, TripleLayerQuadAllocatorTrait};

    // Three distinct quads (distinguishable by position[0]).
    let make_quad = |i: usize| {
        let mut q = QuadInstance::default();
        q.position[0] = i as f32;
        q
    };

    let mut heap_in_order = HeapQuadAllocator::default();
    for i in 0..3 {
        heap_in_order.extend_with_instance(0, make_quad(i));
    }

    let mut heap_reversed = HeapQuadAllocator::default();
    for i in (0..3).rev() {
        heap_reversed.extend_with_instance(0, make_quad(i));
    }

    let layer_instances1 = [applied_layer0_instances(&heap_in_order, 10)];
    let layer_instances2 = [applied_layer0_instances(&heap_reversed, 10)];
    assert_eq!(layer_instances1[0].len(), 3);
    assert_eq!(layer_instances2[0].len(), 3);
    assert_ne!(
        bytemuck::cast_slice::<QuadInstance, u8>(&layer_instances1[0]),
        bytemuck::cast_slice::<QuadInstance, u8>(&layer_instances2[0]),
        "fixture bug: the two orderings must actually differ"
    );

    let hsb = [0.0, 0.0, 0.0];
    let projection = [[0.0; 4]; 4];
    let refs1: Vec<&[QuadInstance]> = layer_instances1.iter().map(|v| v.as_slice()).collect();
    let refs2: Vec<&[QuadInstance]> = layer_instances2.iter().map(|v| v.as_slice()).collect();
    let sig1 = compute_frame_signature_from_parts(100, 100, &hsb, &projection, &refs1);
    let sig2 = compute_frame_signature_from_parts(100, 100, &hsb, &projection, &refs2);

    assert_ne!(
        sig1, sig2,
        "the same quads emitted in a different order must produce a different signature"
    );
}

impl crate::TermWindow {
    pub fn call_draw(&mut self, frame: &mut RenderFrame) -> anyhow::Result<()> {
        match frame {
            RenderFrame::WebGpu => self.call_draw_webgpu(),
        }
    }

    fn call_draw_webgpu(&mut self) -> anyhow::Result<()> {
        // Check render-thread back-pressure BEFORE building the frame.
        // build_webgpu_frame writes instance data to persistent GPU buffers
        // via Queue::write_buffer; if a previous frame is still being
        // submitted on the render thread, those writes can interleave with
        // the in-flight frame's reads on the same buffer. Bailing out here
        // (before any buffer write) eliminates the race entirely.
        if let Some(rt) = self.render_thread.as_ref() {
            if rt.is_in_flight() {
                rt.set_repaint_pending();
                metrics::counter!("gui.render_thread.frames_dropped").increment(1);
                // The render thread can finish (and check repaint_pending)
                // in the gap between the is_in_flight() check above and the
                // set_repaint_pending() call just now, observing it as not
                // yet set and skipping its own invalidate(). Check again: if
                // it's no longer in flight, we may have lost that wakeup, so
                // request a repaint ourselves rather than leaving this
                // frame's content stuck on screen until an unrelated event
                // happens to invalidate the window.
                if !rt.is_in_flight() {
                    if let Some(win) = self.window.as_ref() {
                        win.invalidate();
                    }
                }
                return Ok(());
            }
        }

        // Borrow (not clone) each sub-layer's QuadInstance data to feed the
        // signature hasher. `build_webgpu_frame` only *reads* `vb.instances`
        // (via `write_instances_to_gpu`, itself just `instances.borrow()`)
        // and rotates `vb.index`/`vb.bufs` -- it never mutates or clears
        // `vb.instances`. Clearing only happens via `clear_quad_allocation`
        // at the start of the *next* frame's paint (see `render/paint.rs`),
        // so it's safe to hold these borrows across the `build_webgpu_frame`
        // call below and hash straight from them afterwards, entirely
        // avoiding a per-frame clone of every QuadInstance.
        //
        // Borrow-checker note: `render_state.layers` holds `Rc<RenderLayer>`,
        // but `render_state` itself is reached through `&self`, so a
        // `Ref<Vec<QuadInstance>>` borrowed from it would normally be tied
        // to that `&self` borrow and couldn't survive the later
        // `self.build_webgpu_frame()` call (which needs `&mut self`).
        // Cloning the `Rc<RenderLayer>` handles first (a cheap refcount
        // bump, not a data copy) breaks that dependency on `&self`, so the
        // `Ref` guards taken from them afterwards are free to outlive it.
        let layers: Vec<std::rc::Rc<crate::renderstate::RenderLayer>> = {
            let render_state = self.render_state.as_ref().unwrap();
            render_state.layers.borrow().iter().cloned().collect()
        };
        // Keep the outer `Ref<[TripleVertexBuffer; 3]>` guards alive
        // alongside the inner `Ref<Vec<QuadInstance>>` guards they were
        // borrowed from -- `vb.instances.borrow()` borrows a `RefCell`
        // nested inside a slot of the array, so the array's own borrow
        // guard (`vbs_guards`) must outlive it too.
        let vbs_guards: Vec<std::cell::Ref<[crate::renderstate::TripleVertexBuffer; 3]>> =
            layers.iter().map(|layer| layer.vb.borrow()).collect();
        let mut instance_guards: Vec<std::cell::Ref<Vec<QuadInstance>>> = Vec::new();
        for vbs in &vbs_guards {
            for vb in vbs.iter() {
                instance_guards.push(vb.instances.borrow());
            }
        }
        let layer_instances: Vec<&[QuadInstance]> =
            instance_guards.iter().map(|g| g.as_slice()).collect();

        let frame = self.build_webgpu_frame()?;

        // Compute the signature for this frame from the borrows taken above.
        let signature = self.compute_frame_signature(&frame, &layer_instances);
        drop(instance_guards);
        drop(vbs_guards);

        // Compare with the previous frame signature and skip if identical
        if let Some(last_signature) = self.last_frame_signature {
            if signature == last_signature {
                metrics::counter!("gui.frame.skipped").increment(1);
                return Ok(());
            }
        }

        // New frame or signature differs - submit it
        self.last_frame_signature = Some(signature);
        metrics::counter!("gui.frame.submitted").increment(1);

        if let Some(rt) = self.render_thread.as_ref() {
            rt.send_frame(frame);
        } else {
            self.webgpu.as_ref().unwrap().submit_frame(frame)?;
        }
        Ok(())
    }

    /// Computes a content signature for a GpuFrame to detect identical consecutive frames.
    /// The signature includes:
    /// - All layer QuadInstance data (position/color/tex/hsv/flags)
    /// - uniform.projection (changes on window resize)
    /// - uniform.foreground_text_hsb (changes on config reload)
    /// - Window/surface dimensions
    ///
    /// Explicitly excludes uniform.milliseconds because it increases every frame and
    /// would prevent any frame from being judged identical. The shader never reads
    /// milliseconds (verified by grep in shader.wgsl), so excluding it cannot cause
    /// stale-pixel bugs.
    fn compute_frame_signature(
        &self,
        frame: &GpuFrame,
        layer_instances: &[&[QuadInstance]],
    ) -> u64 {
        compute_frame_signature_from_parts(
            self.dimensions.pixel_width,
            self.dimensions.pixel_height,
            &frame.uniform.foreground_text_hsb,
            &frame.uniform.projection,
            layer_instances,
        )
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

                    draws.push(GpuDraw {
                        vertex_buffer: instance_buffer,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quad::QuadInstance;

    #[test]
    fn test_signature_identical_frames() {
        // Create two identical sets of layer instances, dimensions, and uniform data
        let instances1: Vec<QuadInstance> = vec![QuadInstance {
            position: [0.0, 0.0, 10.0, 10.0],
            fg_color: [1.0, 0.0, 0.0, 1.0],
            alt_color: [0.0, 1.0, 0.0, 1.0],
            tex: [0.0, 1.0, 0.0, 1.0],
            hsv: [0.0, 0.0, 1.0],
            has_color: 0.0,
            mix_value: 0.5,
        }];

        let instances2 = instances1.clone();

        let foreground_text_hsb = [0.5, 0.5, 0.5];
        let projection = [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ];

        // Call the real signature computation function twice with identical arguments
        // This test now validates that the function uses a fixed/deterministic hasher seed,
        // so identical frames across separate calls produce identical signatures.
        let sig1 = compute_frame_signature_from_parts(
            1920,
            1080,
            &foreground_text_hsb,
            &projection,
            &[instances1.as_slice()],
        );
        let sig2 = compute_frame_signature_from_parts(
            1920,
            1080,
            &foreground_text_hsb,
            &projection,
            &[instances2.as_slice()],
        );

        assert_eq!(
            sig1, sig2,
            "Identical frame data should produce identical signatures across separate calls"
        );
    }

    #[test]
    fn test_signature_differs_when_instances_differ() {
        let instances1: Vec<QuadInstance> = vec![QuadInstance {
            position: [0.0, 0.0, 10.0, 10.0],
            fg_color: [1.0, 0.0, 0.0, 1.0],
            alt_color: [0.0, 1.0, 0.0, 1.0],
            tex: [0.0, 1.0, 0.0, 1.0],
            hsv: [0.0, 0.0, 1.0],
            has_color: 0.0,
            mix_value: 0.5,
        }];

        let mut instances2 = instances1.clone();
        // Change one pixel of position - this should produce a different signature
        instances2[0].position[0] = 1.0; // Was 0.0, now 1.0

        let foreground_text_hsb = [0.5, 0.5, 0.5];
        let projection = [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ];

        let sig1 = compute_frame_signature_from_parts(
            1920,
            1080,
            &foreground_text_hsb,
            &projection,
            &[instances1.as_slice()],
        );
        let sig2 = compute_frame_signature_from_parts(
            1920,
            1080,
            &foreground_text_hsb,
            &projection,
            &[instances2.as_slice()],
        );

        assert_ne!(
            sig1, sig2,
            "Differing instance data should produce different signatures"
        );
    }

    #[test]
    fn test_signature_includes_projection() {
        let instances: Vec<QuadInstance> = vec![];
        let foreground_text_hsb = [0.5, 0.5, 0.5];

        let projection1 = [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        let projection2 = [
            [2.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ]; // Different

        let sig1 = compute_frame_signature_from_parts(
            1920,
            1080,
            &foreground_text_hsb,
            &projection1,
            &[instances.as_slice()],
        );
        let sig2 = compute_frame_signature_from_parts(
            1920,
            1080,
            &foreground_text_hsb,
            &projection2,
            &[instances.as_slice()],
        );

        assert_ne!(
            sig1, sig2,
            "Different projection matrices should produce different signatures"
        );
    }

    #[test]
    fn test_signature_includes_foreground_text_hsb() {
        let instances: Vec<QuadInstance> = vec![];
        let projection = [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ];

        let hsb1 = [0.5, 0.5, 0.5];
        let hsb2 = [0.6, 0.5, 0.5]; // Different hue

        let sig1 = compute_frame_signature_from_parts(
            1920,
            1080,
            &hsb1,
            &projection,
            &[instances.as_slice()],
        );
        let sig2 = compute_frame_signature_from_parts(
            1920,
            1080,
            &hsb2,
            &projection,
            &[instances.as_slice()],
        );

        assert_ne!(
            sig1, sig2,
            "Different HSB values should produce different signatures"
        );
    }

    #[test]
    fn test_signature_includes_dimensions() {
        let instances: Vec<QuadInstance> = vec![];
        let foreground_text_hsb = [0.5, 0.5, 0.5];
        let projection = [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ];

        // Same content, different dimensions - should produce different signatures
        // because a resize with no content change still needs a real redraw
        let sig1 = compute_frame_signature_from_parts(
            1920,
            1080,
            &foreground_text_hsb,
            &projection,
            &[instances.as_slice()],
        );
        let sig2 = compute_frame_signature_from_parts(
            2560,
            1440,
            &foreground_text_hsb,
            &projection,
            &[instances.as_slice()],
        );

        assert_ne!(
            sig1, sig2,
            "Different dimensions should produce different signatures (resize must force redraw)"
        );
    }

    /// This test confirms that `milliseconds` cannot affect the signature at the type level.
    ///
    /// The real `TermWindow::compute_frame_signature` method extracts `frame.uniform.foreground_text_hsb`,
    /// `frame.uniform.projection`, and `self.dimensions`, then delegates to `compute_frame_signature_from_parts`.
    /// Critically, `compute_frame_signature_from_parts` does NOT accept a `milliseconds` parameter at all.
    ///
    /// This type-level guarantee means:
    /// 1. The milliseconds field from `ShaderUniform` is never passed into the signature computation.
    /// 2. Even if someone edits the wrapper method to accidentally include it, they would need to change
    ///    `compute_frame_signature_from_parts`'s signature, which is a more obvious breakage.
    /// 3. The shader never reads milliseconds (verified by `grep -n "milliseconds" crates/wezterm-gui/src/shader.wgsl`),
    ///    so excluding it from the signature is correct and cannot cause stale-pixel bugs.
    ///
    /// This is the most important property of task #450: if milliseconds were included, every frame would
    /// hash differently and nothing would ever be skipped, defeating the entire purpose.
    #[test]
    fn test_milliseconds_excluded_from_signature_type_level() {
        // The signature function's parameters are its only inputs - no `milliseconds` parameter exists.
        // This is a compile-time proof that milliseconds cannot affect the result.
        //
        // Parameters (and their meanings):
        // - pixel_width: usize - window width (resize detection)
        // - pixel_height: usize - window height (resize detection)
        // - foreground_text_hsb: &[f32; 3] - config foreground color transform
        // - projection: &[[f32; 4]; 4] - projection matrix (resize detection)
        // - layer_instances: &[&[QuadInstance]] - actual renderable content (borrowed, not owned)
        //
        // Notably absent: a `milliseconds` parameter of any type.
        //
        // The wrapper `TermWindow::compute_frame_signature(&self, frame: &GpuFrame, layer_instances: &[&[QuadInstance]])`
        // extracts the above fields from `self.dimensions`, `frame.uniform.foreground_text_hsb`, and `frame.uniform.projection`,
        // but never touches `frame.uniform.milliseconds`.
        //
        // This test documents that design choice. If future code incorrectly adds milliseconds to the signature,
        // it would need to modify `compute_frame_signature_from_parts`'s signature, which is an obvious red flag.
        let _ = compute_frame_signature_from_parts
            as fn(usize, usize, &[f32; 3], &[[f32; 4]; 4], &[&[QuadInstance]]) -> u64;

        // If milliseconds were included, the function signature would need a parameter like:
        // - milliseconds: u32, or
        // - uniform: &ShaderUniform (which includes milliseconds)
        //
        // Neither exists, proving exclusion at the type level.
    }
}
