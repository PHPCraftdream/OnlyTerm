use super::glyphcache::GlyphCache;
use super::quad::*;
use super::utilsprites::{RenderMetrics, UtilSprites};
use crate::termwindow::webgpu::{adapter_info_to_gpu_info, WebGpuState, WebGpuTexture};
use ::window::bitmaps::atlas::OutOfTextureSpace;
use ::window::bitmaps::Texture2d;
use ::window::glium::backend::Context as GliumContext;
use ::window::glium::buffer::Mapping;
use ::window::glium::{
    CapabilitiesSource, IndexBuffer as GliumIndexBuffer, VertexBuffer as GliumVertexBuffer,
};
use ::window::*;
use anyhow::Context;
use std::cell::{Cell, RefCell, RefMut};
use std::convert::TryInto;
use std::rc::Rc;
use std::sync::Arc;
use wezterm_font::FontConfiguration;
use wgpu::util::DeviceExt;

const INDICES_PER_CELL: usize = 6;

#[derive(Clone)]
pub enum RenderContext {
    Glium(Rc<GliumContext>),
    WebGpu(Arc<WebGpuState>),
}

pub enum RenderFrame<'a> {
    Glium(&'a mut glium::Frame),
    WebGpu,
}

impl RenderContext {
    pub fn allocate_index_buffer(&self, indices: &[u32]) -> anyhow::Result<IndexBuffer> {
        match self {
            Self::Glium(context) => Ok(IndexBuffer::Glium(GliumIndexBuffer::new(
                context,
                glium::index::PrimitiveType::TrianglesList,
                indices,
            )?)),
            Self::WebGpu(state) => Ok(IndexBuffer::WebGpu(WebGpuIndexBuffer::new(indices, state))),
        }
    }

    pub fn allocate_vertex_buffer_initializer(&self, num_quads: usize) -> Vec<Vertex> {
        match self {
            Self::Glium(_) => {
                vec![Vertex::default(); num_quads * VERTICES_PER_CELL]
            }
            Self::WebGpu(_) => vec![],
        }
    }

    pub fn allocate_vertex_buffer(
        &self,
        num_quads: usize,
        initializer: &[Vertex],
    ) -> anyhow::Result<VertexBuffer> {
        match self {
            Self::Glium(context) => Ok(VertexBuffer::Glium(GliumVertexBuffer::dynamic(
                context,
                initializer,
            )?)),
            Self::WebGpu(state) => Ok(VertexBuffer::WebGpu(WebGpuVertexBuffer::new(
                num_quads * VERTICES_PER_CELL,
                state,
            ))),
        }
    }

    pub fn allocate_texture_atlas(&self, size: usize) -> anyhow::Result<Rc<dyn Texture2d>> {
        match self {
            Self::Glium(context) => {
                let caps = context.get_capabilities();
                // You'd hope that allocating a texture would automatically
                // include this check, but it doesn't, and instead, the texture
                // silently fails to bind when attempting to render into it later.
                // So! We check and raise here for ourselves!
                let max_texture_size: usize = caps
                    .max_texture_size
                    .try_into()
                    .context("represent Capabilities.max_texture_size as usize")?;
                if size > max_texture_size {
                    anyhow::bail!(
                        "Cannot use a texture of size {} as it is larger \
                         than the max {} supported by your GPU",
                        size,
                        caps.max_texture_size
                    );
                }
                use crate::glium::texture::SrgbTexture2d;
                let surface: Rc<dyn Texture2d> = Rc::new(SrgbTexture2d::empty_with_format(
                    context,
                    glium::texture::SrgbFormat::U8U8U8U8,
                    glium::texture::MipmapsOption::NoMipmap,
                    size as u32,
                    size as u32,
                )?);
                Ok(surface)
            }
            Self::WebGpu(state) => {
                let texture: Rc<dyn Texture2d> =
                    Rc::new(WebGpuTexture::new(size as u32, size as u32, state)?);
                Ok(texture)
            }
        }
    }

    pub fn renderer_info(&self) -> String {
        match self {
            Self::Glium(ctx) => format!(
                "OpenGL: {} {}",
                ctx.get_opengl_renderer_string(),
                ctx.get_opengl_version_string()
            ),
            Self::WebGpu(state) => {
                let info = adapter_info_to_gpu_info(state.adapter_info.clone());
                format!("WebGPU: {info}")
            }
        }
    }
}

