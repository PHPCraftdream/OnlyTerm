use super::glyphcache::GlyphCache;
use super::quad::*;
use super::utilsprites::{RenderMetrics, UtilSprites};
use crate::termwindow::webgpu::{adapter_info_to_gpu_info, WebGpuState, WebGpuTexture};
use ::window::bitmaps::atlas::OutOfTextureSpace;
use ::window::bitmaps::Texture2d;
use anyhow::Context;
use std::cell::{Cell, RefCell, RefMut};
use std::rc::Rc;
use std::sync::Arc;
use wezterm_font::FontConfiguration;
use wgpu::util::DeviceExt;

#[derive(Clone)]
pub struct RenderContext(pub Arc<WebGpuState>);

pub enum RenderFrame {
    WebGpu,
}

impl RenderContext {
    pub fn allocate_index_buffer(&self, indices: &[u32]) -> anyhow::Result<IndexBuffer> {
        Ok(IndexBuffer(WebGpuIndexBuffer::new(indices, &self.0)))
    }

    pub fn allocate_vertex_buffer_initializer(
        &self,
        _num_quads: usize,
    ) -> Vec<crate::quad::QuadInstance> {
        vec![]
    }

    pub fn allocate_vertex_buffer(
        &self,
        num_quads: usize,
        _initializer: &[crate::quad::QuadInstance],
    ) -> anyhow::Result<VertexBuffer> {
        Ok(VertexBuffer(WebGpuInstanceBuffer::new(num_quads, &self.0)))
    }

    pub fn allocate_texture_atlas(&self, size: usize) -> anyhow::Result<Rc<dyn Texture2d>> {
        let texture: Rc<dyn Texture2d> =
            Rc::new(WebGpuTexture::new(size as u32, size as u32, &self.0)?);
        Ok(texture)
    }

    pub fn renderer_info(&self) -> String {
        let info = adapter_info_to_gpu_info(self.0.adapter_info().clone());
        format!("WebGPU: {info}")
    }
}

pub struct IndexBuffer(WebGpuIndexBuffer);

impl IndexBuffer {
    pub fn webgpu(&self) -> &WebGpuIndexBuffer {
        &self.0
    }
}

pub struct VertexBuffer(WebGpuInstanceBuffer);

impl VertexBuffer {
    pub fn webgpu(&self) -> &WebGpuInstanceBuffer {
        &self.0
    }
    pub fn webgpu_mut(&mut self) -> &mut WebGpuInstanceBuffer {
        &mut self.0
    }
}

/// A safe (no `unsafe`, no lifetime erasure) replacement for the old
/// self-referential `MappedQuads`. This is only ever used as a `&mut`
/// borrow handed to a caller-supplied closure (see
/// `RenderLayer::with_quad_allocator`) -- it is never returned as an
/// owned value, so its lifetime is an ordinary borrow tied to whatever
/// RefCell guards the caller is holding in its own stack frame, and the
/// borrow checker verifies it exactly like any other nested borrow.
#[allow(dead_code)] // Kept for public API surface; may be revived in future
pub struct WebGpuVertexBuffer {
    buf: wgpu::Buffer,
    num_vertices: usize,
    state: Arc<WebGpuState>,
}

impl std::ops::Deref for WebGpuVertexBuffer {
    type Target = wgpu::Buffer;
    fn deref(&self) -> &Self::Target {
        &self.buf
    }
}

#[allow(dead_code)] // Kept for public API surface; may be revived in future
impl WebGpuVertexBuffer {
    pub fn new(num_vertices: usize, state: &Arc<WebGpuState>) -> Self {
        Self {
            buf: state.device().create_buffer(&wgpu::BufferDescriptor {
                label: Some("Vertex Buffer"),
                size: (num_vertices * std::mem::size_of::<Vertex>()) as wgpu::BufferAddress,
                usage: wgpu::BufferUsages::VERTEX,
                mapped_at_creation: true,
            }),
            num_vertices,
            state: Arc::clone(state),
        }
    }

