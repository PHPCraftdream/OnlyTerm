use super::glyphcache::GlyphCache;
use super::quad::*;
use super::utilsprites::{RenderMetrics, UtilSprites};
use ::window::bitmaps::atlas::OutOfTextureSpace;
use ::window::bitmaps::Texture2d;
use onlyterm_font::FontConfiguration;
use onlyterm_gpu_render::{adapter_info_to_gpu_info, WebGpuState, WebGpuTexture};
use std::cell::{Cell, RefCell, RefMut};
use std::rc::Rc;
use std::sync::Arc;
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
    #[allow(dead_code)] // Kept as Vec::with_capacity sizing hint, no longer a ceiling
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

    pub fn instance_count(&self) -> usize {
        self.instances.borrow().len()
    }

    /// Transfer this frame's accumulator to the wire writer. The replacement
    /// buffer is already empty and will collect the next frame's instances.
    pub(crate) fn take_instances_for_wire(
        &self,
        pool: Option<&onlyterm_gpu_render::wire::WireDrawPool>,
    ) -> Vec<crate::quad::QuadInstance> {
        let replacement = pool
            .map(onlyterm_gpu_render::wire::pool_take)
            .unwrap_or_default();
        std::mem::replace(&mut *self.instances.borrow_mut(), replacement)
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
    /// Returns a scratch Vec to the pool for reuse (preserving capacity).
    ///
    /// When the accumulator is still empty (the first call this frame, e.g.
    /// the main content pass), there is nothing to copy into: swap the
    /// scratch Vec in as the accumulator directly, and send the (empty,
    /// former-accumulator) Vec to the pool instead. This avoids copying
    /// every quad of the frame's main pass. Subsequent calls (e.g. the
    /// UI-chrome pass, or nested `with_quad_allocator` calls) find a
    /// non-empty accumulator and fall back to extend, appending in call
    /// order exactly as before.
    pub fn accumulate_instances(&self, mut instances: Vec<crate::quad::QuadInstance>) {
        let mut acc = self.instances.borrow_mut();
        if acc.is_empty() {
            std::mem::swap(&mut *acc, &mut instances);
        } else {
            acc.extend_from_slice(&instances);
        }
        drop(acc);
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
    #[allow(dead_code)] // Kept for compute_vertices calls (if needed later)
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
        layer.instances.extend_from_slice(instances);
        layer.next.set(layer.next.get() + instances.len());
    }
}

pub struct RenderState {
    pub context: RenderContext,
    pub glyph_cache: RefCell<GlyphCache>,
    pub util_sprites: UtilSprites,
    pub layers: RefCell<Vec<Rc<RenderLayer>>>,
}

/// Turns on atlas-write mirroring (`WebGpuTexture::enable_mirroring`) for a
/// freshly-created glyph cache's atlas, if `mirror` is set -- called at the
/// exact moment the atlas is known to be empty (immediately after
/// construction, before the caller does anything else with it), so a
/// `HostProcess`-backend child's mirror never misses a glyph written between
/// atlas creation and whenever mirroring would otherwise have been noticed
/// and turned on (e.g. by comparing atlas identity across frames, which is
/// too late: glyphs from the very paint pass that triggered an atlas regrow
/// are written before that comparison ever runs). A no-op if this isn't a
/// `WebGpuTexture`-backed atlas (downcast fails) or `mirror` is false.
fn enable_atlas_mirroring_if_needed(glyph_cache: &GlyphCache, mirror: bool) {
    if !mirror {
        return;
    }
    let tex = glyph_cache.atlas.texture();
    if let Some(tex) = tex.downcast_ref::<WebGpuTexture>() {
        tex.enable_mirroring();
    }
}

impl RenderState {
    pub fn new(
        context: RenderContext,
        fonts: &Rc<FontConfiguration>,
        metrics: &RenderMetrics,
        mut atlas_size: usize,
        mirror_atlas: bool,
    ) -> anyhow::Result<Self> {
        loop {
            let glyph_cache = RefCell::new(GlyphCache::new_gl(&context, fonts, atlas_size)?);
            // Strictly before `UtilSprites::new`: that is what writes
            // `white_space` (and the underline/cursor sprites) into the
            // brand-new atlas, and those writes have to be recorded too --
            // `white_space` is the texel every glyph, underline and cursor
            // quad samples, so a mirror missing it draws all of them with
            // alpha 0. `GlyphCache::new_gl` above is deliberately still
            // outside: `Atlas::new` blanks the whole texture, and there is
            // nothing to replay about a blank that the mirror already starts
            // out as.
            enable_atlas_mirroring_if_needed(&glyph_cache.borrow(), mirror_atlas);
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

    pub fn config_changed(&mut self) {
        self.glyph_cache.borrow_mut().config_changed();
    }

    pub fn recreate_texture_atlas(
        &mut self,
        fonts: &Rc<FontConfiguration>,
        metrics: &RenderMetrics,
        size: Option<usize>,
        mirror_atlas: bool,
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
            match self.recreate_texture_atlas_impl(fonts, metrics, size, mirror_atlas) {
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
        mirror_atlas: bool,
    ) -> anyhow::Result<()> {
        let size = size.unwrap_or_else(|| self.glyph_cache.borrow().atlas.size());
        let mut new_glyph_cache = GlyphCache::new_gl(&self.context, fonts, size)?;
        // Before `UtilSprites::new`, for the reason spelled out in
        // `RenderState::new`.
        enable_atlas_mirroring_if_needed(&new_glyph_cache, mirror_atlas);
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
#[path = "renderstate_wire_test.rs"]
mod wire_transfer_tests;

#[cfg(test)]
#[path = "renderstate_accumulate_test.rs"]
mod accumulate_tests;
