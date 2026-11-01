use super::glyphcache::GlyphCache;
use super::quad::*;
use super::utilsprites::{RenderMetrics, UtilSprites};
use ::window::bitmaps::atlas::OutOfTextureSpace;
use ::window::bitmaps::Texture2d;
use onlyterm_font::FontConfiguration;
use onlyterm_gpu_render::{adapter_info_to_gpu_info, CpuAtlasTexture, WebGpuState, WebGpuTexture};
use std::cell::{Cell, RefCell, RefMut};
use std::rc::Rc;
use std::sync::Arc;

#[derive(Clone)]
pub enum RenderContext {
    Hosted,
    InProcess(Arc<WebGpuState>),
}

pub enum RenderFrame {
    WebGpu,
}

impl RenderContext {
    pub fn hosted() -> Self {
        Self::Hosted
    }

    pub fn in_process(state: Arc<WebGpuState>) -> Self {
        Self::InProcess(state)
    }

    pub fn webgpu(&self) -> Option<&Arc<WebGpuState>> {
        match self {
            Self::Hosted => None,
            Self::InProcess(state) => Some(state),
        }
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
        Ok(VertexBuffer(WebGpuInstanceBuffer::new(num_quads)))
    }

    pub fn allocate_texture_atlas(&self, size: usize) -> anyhow::Result<Rc<dyn Texture2d>> {
        match self {
            Self::Hosted => Ok(Rc::new(CpuAtlasTexture::new(size, size))),
            Self::InProcess(state) => Ok(Rc::new(WebGpuTexture::new(
                size as u32,
                size as u32,
                state,
            )?)),
        }
    }

    pub fn renderer_info(&self) -> Option<String> {
        match self {
            Self::Hosted => None,
            Self::InProcess(state) => {
                let info = adapter_info_to_gpu_info(state.adapter_info().clone());
                Some(format!("WebGPU: {info}"))
            }
        }
    }
}

pub struct VertexBuffer(WebGpuInstanceBuffer);

impl VertexBuffer {
    pub fn webgpu(&self) -> &WebGpuInstanceBuffer {
        &self.0
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

fn rounded_instance_capacity(new_capacity: usize) -> usize {
    (new_capacity + 127) & !127
}

fn needs_instance_buffer_allocation(has_buffer: bool, len: usize, capacity: usize) -> bool {
    len > capacity || (len > 0 && !has_buffer)
}

/// Uses persistent buffer with Queue::write_buffer instead of per-frame recreation.
pub struct WebGpuInstanceBuffer {
    buf: Option<wgpu::Buffer>,
    capacity: usize,
    used_instances: usize,
}

impl WebGpuInstanceBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            buf: None,
            capacity,
            used_instances: 0,
        }
    }

    fn allocate(&self, device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Instance Buffer"),
            size: (capacity * std::mem::size_of::<crate::quad::QuadInstance>())
                as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    /// Ensure capacity is at least `new_capacity`. Reallocates buffer if needed.
    pub fn ensure_capacity(&mut self, device: &wgpu::Device, new_capacity: usize) {
        if new_capacity <= self.capacity {
            return;
        }
        // Round up to next multiple of 128, as before.
        let new_capacity = rounded_instance_capacity(new_capacity);
        self.buf = Some(self.allocate(device, new_capacity));
        self.capacity = new_capacity;
    }

    /// Upload instance data. GPU storage is created on the first in-process write.
    pub fn write_instances(
        &mut self,
        state: &WebGpuState,
        instances: &[crate::quad::QuadInstance],
    ) {
        if instances.is_empty() {
            self.used_instances = 0;
            return;
        }
        if needs_instance_buffer_allocation(self.buf.is_some(), instances.len(), self.capacity) {
            self.ensure_capacity(state.device(), instances.len());
            if self.buf.is_none() {
                self.buf = Some(self.allocate(state.device(), self.capacity));
            }
        }
        let buf = self.buf.as_ref().unwrap();
        state
            .queue()
            .write_buffer(buf, 0, bytemuck::cast_slice(instances));
        self.used_instances = instances.len();
    }

    pub fn used_instances(&self) -> usize {
        self.used_instances
    }

    pub fn buffer(&self) -> Option<&wgpu::Buffer> {
        self.buf.as_ref()
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
    pub fn write_instances_to_gpu(&self, state: &WebGpuState) -> (wgpu::Buffer, u32) {
        let instances = self.instances.borrow();
        let mut bufs = self.bufs.borrow_mut();
        let instance_buffer = &mut bufs[self.index.get()].0;
        instance_buffer.write_instances(state, &instances);
        let buffer = wgpu::Buffer::clone(instance_buffer.buffer().unwrap());
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
            if onlyterm_metrics::profile_pipeline_enabled() {
                let quad_count =
                    view0.instances.len() + view1.instances.len() + view2.instances.len();
                onlyterm_metrics::cached_histogram!("gui.paint.collect.quads.size")
                    .record(quad_count as f64);
            }
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
mod instance_buffer_tests {
    use super::{
        needs_instance_buffer_allocation, rounded_instance_capacity, WebGpuInstanceBuffer,
    };
    use onlyterm_gpu_render::CpuAtlasTexture;

    #[test]
    fn lazy_instance_buffer_preserves_capacity_until_first_write() {
        let buffer = WebGpuInstanceBuffer::new(13);
        assert!(!needs_instance_buffer_allocation(false, 0, 13));
        assert!(needs_instance_buffer_allocation(false, 1, 13));
        assert!(needs_instance_buffer_allocation(false, 14, 13));
        assert!(!needs_instance_buffer_allocation(true, 13, 13));
        assert_eq!(buffer.capacity, 13);
        assert!(buffer.buffer().is_none());
        assert_eq!(buffer.used_instances(), 0);
    }

    #[test]
    fn hosted_context_allocates_cpu_atlas_and_device_free_layer_slots() {
        let context = super::RenderContext::hosted();
        assert!(context.webgpu().is_none());
        let texture_before = context.allocate_texture_atlas(16).unwrap();
        let texture_after_rebuild = context.allocate_texture_atlas(16).unwrap();
        assert!(!std::rc::Rc::ptr_eq(
            &texture_before,
            &texture_after_rebuild
        ));
        assert!(texture_before.downcast_ref::<CpuAtlasTexture>().is_some());
        assert!(texture_after_rebuild
            .downcast_ref::<CpuAtlasTexture>()
            .is_some());
        assert!(context.renderer_info().is_none());
        let layer = super::RenderLayer::new(&context, 32, 0).unwrap();
        assert_eq!(layer.vb.borrow().len(), 3);
        for vb in layer.vb.borrow().iter() {
            assert!(vb
                .bufs
                .borrow()
                .iter()
                .all(|buffer| buffer.webgpu().buffer().is_none()));
        }
    }

    #[test]
    fn growth_rounding_is_unchanged() {
        assert_eq!(rounded_instance_capacity(14), 128);
        assert_eq!(rounded_instance_capacity(128), 128);
        assert_eq!(rounded_instance_capacity(129), 256);
        let buffer = WebGpuInstanceBuffer::new(13);
        assert_eq!(buffer.capacity, 13);
        assert!(buffer.buffer().is_none());
    }
}

#[cfg(test)]
#[path = "renderstate_wire_test.rs"]
mod wire_transfer_tests;

#[cfg(test)]
#[path = "renderstate_accumulate_test.rs"]
mod accumulate_tests;