    pub fn map(&self) -> wgpu::BufferViewMut<'_> {
        // `get_mapped_range_mut`'s returned `BufferViewMut` carries its own
        // internal copy of the slice descriptor (see wgpu's
        // `BufferSlice::get_mapped_range_mut`), so there's no need to also
        // keep the `BufferSlice` temporary around as a sibling field.
        self.buf.slice(..).get_mapped_range_mut()
    }

    pub fn recreate(&mut self) -> wgpu::Buffer {
        let mut new_buf = self.state.device().create_buffer(&wgpu::BufferDescriptor {
            label: Some("Vertex Buffer"),
            size: (self.num_vertices * std::mem::size_of::<Vertex>()) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX,
            mapped_at_creation: true,
        });
        std::mem::swap(&mut new_buf, &mut self.buf);
        new_buf
    }
}

/// Instance-mode vertex buffer for instanced rendering.
/// Uses persistent buffer with Queue::write_buffer instead of per-frame recreation.
pub struct WebGpuInstanceBuffer {
    buf: wgpu::Buffer,
    capacity: usize,
    state: Arc<WebGpuState>,
    used_instances: usize,
}

impl WebGpuInstanceBuffer {
    pub fn new(capacity: usize, state: &Arc<WebGpuState>) -> Self {
        let buf = state.device().create_buffer(&wgpu::BufferDescriptor {
            label: Some("Instance Buffer"),
            size: (capacity * std::mem::size_of::<crate::quad::QuadInstance>())
                as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            buf,
            capacity,
            state: Arc::clone(state),
            used_instances: 0,
        }
    }

    /// Ensure capacity is at least `new_capacity`. Reallocates buffer if needed.
    pub fn ensure_capacity(&mut self, new_capacity: usize) {
        if new_capacity <= self.capacity {
            return;
        }
        // Round up to next multiple of 128
        let new_capacity = (new_capacity + 127) & !127;
        let new_buf = self.state.device().create_buffer(&wgpu::BufferDescriptor {
            label: Some("Instance Buffer (resized)"),
            size: (new_capacity * std::mem::size_of::<crate::quad::QuadInstance>())
                as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.buf = new_buf;
        self.capacity = new_capacity;
    }

    /// Write instance data to the buffer using Queue::write_buffer.
    /// Only writes the actually-used portion, not the full capacity.
    pub fn write_instances(&mut self, instances: &[crate::quad::QuadInstance]) {
        self.ensure_capacity(instances.len());
        self.state
            .queue()
            .write_buffer(&self.buf, 0, bytemuck::cast_slice(instances));
        self.used_instances = instances.len();
    }

    pub fn used_instances(&self) -> usize {
        self.used_instances
    }

    pub fn buffer(&self) -> &wgpu::Buffer {
        &self.buf
    }
}

pub struct WebGpuIndexBuffer {
    buf: wgpu::Buffer,
}

impl std::ops::Deref for WebGpuIndexBuffer {
    type Target = wgpu::Buffer;
    fn deref(&self) -> &Self::Target {
        &self.buf
    }
}

impl WebGpuIndexBuffer {
    pub fn new(indices: &[u32], state: &WebGpuState) -> Self {
        Self {
            buf: state
                .device()
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Index Buffer"),
                    usage: wgpu::BufferUsages::INDEX,
                    contents: bytemuck::cast_slice(indices),
                }),
        }
    }
}

pub struct MappedQuadsView {
    instances: Vec<crate::quad::QuadInstance>,
    next: Cell<usize>,
    capacity: usize,
}

impl MappedQuadsView {
    pub fn instances(&mut self) -> &mut Vec<crate::quad::QuadInstance> {
        &mut self.instances
    }

    /// Consumes the view and returns everything collected into it, for a
    /// caller that drives a `MappedQuadsView` directly (rather than through
    /// `with_quad_allocator`, which does this merge-back itself) to hand off
    /// to `TripleVertexBuffer::accumulate_instances`.
    pub fn into_instances(self) -> Vec<crate::quad::QuadInstance> {
        self.instances
    }
}

impl QuadAllocator for MappedQuadsView {
    fn allocate<'b>(&'b mut self) -> anyhow::Result<QuadImpl<'b>> {
        let idx = self.next.get();
        self.next.set(idx + 1);
        let _idx = if idx >= self.capacity {
            // We don't have enough quads, so we'll keep re-using
            // the first quad until we reach the end of the render
            // pass, at which point we'll detect this condition
            // and re-allocate the quads.
            0
        } else {
            idx
        };