// Dual-backend enum (OpenGL/WebGPU); the size difference is inherent to
// wrapping two buffer backends in one type, allocated once per frame.
#[allow(clippy::large_enum_variant)]
pub enum IndexBuffer {
    Glium(GliumIndexBuffer<u32>),
    WebGpu(WebGpuIndexBuffer),
}

impl IndexBuffer {
    pub fn glium(&self) -> &GliumIndexBuffer<u32> {
        match self {
            Self::Glium(g) => g,
            _ => unreachable!(),
        }
    }
    pub fn webgpu(&self) -> &WebGpuIndexBuffer {
        match self {
            Self::WebGpu(g) => g,
            _ => unreachable!(),
        }
    }
}

// Dual-backend enum (OpenGL/WebGPU); the size difference is inherent to
// wrapping two buffer backends in one type, allocated once per frame.
#[allow(clippy::large_enum_variant)]
pub enum VertexBuffer {
    Glium(GliumVertexBuffer<Vertex>),
    WebGpu(WebGpuVertexBuffer),
}

impl VertexBuffer {
    pub fn glium(&self) -> &GliumVertexBuffer<Vertex> {
        match self {
            Self::Glium(g) => g,
            _ => unreachable!(),
        }
    }
    pub fn webgpu(&self) -> &WebGpuVertexBuffer {
        match self {
            Self::WebGpu(g) => g,
            _ => unreachable!(),
        }
    }
    pub fn webgpu_mut(&mut self) -> &mut WebGpuVertexBuffer {
        match self {
            Self::WebGpu(g) => g,
            _ => unreachable!(),
        }
    }
}

enum MappedVertexBuffer<'a> {
    Glium(Mapping<'a, [Vertex]>),
    WebGpu(WebGpuMappedVertexBuffer<'a>),
}

impl<'a> MappedVertexBuffer<'a> {
    fn slice_mut(&mut self, range: std::ops::Range<usize>) -> &mut [Vertex] {
        match self {
            Self::Glium(g) => &mut g[range],
            Self::WebGpu(g) => {
                let mapping: &mut [Vertex] = bytemuck::cast_slice_mut(&mut g.mapping);
                &mut mapping[range]
            }
        }
    }
}

/// A safe (no `unsafe`, no lifetime erasure) replacement for the old
/// self-referential `MappedQuads`. This is only ever used as a `&mut`
/// borrow handed to a caller-supplied closure (see
/// `RenderLayer::with_quad_allocator`) -- it is never returned as an
/// owned value, so its lifetime is an ordinary borrow tied to whatever
/// RefCell guards the caller is holding in its own stack frame, and the
/// borrow checker verifies it exactly like any other nested borrow.
pub struct MappedQuadsView<'a> {
    mapping: MappedVertexBuffer<'a>,
    next: &'a Cell<usize>,
    capacity: usize,
}

pub struct WebGpuMappedVertexBuffer<'a> {
    mapping: wgpu::BufferViewMut<'a>,
}

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

impl WebGpuVertexBuffer {
    pub fn new(num_vertices: usize, state: &Arc<WebGpuState>) -> Self {
        Self {
            buf: state.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Vertex Buffer"),
                size: (num_vertices * std::mem::size_of::<Vertex>()) as wgpu::BufferAddress,
                usage: wgpu::BufferUsages::VERTEX,
                mapped_at_creation: true,
            }),
            num_vertices,
            state: Arc::clone(state),
        }
    }

    pub fn map(&self) -> WebGpuMappedVertexBuffer<'_> {
        // `get_mapped_range_mut`'s returned `BufferViewMut` carries its own
        // internal copy of the slice descriptor (see wgpu's
        // `BufferSlice::get_mapped_range_mut`), so there's no need to also
        // keep the `BufferSlice` temporary around as a sibling field.
        let mapping = self.buf.slice(..).get_mapped_range_mut();
        WebGpuMappedVertexBuffer { mapping }
    }

    pub fn recreate(&mut self) -> wgpu::Buffer {
        let mut new_buf = self.state.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Vertex Buffer"),
            size: (self.num_vertices * std::mem::size_of::<Vertex>()) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX,
            mapped_at_creation: true,
        });
        std::mem::swap(&mut new_buf, &mut self.buf);
        new_buf
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
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Index Buffer"),
                    usage: wgpu::BufferUsages::INDEX,
                    contents: bytemuck::cast_slice(indices),
                }),
        }
    }
}