        self.instances.push(crate::quad::QuadInstance::default());
        Ok(QuadImpl::Boxed(self.instances.last_mut().unwrap()))
    }

    fn extend_with(&mut self, vertices: &[Vertex]) {
        // Legacy path: expand vertices to instances
        let idx = self.next.get();
        let len = vertices.len();

        // idx and next are number of quads, so divide by number of vertices
        let num_quads = len / VERTICES_PER_CELL;
        self.next.set(idx + num_quads);

        if num_quads == 0 {
            return;
        }

        let start_quad_idx = self.instances.len();
        self.instances.resize(
            start_quad_idx + num_quads,
            crate::quad::QuadInstance::default(),
        );

        // SAFETY: `vertices` is a `&[Vertex]` whose length is a multiple of
        // `VERTICES_PER_CELL` (asserted below); reinterpreting it as a slice
        // of `[Vertex; VERTICES_PER_CELL]` chunks is layout-compatible since
        // both sides are the same repr and alignment.
        assert_eq!(vertices.len() % VERTICES_PER_CELL, 0);
        // SAFETY: `vertices` is a `&[Vertex]` whose length is a multiple of `VERTICES_PER_CELL`
        // (asserted above); reinterpreting it as a slice of `[Vertex; VERTICES_PER_CELL]` chunks
        // is layout-compatible since both sides are the same repr and alignment.
        let src_quads: &[[Vertex; VERTICES_PER_CELL]] = unsafe {
            std::slice::from_raw_parts(vertices.as_ptr().cast(), vertices.len() / VERTICES_PER_CELL)
        };

        for (i, quad) in src_quads.iter().enumerate() {
            let instance = &mut self.instances[start_quad_idx + i];
            // Extract instance data from the 4 vertices (all should be identical for per-quad data)
            let tex_top_left = quad[V_TOP_LEFT].tex;
            let tex_bot_right = quad[V_BOT_RIGHT].tex;
            let position_top_left = quad[V_TOP_LEFT].position;
            let position_bot_right = quad[V_BOT_RIGHT].position;

            instance.tex = [
                tex_top_left[0],
                tex_bot_right[0],
                tex_top_left[1],
                tex_bot_right[1],
            ];
            instance.position = [
                position_top_left[0],
                position_top_left[1],
                position_bot_right[0],
                position_bot_right[1],
            ];
            instance.has_color = quad[V_TOP_LEFT].has_color;
            instance.alt_color = quad[V_TOP_LEFT].alt_color;
            instance.fg_color = quad[V_TOP_LEFT].fg_color;
            instance.hsv = quad[V_TOP_LEFT].hsv;
            instance.mix_value = quad[V_TOP_LEFT].mix_value;
        }
    }

    fn extend_with_instance(&mut self, instance: crate::quad::QuadInstance) {
        let idx = self.next.get();
        if idx >= self.capacity {
            return; // Out of space, skip
        }
        self.next.set(idx + 1);
        self.instances.push(instance);
    }
}

pub struct TripleVertexBuffer {
    pub index: Cell<usize>,
    pub bufs: RefCell<Vec<VertexBuffer>>,
    pub capacity: usize,
    /// Instances collected for this layer during the current frame.
    /// `with_quad_allocator` can be called more than once per frame against
    /// the same `RenderLayer` (the main content pass in `render/paint.rs`
    /// and the UI-chrome pass in `box_model.rs` both write into it), so this
    /// accumulates across calls rather than being overwritten by each one;
    /// `clear_quad_allocation` empties it at the start of a new frame, and
    /// `write_instances_to_gpu` uploads whatever has accumulated by the time
    /// the frame is actually submitted (`render/draw.rs`).
    pub instances: RefCell<Vec<crate::quad::QuadInstance>>,
    /// Pool of scratch `Vec<QuadInstance>` buffers for reuse by `map_instances`.
    /// Each call gets its own exclusively-owned Vec from the pool (or a fresh
    /// allocation if the pool is empty), and the Vec is returned to the pool
    /// after `accumulate_instances` merges its contents. This handles reentrancy:
    /// nested `with_quad_allocator` calls on the same RenderLayer pop different
    /// Vecs, so they never collide.
    scratch_pool: RefCell<Vec<Vec<crate::quad::QuadInstance>>>,
}

impl TripleVertexBuffer {
    pub fn new(bufs: Vec<VertexBuffer>, capacity: usize) -> Self {
        Self {
            index: Cell::new(0),
            bufs: RefCell::new(bufs),
            capacity,
            instances: RefCell::new(Vec::new()),
            scratch_pool: RefCell::new(Vec::new()),
        }
    }

    pub fn clear_quad_allocation(&self) {
        self.instances.borrow_mut().clear();
    }

    pub fn need_more_quads(&self) -> Option<usize> {
        let next = self.instances.borrow().len();
        if next > self.capacity {
            Some(next)
        } else {
            None
        }
    }

    pub fn instance_count(&self) -> usize {
        self.instances.borrow().len()
    }

    /// Creates an instance-based view for allocation. The view collects into
    /// its own fresh, owned `Vec` (not a live view into any pre-existing GPU
    /// or accumulator state), so its internal bump-allocator counter starts
    /// at 0 regardless of what's already accumulated in `self.instances`
    /// from an earlier `with_quad_allocator` call this frame; the caller
    /// (`with_quad_allocator`) merges the view's collected instances into
    /// `self.instances` once painting for that call is done.
    pub fn map_instances(&self) -> MappedQuadsView {
        // Pop a Vec from the pool, or allocate fresh if empty
        let instances = self
            .scratch_pool
            .borrow_mut()
            .pop()
            .unwrap_or_else(|| Vec::with_capacity(self.capacity));

        MappedQuadsView {
            instances,
            next: Cell::new(0),
            capacity: self.capacity,
        }
    }

    /// Merges instances collected by one `with_quad_allocator` call (i.e.
    /// one `MappedQuadsView`'s worth) into this frame's accumulator.
    /// Returns the scratch Vec to the pool for reuse (preserving capacity).
    pub fn accumulate_instances(&self, mut instances: Vec<crate::quad::QuadInstance>) {
        self.instances.borrow_mut().extend(&instances);
        // Clear the Vec (preserving capacity) and return to pool for reuse
        instances.clear();
        self.scratch_pool.borrow_mut().push(instances);
    }

    /// Uploads everything accumulated so far this frame to the GPU instance
    /// buffer and returns the buffer plus the instance count for draw
    /// submission.
    pub fn write_instances_to_gpu(&self) -> (wgpu::Buffer, u32) {
        let instances = self.instances.borrow();
        let mut bufs = self.bufs.borrow_mut();
        let instance_buffer = &mut bufs[self.index.get()].0;
        instance_buffer.write_instances(&instances);
        let buffer = wgpu::Buffer::clone(instance_buffer.buffer());
        (buffer, instance_buffer.used_instances() as u32)
    }

    /// Borrows the currently-active vertex buffer. `RefMut::map` is a
    /// safe std API; the old version of this only needed `unsafe` because
    /// it additionally erased the lifetime to `'static` so the guard
    /// could be stored in a self-referential struct. Callers now just
    /// hold the guard for as long as they need it, like any other borrow.
    pub fn current_vb_mut(&self) -> RefMut<'_, VertexBuffer> {
        let index = self.index.get();
        RefMut::map(self.bufs.borrow_mut(), |bufs| &mut bufs[index])
    }

    /// Rotates to the next of `bufs.len()` slots. `bufs` holds a single
    /// buffer -- `write_instances` writes to it each frame,
    /// so a second/third rotation slot would never hold
    /// a buffer the GPU has actually seen before and would just be wasted
    /// resident memory. With one slot, this is a no-op: index stays 0.
    pub fn next_index(&self) {
        let len = self.bufs.borrow().len();
        let mut index = self.index.get();
        index += 1;
        if index >= len {
            index = 0;
        }
        self.index.set(index);
    }
}

pub struct RenderLayer {
    pub vb: RefCell<[TripleVertexBuffer; 3]>,
    context: RenderContext,
    zindex: i8,
}