impl<'a> QuadAllocator for MappedQuadsView<'a> {
    fn allocate<'b>(&'b mut self) -> anyhow::Result<QuadImpl<'b>> {
        let idx = self.next.get();
        self.next.set(idx + 1);
        let idx = if idx >= self.capacity {
            // We don't have enough quads, so we'll keep re-using
            // the first quad until we reach the end of the render
            // pass, at which point we'll detect this condition
            // and re-allocate the quads.
            0
        } else {
            idx
        };

        let idx = idx * VERTICES_PER_CELL;
        let mut quad = Quad {
            vert: self.mapping.slice_mut(idx..idx + VERTICES_PER_CELL),
        };

        quad.set_has_color(false);

        Ok(QuadImpl::Vert(quad))
    }

    fn extend_with(&mut self, vertices: &[Vertex]) {
        let idx = self.next.get();
        let len = vertices.len();

        // idx and next are number of quads, so divide by number of vertices
        self.next.set(idx + len / VERTICES_PER_CELL);
        // Only copy in if there is enough room.
        // We'll detect the out of space condition at the end of
        // the render pass.
        let idx = idx * VERTICES_PER_CELL;
        let capacity = self.capacity * VERTICES_PER_CELL;
        if idx + len <= capacity {
            self.mapping
                .slice_mut(idx..idx + len)
                .copy_from_slice(vertices);
        }
    }
}

pub struct TripleVertexBuffer {
    pub index: Cell<usize>,
    pub bufs: RefCell<Vec<VertexBuffer>>,
    pub indices: IndexBuffer,
    pub capacity: usize,
    pub next_quad: Cell<usize>,
}

impl TripleVertexBuffer {
    pub fn clear_quad_allocation(&self) {
        self.next_quad.set(0);
    }

    pub fn need_more_quads(&self) -> Option<usize> {
        let next = self.next_quad.get();
        if next > self.capacity {
            Some(next)
        } else {
            None
        }
    }

    pub fn vertex_index_count(&self) -> (usize, usize) {
        let num_quads = self.next_quad.get();
        (num_quads * VERTICES_PER_CELL, num_quads * INDICES_PER_CELL)
    }

    /// Maps the currently-active vertex buffer and returns a view over it,
    /// tied to the borrow of `bufs` that the caller already holds.
    /// Unlike the old `map()`, this doesn't return an owned, independent
    /// value: the caller (`RenderLayer::with_quad_allocator`) keeps the
    /// `RefMut` guard for `bufs` alive in its own stack frame for exactly
    /// as long as the returned view is used, so this is an ordinary
    /// nested borrow rather than a self-referential struct.
    pub fn map<'a>(&'a self, bufs: &'a mut [VertexBuffer]) -> MappedQuadsView<'a> {
        let index = self.index.get();
        let mapping = match &mut bufs[index] {
            VertexBuffer::Glium(vb) => {
                let buf_slice = vb.slice_mut(..).expect("to map vertex buffer");
                MappedVertexBuffer::Glium(buf_slice.map())
            }
            VertexBuffer::WebGpu(vb) => MappedVertexBuffer::WebGpu(vb.map()),
        };

        MappedQuadsView {
            mapping,
            next: &self.next_quad,
            capacity: self.capacity,
        }
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

    /// Rotates to the next of `bufs.len()` slots. On Glium, `bufs` holds 3
    /// buffers (unchanged legacy behavior: rotating avoids writing into a
    /// buffer the GPU might still be reading from a prior frame, since the
    /// same buffer objects are reused frame over frame). On WebGpu, `bufs`
    /// holds a single buffer -- `recreate()` swaps in a brand new GPU buffer
    /// every frame regardless of slot, so a second/third rotation slot never
    /// holds a buffer the GPU has actually seen before and would just be
    /// wasted resident memory. With one slot, this is a no-op: index stays 0.
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
        let vbs = self.vb.borrow();
        let mut bufs0 = vbs[0].bufs.borrow_mut();
        let mut bufs1 = vbs[1].bufs.borrow_mut();
        let mut bufs2 = vbs[2].bufs.borrow_mut();

        let view0 = vbs[0].map(&mut bufs0);
        let view1 = vbs[1].map(&mut bufs1);
        let view2 = vbs[2].map(&mut bufs2);

        let mut layers = TripleLayerQuadAllocator::Gpu(BorrowedLayers {
            layers: [view0, view1, view2],
        });

        f(&mut layers)
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
        let mut indices = Vec::with_capacity(num_quads * INDICES_PER_CELL);

        for q in 0..num_quads {
            let idx = (q * VERTICES_PER_CELL) as u32;

            // Emit two triangles to form the glyph quad
            indices.push(idx + V_TOP_LEFT as u32);
            indices.push(idx + V_TOP_RIGHT as u32);
            indices.push(idx + V_BOT_LEFT as u32);

            indices.push(idx + V_TOP_RIGHT as u32);
            indices.push(idx + V_BOT_LEFT as u32);
            indices.push(idx + V_BOT_RIGHT as u32);
        }

        // Glium reuses the same buffer objects frame over frame, so it needs
        // to rotate across multiple (3) slots to avoid writing into a buffer
        // the GPU might still be reading from a previous frame. WebGpu's
        // `recreate()` swaps in a brand new GPU buffer every frame regardless
        // of slot (see `call_draw_webgpu`), so a second/third slot would
        // never actually hold a buffer the GPU has seen before -- rotation
        // buys nothing there, so it gets a single slot to avoid keeping 3x
        // the vertex-buffer memory resident for no benefit.
        let num_slots = match context {
            RenderContext::Glium(_) => 3,
            RenderContext::WebGpu(_) => 1,
        };
        let mut bufs = Vec::with_capacity(num_slots);
        for _ in 0..num_slots {
            bufs.push(context.allocate_vertex_buffer(num_quads, &verts)?);
        }

        let buffer = TripleVertexBuffer {
            index: Cell::new(0),
            bufs: RefCell::new(bufs),
            capacity: num_quads,
            indices: context.allocate_index_buffer(&indices)?,
            next_quad: Cell::new(0),
        };

        Ok(buffer)
    }
}