impl RenderLayer {
    pub fn new(context: &RenderContext, num_quads: usize, zindex: i8) -> anyhow::Result<Self> {
        let vb = [
            Self::compute_vertices(context, 32)?,
            Self::compute_vertices(context, num_quads)?,
            Self::compute_vertices(context, 32)?,
        ];

        Ok(Self {
            context: context.clone(),
            vb: RefCell::new(vb),
            zindex,
        })
    }

    pub fn clear_quad_allocation(&self) {
        for vb in self.vb.borrow().iter() {
            vb.clear_quad_allocation();
        }
    }

    /// Maps the three per-layer vertex buffers and hands the resulting
    /// quad allocator to `f`. This replaces the old `quad_allocator()`,
    /// which returned an owned, `unsafe`-erased-to-`'static` value; here
    /// the `Ref`/`RefMut` guards and the views derived from them all live
    /// as ordinary local variables in this one function's stack frame,
    /// for exactly as long as `f` runs, so the borrow checker verifies
    /// the whole thing without any transmutes.
    pub fn with_quad_allocator<R>(&self, f: impl FnOnce(&mut TripleLayerQuadAllocator) -> R) -> R {
        let start = std::time::Instant::now();

        let vbs = self.vb.borrow();

        let view0 = vbs[0].map_instances();
        let view1 = vbs[1].map_instances();
        let view2 = vbs[2].map_instances();

        let mut layers = TripleLayerQuadAllocator::Gpu(BorrowedLayers {
            layers: [view0, view1, view2],
        });

        let result = f(&mut layers);

        metrics::histogram!("gui.paint.collect").record(start.elapsed());

        // `f` collected quads into each view's own owned `Vec` (see
        // `TripleVertexBuffer::map_instances`'s doc comment); merge them
        // into the per-layer accumulator now that painting for this call is
        // done. Without this step the collected quads are simply dropped
        // here and nothing this call painted ever reaches the GPU -- which
        // is exactly what happened before this was wired up: no panic, no
        // error, just an empty frame.
        if let TripleLayerQuadAllocator::Gpu(borrowed) = layers {
            let [view0, view1, view2] = borrowed.layers;
            vbs[0].accumulate_instances(view0.instances);
            vbs[1].accumulate_instances(view1.instances);
            vbs[2].accumulate_instances(view2.instances);
        }

        result
    }

    pub fn need_more_quads(&self, vb_idx: usize) -> Option<usize> {
        self.vb.borrow()[vb_idx].need_more_quads()
    }

    pub fn reallocate_quads(&self, idx: usize, num_quads: usize) -> anyhow::Result<()> {
        let vb = Self::compute_vertices(&self.context, num_quads)?;
        self.vb.borrow_mut()[idx] = vb;
        Ok(())
    }

    /// Compute a vertex buffer to hold the quads that comprise the visible
    /// portion of the screen.   We recreate this when the screen is resized.
    /// The idea is that we want to minimize any heavy lifting and computation
    /// and instead just poke some attributes into the offset that corresponds
    /// to a changed cell when we need to repaint the screen, and then just
    /// let the GPU figure out the rest.
    fn compute_vertices(
        context: &RenderContext,
        num_quads: usize,
    ) -> anyhow::Result<TripleVertexBuffer> {
        let verts = context.allocate_vertex_buffer_initializer(num_quads);
        log::trace!(
            "compute_vertices num_quads={}, allocated {} bytes",
            num_quads,
            verts.len() * std::mem::size_of::<Vertex>()
        );

        // `recreate()` swaps in a brand new GPU buffer every frame regardless
        // of slot (see `call_draw_webgpu`), so more than one rotation slot
        // would never actually hold a buffer the GPU has seen before --
        // rotation buys nothing here, so this gets a single slot rather than
        // keeping extra vertex-buffer memory resident for no benefit.
        let num_slots = 1;
        let mut bufs = Vec::with_capacity(num_slots);
        for _ in 0..num_slots {
            bufs.push(context.allocate_vertex_buffer(num_quads, &verts)?);
        }

        let buffer = TripleVertexBuffer::new(bufs, num_quads);

        Ok(buffer)
    }
}

pub struct BorrowedLayers {
    pub layers: [MappedQuadsView; 3],
}

impl TripleLayerQuadAllocatorTrait for BorrowedLayers {
    fn allocate(&mut self, layer_num: usize) -> anyhow::Result<QuadImpl<'_>> {
        self.layers[layer_num].allocate()
    }

    fn extend_with_instance(&mut self, layer_num: usize, instance: QuadInstance) {
        self.layers[layer_num].extend_with_instance(instance)
    }

    fn extend_from_slice(&mut self, layer_num: usize, instances: &[QuadInstance]) {
        let layer = &mut self.layers[layer_num];
        // Preserve the exact overflow-drop behavior from the per-quad loop:
        // truncate to the available capacity, then bulk-copy what fits.
        let available = layer.capacity.saturating_sub(layer.next.get());
        let n = instances.len().min(available);
        let to_copy = &instances[..n];
        layer.instances.extend_from_slice(to_copy);
        layer.next.set(layer.next.get() + to_copy.len());
    }
}

pub struct RenderState {
    pub context: RenderContext,
    pub glyph_cache: RefCell<GlyphCache>,
    pub util_sprites: UtilSprites,
    pub layers: RefCell<Vec<Rc<RenderLayer>>>,
}

impl RenderState {
    pub fn new(
        context: RenderContext,
        fonts: &Rc<FontConfiguration>,
        metrics: &RenderMetrics,
        mut atlas_size: usize,
    ) -> anyhow::Result<Self> {
        loop {
            let glyph_cache = RefCell::new(GlyphCache::new_gl(&context, fonts, atlas_size)?);
            let result = UtilSprites::new(&mut glyph_cache.borrow_mut(), metrics);
            match result {
                Ok(util_sprites) => {
                    let main_layer = Rc::new(RenderLayer::new(&context, 1024, 0)?);

                    return Ok(Self {
                        context,
                        glyph_cache,
                        util_sprites,
                        layers: RefCell::new(vec![main_layer]),
                    });
                }
                Err(OutOfTextureSpace {
                    size: Some(size), ..
                }) => {
                    atlas_size = size;
                }
                Err(OutOfTextureSpace { size: None, .. }) => {
                    anyhow::bail!("requested texture size is impossible!?")
                }
            };
        }
    }

    pub fn layer_for_zindex(&self, zindex: i8) -> anyhow::Result<Rc<RenderLayer>> {
        if let Some(layer) = self
            .layers
            .borrow()
            .iter()
            .find(|l| l.zindex == zindex)
            .map(Rc::clone)
        {
            return Ok(layer);
        }

        let layer = Rc::new(RenderLayer::new(&self.context, 128, zindex)?);
        let mut layers = self.layers.borrow_mut();
        layers.push(Rc::clone(&layer));

        // Keep the layers sorted by zindex so that they are rendered in
        // the correct order when the layers array is iterated.
        layers.sort_by_key(|a| a.zindex);

        Ok(layer)
    }

    /// Returns true if any of the layers needed more quads to be allocated,
    /// and if we successfully allocated them.
    /// Returns false if the quads were sufficient.
    /// Returns Err if we needed to allocate but failed.
    pub fn allocated_more_quads(&mut self) -> anyhow::Result<bool> {
        let mut allocated = false;

        for layer in self.layers.borrow().iter() {
            for vb_idx in 0..3 {
                if let Some(need_quads) = layer.need_more_quads(vb_idx) {
                    // Round up to next multiple of 128 that is >=
                    // the number of needed quads for this frame
                    let num_quads = (need_quads + 127) & !127;
                    layer.reallocate_quads(vb_idx, num_quads).with_context(|| {
                        format!(
                            "Failed to allocate {} quads (needed {})",
                            num_quads, need_quads,
                        )
                    })?;
                    log::trace!("Allocated {} quads (needed {})", num_quads, need_quads);
                    allocated = true;
                }
            }
        }

        Ok(allocated)
    }

    pub fn config_changed(&mut self) {
        self.glyph_cache.borrow_mut().config_changed();
    }