pub struct BorrowedLayers<'a> {
    pub layers: [MappedQuadsView<'a>; 3],
}

impl<'a> TripleLayerQuadAllocatorTrait for BorrowedLayers<'a> {
    fn allocate(&mut self, layer_num: usize) -> anyhow::Result<QuadImpl<'_>> {
        self.layers[layer_num].allocate()
    }

    fn extend_with(&mut self, layer_num: usize, vertices: &[Vertex]) {
        self.layers[layer_num].extend_with(vertices)
    }
}

pub struct RenderState {
    pub context: RenderContext,
    pub glyph_cache: RefCell<GlyphCache>,
    pub util_sprites: UtilSprites,
    pub glyph_prog: Option<glium::Program>,
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
                    let glyph_prog = match &context {
                        RenderContext::Glium(context) => {
                            Some(Self::compile_prog(context, Self::glyph_shader)?)
                        }
                        RenderContext::WebGpu(_) => None,
                    };

                    let main_layer = Rc::new(RenderLayer::new(&context, 1024, 0)?);

                    return Ok(Self {
                        context,
                        glyph_cache,
                        util_sprites,
                        glyph_prog,
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

    fn compile_prog(
        context: &Rc<GliumContext>,
        fragment_shader: fn(&str) -> (String, String),
    ) -> anyhow::Result<glium::Program> {
        let mut errors = vec![];

        let caps = context.get_capabilities();
        log::trace!("Compiling shader. context.capabilities.srgb={}", caps.srgb);

        for version in &["330 core", "330", "320 es", "300 es"] {
            let (vertex_shader, fragment_shader) = fragment_shader(version);
            let source = glium::program::ProgramCreationInput::SourceCode {
                vertex_shader: &vertex_shader,
                fragment_shader: &fragment_shader,
                outputs_srgb: true,
                tessellation_control_shader: None,
                tessellation_evaluation_shader: None,
                transform_feedback_varyings: None,
                uses_point_size: false,
                geometry_shader: None,
            };
            match glium::Program::new(context, source) {
                Ok(prog) => {
                    return Ok(prog);
                }
                Err(err) => errors.push(format!("shader version: {}: {:#}", version, err)),
            };
        }

        anyhow::bail!("Failed to compile shaders: {}", errors.join("\n"))
    }

    fn glyph_shader(version: &str) -> (String, String) {
        (
            format!(
                "#version {}\n{}",
                version,
                include_str!("glyph-vertex.glsl")
            ),
            format!("#version {}\n{}", version, include_str!("glyph-frag.glsl")),
        )
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