    pub fn recreate_texture_atlas(
        &mut self,
        fonts: &Rc<FontConfiguration>,
        metrics: &RenderMetrics,
        size: Option<usize>,
    ) -> anyhow::Result<()> {
        // We make a a couple of passes at resizing; if the user has selected a large
        // font size (or a large scaling factor) then the `size==None` case will not
        // be able to fit the initial utility glyphs and apply_scale_change won't
        // be able to deal with that error situation.  Rather than make every
        // caller know how to deal with OutOfTextureSpace we try to absorb
        // and accomodate that here.
        let mut size = size;
        let mut attempt = 10;
        loop {
            match self.recreate_texture_atlas_impl(fonts, metrics, size) {
                Ok(_) => return Ok(()),
                Err(err) => {
                    attempt -= 1;
                    if attempt == 0 {
                        return Err(err);
                    }

                    if let Some(&OutOfTextureSpace {
                        size: Some(needed_size),
                        ..
                    }) = err.downcast_ref::<OutOfTextureSpace>()
                    {
                        size.replace(needed_size);
                        continue;
                    }

                    return Err(err);
                }
            }
        }
    }

    fn recreate_texture_atlas_impl(
        &mut self,
        fonts: &Rc<FontConfiguration>,
        metrics: &RenderMetrics,
        size: Option<usize>,
    ) -> anyhow::Result<()> {
        let size = size.unwrap_or_else(|| self.glyph_cache.borrow().atlas.size());
        let mut new_glyph_cache = GlyphCache::new_gl(&self.context, fonts, size)?;
        self.util_sprites = UtilSprites::new(&mut new_glyph_cache, metrics)?;

        let mut glyph_cache = self.glyph_cache.borrow_mut();

        // Steal the decoded image cache; without this, any animating gifs
        // would reset back to frame 0 each time we filled the texture
        std::mem::swap(
            &mut glyph_cache.image_cache,
            &mut new_glyph_cache.image_cache,
        );

        *glyph_cache = new_glyph_cache;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test that the scratch pool actually reuses Vec capacity across calls.
    /// This exercises the REAL `TripleVertexBuffer::map_instances` and
    /// `TripleVertexBuffer::accumulate_instances` methods. If someone reverts
    /// the pooling fix (changes `map_instances` back to `Vec::with_capacity`
    /// on every call), this test would fail because the second call would
    /// allocate a fresh Vec (different pointer).
    #[test]
    fn test_scratch_pool_reuses_capacity() {
        // TripleVertexBuffer::new accepts an empty vec for bufs.
        // Neither map_instances nor accumulate_instances touches self.bufs,
        // so this is safe for testing the pool logic without a real wgpu device.
        let tvb = TripleVertexBuffer::new(vec![], 100);

        // First call: should allocate fresh Vec with capacity 100
        let mut view1 = tvb.map_instances();
        let ptr1 = view1.instances.as_ptr();
        let cap1 = view1.instances.capacity();
        assert_eq!(
            cap1, 100,
            "First call should get capacity matching TripleVertexBuffer.capacity"
        );

        // Push distinguishable quads
        let quad_a = crate::quad::QuadInstance {
            position: [10.0, 20.0, 30.0, 40.0],
            ..Default::default()
        };
        view1.instances.push(quad_a);
        let quad_b = crate::quad::QuadInstance {
            position: [50.0, 60.0, 70.0, 80.0],
            ..Default::default()
        };
        view1.instances.push(quad_b);

        // Accumulate: this should return the Vec to the pool (capacity preserved)
        tvb.accumulate_instances(view1.instances);

        // Second call: should REUSE the pooled Vec (same pointer and capacity)
        let view2 = tvb.map_instances();
        let ptr2 = view2.instances.as_ptr();
        let cap2 = view2.instances.capacity();
        assert_eq!(
            ptr1, ptr2,
            "Second call should reuse pooled Vec (same pointer)"
        );
        assert_eq!(
            cap1, cap2,
            "Second call should reuse pooled Vec (same capacity)"
        );

        // Assert that the first call's quads made it to the accumulator
        assert_eq!(
            tvb.instance_count(),
            2,
            "Accumulator should have 2 instances from first call"
        );

        let acc_instances = tvb.instances.borrow();
        assert_eq!(
            acc_instances[0].position,
            [10.0, 20.0, 30.0, 40.0],
            "First quad's position should match"
        );
        assert_eq!(
            acc_instances[1].position,
            [50.0, 60.0, 70.0, 80.0],
            "Second quad's position should match"
        );
    }

    /// Test that reentrant nested calls on the same TripleVertexBuffer
    /// get different buffers and don't lose data.
    ///
    /// This exercises the REAL `TripleVertexBuffer::map_instances` and
    /// `TripleVertexBuffer::accumulate_instances` methods in the exact
    /// reentrancy pattern that occurs in production (see box_model.rs:844's
    /// recursive `render_element` calls).
    ///
    /// A naive implementation that uses a single shared Vec with
    /// `std::mem::take` (or similar) would FAIL this test because the inner
    /// call would steal the outer call's in-progress Vec, discarding its data.
    /// The pooling implementation passes because each call gets its own
    /// exclusively-owned Vec from the pool.
    #[test]
    fn test_reentrant_calls_use_different_buffers() {
        let tvb = TripleVertexBuffer::new(vec![], 100);

        // Outer call: get a buffer and add distinguishable quads
        let mut outer = tvb.map_instances();
        let outer_ptr = outer.instances.as_ptr();
        let outer_cap = outer.instances.capacity();
        for i in 0..2 {
            let quad = crate::quad::QuadInstance {
                position: [
                    100.0 + i as f32,
                    200.0 + i as f32,
                    300.0 + i as f32,
                    400.0 + i as f32,
                ],
                ..Default::default()
            };
            outer.instances.push(quad);
        }

        // Inner call (while outer is still alive and un-accumulated):
        // This simulates the reentrancy from box_model.rs where a child element's
        // render_element is called while the parent's with_quad_allocator is still active.
        let mut inner = tvb.map_instances();
        let inner_ptr = inner.instances.as_ptr();
        let inner_cap = inner.instances.capacity();

        // CRITICAL: outer and inner must be DIFFERENT Vecs
        assert_ne!(
            outer_ptr, inner_ptr,
            "Outer and inner calls must use different Vecs. A naive shared-Vec implementation fails here."
        );
        assert_eq!(
            outer_cap, inner_cap,
            "Both should have the same capacity (from TripleVertexBuffer.capacity)"
        );

        // Add distinguishable quads to inner
        for i in 0..3 {
            let quad = crate::quad::QuadInstance {
                position: [
                    500.0 + i as f32,
                    600.0 + i as f32,
                    700.0 + i as f32,
                    800.0 + i as f32,
                ],
                ..Default::default()
            };
            inner.instances.push(quad);
        }

        // Accumulate inner first (this is what happens in real recursion:
        // the inner call finishes before the outer one)
        tvb.accumulate_instances(inner.instances);

        // Accumulate outer
        tvb.accumulate_instances(outer.instances);

        // Verify ALL instances from both calls survived
        assert_eq!(
            tvb.instance_count(),
            5,
            "Accumulator should have 5 instances total (2 outer + 3 inner)"
        );

        // Verify the actual data (not just count) to catch corruption bugs
        let acc_instances = tvb.instances.borrow();
        let mut found_outer = [false; 2];
        let mut found_inner = [false; 3];

        for instance in acc_instances.iter() {
            // Check for outer quads using exact position matching
            if instance.position == [100.0, 200.0, 300.0, 400.0] {
                found_outer[0] = true;
            } else if instance.position == [101.0, 201.0, 301.0, 401.0] {
                found_outer[1] = true;
            }
            // Check for inner quads using exact position matching
            else if instance.position == [500.0, 600.0, 700.0, 800.0] {
                found_inner[0] = true;
            } else if instance.position == [501.0, 601.0, 701.0, 801.0] {
                found_inner[1] = true;
            } else if instance.position == [502.0, 602.0, 702.0, 802.0] {
                found_inner[2] = true;
            }
        }

        assert!(
            found_outer.iter().all(|&x| x),
            "Not all outer quads found in accumulator"
        );
        assert!(
            found_inner.iter().all(|&x| x),
            "Not all inner quads found in accumulator"
        );
    }
}
