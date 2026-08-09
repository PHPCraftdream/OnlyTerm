use crate::renderstate::BorrowedLayers;
use ::window::bitmaps::TextureRect;
use ::window::color::LinearRgba;
use config::HsbTransform;
use std::ops::Deref;

/// Each cell is composed of two triangles built from 4 vertices.
/// The buffer is organized row by row.
pub const VERTICES_PER_CELL: usize = 4;
pub const V_TOP_LEFT: usize = 0;
pub const V_TOP_RIGHT: usize = 1;
pub const V_BOT_LEFT: usize = 2;
pub const V_BOT_RIGHT: usize = 3;

/// a regular monochrome text glyph
const IS_GLYPH: f32 = 0.0;
/// a color emoji glyph
const IS_COLOR_EMOJI: f32 = 1.0;
/// a full color texture attached as the
/// background image of the window
const IS_BG_IMAGE: f32 = 2.0;
/// like 2.0, except that instead of an
/// image, we use the solid bg color
const IS_SOLID_COLOR: f32 = 3.0;
/// Grayscale poly quad for non-aa text render layers
const IS_GRAY_SCALE: f32 = 4.0;

#[repr(C)]
#[derive(Copy, Clone, Default, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    // Physical position of the corner of the character cell
    pub position: [f32; 2],
    // glyph texture
    pub tex: [f32; 2],
    pub fg_color: [f32; 4],
    pub alt_color: [f32; 4],
    pub hsv: [f32; 3],
    pub has_color: f32,
    pub mix_value: f32,
}

impl Vertex {
    const ATTRIBS: [wgpu::VertexAttribute; 7] = wgpu::vertex_attr_array![
    0 => Float32x2,
    1 => Float32x2,
    2 => Float32x4,
    3 => Float32x4,
    4 => Float32x3,
    5 => Float32,
    6 => Float32,
    ];

    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBS,
        }
    }
}

/// GPU instance data for a single quad.
/// This contains all per-quad-unique data; the 4 corners are shared across all quads.
/// This is the instanced rendering wire format, ~84 bytes per quad (vs 272 for 4 full vertices).
#[repr(C)]
#[derive(Copy, Clone, Default, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct QuadInstance {
    /// Position rect as [left, top, right, bottom]
    pub position: [f32; 4],
    /// FG color as [r, g, b, a]
    pub fg_color: [f32; 4],
    /// Alt color as [r, g, b, a]
    pub alt_color: [f32; 4],
    /// Texture rect as [x1, x2, y1, y2]
    pub tex: [f32; 4],
    /// HSV transform as [hue, saturation, brightness]
    pub hsv: [f32; 3],
    /// Quad type flag (IS_GLYPH, IS_COLOR_EMOJI, etc.)
    pub has_color: f32,
    /// Mix value for fg_color/alt_color blending
    pub mix_value: f32,
}

impl QuadInstance {
    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        // Locations start at 1, not 0: location 0 belongs to `CornerVertex`,
        // the other vertex buffer bound into this same pipeline, and shader
        // attribute locations must be unique across *all* of a pipeline's
        // vertex buffers, not just within one. Numbering these from 0
        // collides with the corner buffer, which wgpu rejects at
        // `create_render_pipeline` time -- and since wgpu treats validation
        // errors as fatal by default, that surfaces as a panic on the render
        // thread, not a recoverable error: the window's renderer rebuild
        // logic then retries, fails identically, and gives up by closing the
        // window.
        //
        // `vertex_attr_array!` assigns byte offsets sequentially from the
        // formats listed here, so this list's ORDER must match
        // `QuadInstance`'s field declaration order (position, fg_color,
        // alt_color, tex, hsv, has_color, mix_value), and shader.wgsl's
        // `InstanceInput` must bind the same field to the same location.
        // Getting the order wrong doesn't fail validation -- it silently
        // feeds e.g. texture coordinates in as a color.
        const ATTRIBS: [wgpu::VertexAttribute; 7] = wgpu::vertex_attr_array![
            1 => Float32x4, // position: [left, top, right, bottom]
            2 => Float32x4, // fg_color
            3 => Float32x4, // alt_color
            4 => Float32x4, // tex: [x1, x2, y1, y2]
            5 => Float32x3, // hsv
            6 => Float32,   // has_color
            7 => Float32,   // mix_value
        ];
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &ATTRIBS,
        }
    }
}

/// Shared corner data: unit coordinates for the 4 corners of a quad.
/// Used with VertexStepMode::Vertex to interpolate instance data.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CornerVertex {
    /// Unit vector in [0,1] range for this corner (e.g., [0.0, 0.0] = top-left)
    pub corner_unit: [f32; 2],
}

impl CornerVertex {
    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        const ATTRIBS: [wgpu::VertexAttribute; 1] = wgpu::vertex_attr_array![
            0 => Float32x2,
        ];
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &ATTRIBS,
        }
    }

    /// Create the 4 static corners for quad rendering.
    /// Order must match V_TOP_LEFT, V_TOP_RIGHT, V_BOT_LEFT, V_BOT_RIGHT.
    pub fn static_corners() -> [CornerVertex; 4] {
        [
            CornerVertex {
                corner_unit: [0.0, 0.0],
            }, // V_TOP_LEFT
            CornerVertex {
                corner_unit: [1.0, 0.0],
            }, // V_TOP_RIGHT
            CornerVertex {
                corner_unit: [0.0, 1.0],
            }, // V_BOT_LEFT
            CornerVertex {
                corner_unit: [1.0, 1.0],
            }, // V_BOT_RIGHT
        ]
    }
}

pub trait QuadTrait {
    /// Assign the texture coordinates
    fn set_texture(&mut self, coords: TextureRect) {
        let x1 = coords.min_x();
        let x2 = coords.max_x();
        let y1 = coords.min_y();
        let y2 = coords.max_y();
        self.set_texture_discrete(x1, x2, y1, y2);
    }
    fn set_texture_discrete(&mut self, x1: f32, x2: f32, y1: f32, y2: f32);
    fn set_has_color_impl(&mut self, has_color: f32);

    /// Set the color glyph "flag"
    fn set_has_color(&mut self, has_color: bool) {
        self.set_has_color_impl(if has_color { IS_COLOR_EMOJI } else { IS_GLYPH });
    }

    /// Mark as a grayscale polyquad; color and alpha will be
    /// multipled with those in the texture
    fn set_grayscale(&mut self) {
        self.set_has_color_impl(IS_GRAY_SCALE);
    }

    /// Mark this quad as a background image.
    /// Mutually exclusive with set_has_color.
    fn set_is_background_image(&mut self) {
        self.set_has_color_impl(IS_BG_IMAGE);
    }

    fn set_is_background(&mut self) {
        self.set_has_color_impl(IS_SOLID_COLOR);
    }

    fn set_fg_color(&mut self, color: LinearRgba);

    /// Must be called after set_fg_color
    fn set_alt_color_and_mix_value(&mut self, color: LinearRgba, mix_value: f32);

    fn set_hsv(&mut self, hsv: Option<HsbTransform>);
    fn set_position(&mut self, left: f32, top: f32, right: f32, bottom: f32);
}

impl QuadTrait for QuadInstance {
    fn set_texture_discrete(&mut self, x1: f32, x2: f32, y1: f32, y2: f32) {
        self.tex = [x1, x2, y1, y2];
    }

    fn set_has_color_impl(&mut self, has_color: f32) {
        self.has_color = has_color;
    }

    fn set_fg_color(&mut self, color: LinearRgba) {
        self.fg_color = color.into();
    }

    fn set_alt_color_and_mix_value(&mut self, color: LinearRgba, mix_value: f32) {
        self.alt_color = color.into();
        self.mix_value = mix_value;
    }
    fn set_hsv(&mut self, hsv: Option<HsbTransform>) {
        let (h, s, v) = hsv
            .map(|t| (t.hue, t.saturation, t.brightness))
            .unwrap_or((1., 1., 1.));
        self.hsv = [h, s, v];
    }

    fn set_position(&mut self, left: f32, top: f32, right: f32, bottom: f32) {
        self.position = [left, top, right, bottom];
    }
}

pub enum QuadImpl<'a> {
    Vert(Quad<'a>),
    Boxed(&'a mut QuadInstance),
}

impl<'a> QuadTrait for QuadImpl<'a> {
    fn set_texture_discrete(&mut self, x1: f32, x2: f32, y1: f32, y2: f32) {
        match self {
            Self::Vert(q) => q.set_texture_discrete(x1, x2, y1, y2),
            Self::Boxed(q) => q.set_texture_discrete(x1, x2, y1, y2),
        }
    }

    fn set_has_color_impl(&mut self, has_color: f32) {
        match self {
            Self::Vert(q) => q.set_has_color_impl(has_color),
            Self::Boxed(q) => q.set_has_color_impl(has_color),
        }
    }

    fn set_fg_color(&mut self, color: LinearRgba) {
        match self {
            Self::Vert(q) => q.set_fg_color(color),
            Self::Boxed(q) => q.set_fg_color(color),
        }
    }

    fn set_alt_color_and_mix_value(&mut self, color: LinearRgba, mix_value: f32) {
        match self {
            Self::Vert(q) => q.set_alt_color_and_mix_value(color, mix_value),
            Self::Boxed(q) => q.set_alt_color_and_mix_value(color, mix_value),
        }
    }

    fn set_hsv(&mut self, hsv: Option<HsbTransform>) {
        match self {
            Self::Vert(q) => q.set_hsv(hsv),
            Self::Boxed(q) => q.set_hsv(hsv),
        }
    }

    fn set_position(&mut self, left: f32, top: f32, right: f32, bottom: f32) {
        match self {
            Self::Vert(q) => q.set_position(left, top, right, bottom),
            Self::Boxed(q) => q.set_position(left, top, right, bottom),
        }
    }
}

/// A helper for updating the 4 vertices that compose a glyph cell
pub struct Quad<'a> {
    pub(crate) vert: &'a mut [Vertex],
}

impl<'a> QuadTrait for Quad<'a> {
    fn set_texture_discrete(&mut self, x1: f32, x2: f32, y1: f32, y2: f32) {
        self.vert[V_TOP_LEFT].tex = [x1, y1];
        self.vert[V_TOP_RIGHT].tex = [x2, y1];
        self.vert[V_BOT_LEFT].tex = [x1, y2];
        self.vert[V_BOT_RIGHT].tex = [x2, y2];
    }

    fn set_has_color_impl(&mut self, has_color: f32) {
        for v in self.vert.iter_mut() {
            v.has_color = has_color;
        }
    }

    fn set_fg_color(&mut self, color: LinearRgba) {
        for v in self.vert.iter_mut() {
            v.fg_color = color.into();
        }
        self.set_alt_color_and_mix_value(color, 0.);
    }

    /// Must be called after set_fg_color
    fn set_alt_color_and_mix_value(&mut self, color: LinearRgba, mix_value: f32) {
        for v in self.vert.iter_mut() {
            v.alt_color = color.into();
            v.mix_value = mix_value;
        }
    }

    fn set_hsv(&mut self, hsv: Option<HsbTransform>) {
        let (h, s, v) = hsv
            .map(|t| (t.hue, t.saturation, t.brightness))
            .unwrap_or((1., 1., 1.));
        for vert in self.vert.iter_mut() {
            vert.hsv = [h, s, v];
        }
    }

    fn set_position(&mut self, left: f32, top: f32, right: f32, bottom: f32) {
        self.vert[V_TOP_LEFT].position = [left, top];
        self.vert[V_TOP_RIGHT].position = [right, top];
        self.vert[V_BOT_LEFT].position = [left, bottom];
        self.vert[V_BOT_RIGHT].position = [right, bottom];
    }
}

pub trait QuadAllocator {
    fn allocate(&mut self) -> anyhow::Result<QuadImpl<'_>>;
    fn extend_with(&mut self, vertices: &[Vertex]);
    fn extend_with_instance(&mut self, instance: QuadInstance);
}

pub trait TripleLayerQuadAllocatorTrait {
    fn allocate(&mut self, layer_num: usize) -> anyhow::Result<QuadImpl<'_>>;
    // Legacy vertex path removed - now using instanced rendering with extend_with_instance
    fn extend_with_instance(&mut self, layer_num: usize, instance: QuadInstance);
    fn extend_from_slice(&mut self, layer_num: usize, instances: &[QuadInstance]) {
        for instance in instances {
            self.extend_with_instance(layer_num, *instance);
        }
    }
}

#[derive(Default)]
pub struct HeapQuadAllocator {
    layer0: Vec<QuadInstance>,
    layer1: Vec<QuadInstance>,
    layer2: Vec<QuadInstance>,
}

impl std::fmt::Debug for HeapQuadAllocator {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        fmt.debug_struct("HeapQuadAllocator").finish()
    }
}

impl HeapQuadAllocator {
    pub fn apply_to(&self, other: &mut TripleLayerQuadAllocator) -> anyhow::Result<()> {
        let start = std::time::Instant::now();
        other.extend_from_slice(0, &self.layer0);
        other.extend_from_slice(1, &self.layer1);
        other.extend_from_slice(2, &self.layer2);
        metrics::histogram!("quad_buffer_apply").record(start.elapsed());
        Ok(())
    }
}

impl TripleLayerQuadAllocatorTrait for HeapQuadAllocator {
    fn allocate(&mut self, layer_num: usize) -> anyhow::Result<QuadImpl<'_>> {
        let quads = match layer_num {
            0 => &mut self.layer0,
            1 => &mut self.layer1,
            2 => &mut self.layer2,
            _ => unreachable!(),
        };

        quads.push(QuadInstance::default());

        let quad = quads.last_mut().unwrap();
        Ok(QuadImpl::Boxed(quad))
    }

    fn extend_with_instance(&mut self, layer_num: usize, instance: QuadInstance) {
        let dest_quads = match layer_num {
            0 => &mut self.layer0,
            1 => &mut self.layer1,
            2 => &mut self.layer2,
            _ => unreachable!(),
        };
        dest_quads.push(instance);
    }
}

pub enum TripleLayerQuadAllocator<'a> {
    Gpu(BorrowedLayers),
    Heap(&'a mut HeapQuadAllocator),
}

impl<'a> TripleLayerQuadAllocatorTrait for TripleLayerQuadAllocator<'a> {
    fn allocate(&mut self, layer_num: usize) -> anyhow::Result<QuadImpl<'_>> {
        match self {
            Self::Gpu(b) => b.allocate(layer_num),
            Self::Heap(h) => h.allocate(layer_num),
        }
    }

    fn extend_with_instance(&mut self, layer_num: usize, instance: QuadInstance) {
        match self {
            Self::Gpu(b) => b.extend_with_instance(layer_num, instance),
            Self::Heap(h) => h.extend_with_instance(layer_num, instance),
        }
    }

    fn extend_from_slice(&mut self, layer_num: usize, instances: &[QuadInstance]) {
        match self {
            Self::Gpu(b) => b.extend_from_slice(layer_num, instances),
            Self::Heap(h) => h.extend_from_slice(layer_num, instances),
        }
    }
}

// Trait wrapper to allow calling HeapQuadAllocator methods through Rc
pub trait HeapQuadAllocatorExt {
    fn apply_to_translated(
        &self,
        other: &mut TripleLayerQuadAllocator,
        dx: f32,
        dy: f32,
    ) -> anyhow::Result<()>;
}

impl HeapQuadAllocatorExt for HeapQuadAllocator {
    fn apply_to_translated(
        &self,
        other: &mut TripleLayerQuadAllocator,
        dx: f32,
        dy: f32,
    ) -> anyhow::Result<()> {
        let start = std::time::Instant::now();
        for (layer, instances) in [(0, &self.layer0), (1, &self.layer1), (2, &self.layer2)] {
            for mut instance in instances.iter().copied() {
                instance.position[0] += dx;
                instance.position[1] += dy;
                instance.position[2] += dx;
                instance.position[3] += dy;
                other.extend_with_instance(layer, instance);
            }
        }
        metrics::histogram!("quad_buffer_apply").record(start.elapsed());
        Ok(())
    }
}

impl<T: Deref<Target = HeapQuadAllocator>> HeapQuadAllocatorExt for T {
    fn apply_to_translated(
        &self,
        other: &mut TripleLayerQuadAllocator,
        dx: f32,
        dy: f32,
    ) -> anyhow::Result<()> {
        self.deref().apply_to_translated(other, dx, dy)
    }
}

#[cfg(test)]
#[test]
fn size() {
    // Old: 4 vertices per quad, each 68 bytes = 272 bytes per quad
    assert_eq!(std::mem::size_of::<Vertex>() * VERTICES_PER_CELL, 272);
    // QuadInstance is the GPU instance format, 84 bytes
    assert_eq!(std::mem::size_of::<QuadInstance>(), 84);
    // CornerVertex is 8 bytes (2 f32s)
    assert_eq!(std::mem::size_of::<CornerVertex>(), 8);
}

/// Test that HeapQuadAllocator::apply_to's bulk path preserves overflow-drop semantics.
/// The old per-quad loop in MappedQuadsView::extend_with_instance silently drops quads
/// once self.next.get() >= self.capacity; the new bulk path must truncate exactly the
/// same way, not silently accept everything.
///
/// This test calls the REAL production code: HeapQuadAllocator::apply_to -> BorrowedLayers::extend_from_slice.
/// It would fail if BorrowedLayers::extend_from_slice had a bug in its truncation logic.
#[cfg(test)]
#[test]
fn test_apply_to_preserves_overflow_drop() {
    use crate::quad::TripleLayerQuadAllocator;
    use crate::renderstate::BorrowedLayers;

    // Build a real TripleVertexBuffer with capacity 2 (empty bufs is safe for map_instances/accumulate)
    let vb = crate::renderstate::TripleVertexBuffer::new(vec![], 2);

    // Map instances to get MappedQuadsViews for all three layers
    let view0 = vb.map_instances();
    let view1 = vb.map_instances();
    let view2 = vb.map_instances();

    // Build real BorrowedLayers with capacity-limited views
    let borrowed_layers = BorrowedLayers {
        layers: [view0, view1, view2],
    };

    // Wrap in TripleLayerQuadAllocator::Gpu (the real production type used in apply_to)
    let mut dest = TripleLayerQuadAllocator::Gpu(borrowed_layers);

    // Build a HeapQuadAllocator with 5 quads in layer 0 (exceeds capacity 2)
    let mut heap = HeapQuadAllocator::default();
    for i in 0..5 {
        let mut q = QuadInstance::default();
        q.position[0] = i as f32;
        heap.layer0.push(q);
    }
    assert_eq!(heap.layer0.len(), 5);

    // Call the REAL HeapQuadAllocator::apply_to against the real capacity-limited destination
    // This invokes BorrowedLayers::extend_from_slice with truncation logic
    heap.apply_to(&mut dest).unwrap();

    // Extract the result back out to verify truncation
    let TripleLayerQuadAllocator::Gpu(borrowed_layers) = dest else {
        panic!("Expected Gpu variant");
    };

    // Extract layer 0 from the borrowed layers
    // Note: borrowed_layers is consumed here, which is fine since we're done with it
    let [layer_view, _, _] = borrowed_layers.layers;

    // Extract the instances from layer 0
    let result = layer_view.into_instances();

    // The destination must have exactly capacity (2) instances, not 5
    // This proves the real BorrowedLayers::extend_from_slice truncate logic ran correctly
    assert_eq!(
        result.len(),
        2,
        "Bulk path should truncate overflow to capacity (real BorrowedLayers::extend_from_slice)"
    );
    // Verify the FIRST 2 quads were copied (truncation keeps the start, not the end)
    assert_eq!(
        result[0].position[0], 0.0,
        "First instance should be position[0]=0.0"
    );
    assert_eq!(
        result[1].position[0], 1.0,
        "Second instance should be position[0]=1.0"
    );
}

/// Companion to `test_apply_to_preserves_overflow_drop`: the common case is
/// NOT overflow, it's a layer with plenty of spare capacity (e.g. one
/// terminal line's ~80 quads against a layer pre-sized for thousands). A
/// prior version of `BorrowedLayers::extend_from_slice` used
/// `instances.get(..available)`, which returns `None` (and therefore an
/// empty slice via `unwrap_or`) whenever `available > instances.len()` --
/// i.e. in the overwhelmingly common non-overflow case, silently dropping
/// every quad instead of copying it. This shipped as a real regression
/// (task #453): line content stopped reaching the GPU, producing a white
/// screen with an otherwise-responsive window. This test pins the fix.
#[test]
fn test_apply_to_copies_everything_when_under_capacity() {
    use crate::quad::TripleLayerQuadAllocator;
    use crate::renderstate::BorrowedLayers;

    // Capacity (10) is well above the instance count (3) -- the normal case.
    let vb = crate::renderstate::TripleVertexBuffer::new(vec![], 10);
    let view0 = vb.map_instances();
    let view1 = vb.map_instances();
    let view2 = vb.map_instances();
    let borrowed_layers = BorrowedLayers {
        layers: [view0, view1, view2],
    };
    let mut dest = TripleLayerQuadAllocator::Gpu(borrowed_layers);

    let mut heap = HeapQuadAllocator::default();
    for i in 0..3 {
        let mut q = QuadInstance::default();
        q.position[0] = i as f32;
        heap.layer0.push(q);
    }

    heap.apply_to(&mut dest).unwrap();

    let TripleLayerQuadAllocator::Gpu(borrowed_layers) = dest else {
        panic!("Expected Gpu variant");
    };
    let [layer_view, _, _] = borrowed_layers.layers;
    let result = layer_view.into_instances();

    assert_eq!(
        result.len(),
        3,
        "All 3 instances should survive when well under capacity, not be silently dropped"
    );
    assert_eq!(result[0].position[0], 0.0);
    assert_eq!(result[1].position[0], 1.0);
    assert_eq!(result[2].position[0], 2.0);
}

/// Test that Rc<HeapQuadAllocator> (post task #457's LineQuadCacheValue::layers type change)
/// still applies correctly via .apply_to() when called through the Rc. This pins that the
/// type change from HeapQuadAllocator to Rc<HeapQuadAllocator> didn't silently break anything
/// about how apply_to is invoked at real call sites (both cache-hit and retained-row paths).
#[test]
fn test_rc_heap_quad_allocator_apply_to() {
    use crate::quad::TripleLayerQuadAllocator;
    use crate::renderstate::BorrowedLayers;
    use crate::renderstate::TripleVertexBuffer;
    use std::rc::Rc;

    // Build a HeapQuadAllocator and wrap it in Rc.
    let mut heap = HeapQuadAllocator::default();
    for i in 0..3 {
        let mut q = QuadInstance::default();
        q.position[0] = i as f32;
        heap.layer0.push(q);
    }
    let rc_heap = Rc::new(heap);

    // Create a layer accumulator.
    let vb = TripleVertexBuffer::new(vec![], 10);
    let view0 = vb.map_instances();
    let view1 = vb.map_instances();
    let view2 = vb.map_instances();
    let borrowed_layers = BorrowedLayers {
        layers: [view0, view1, view2],
    };

    // Call apply_to through the Rc (the real production usage pattern).
    let mut dest = TripleLayerQuadAllocator::Gpu(borrowed_layers);
    rc_heap.apply_to(&mut dest).unwrap();

    // Extract the result.
    let TripleLayerQuadAllocator::Gpu(borrowed_layers) = dest else {
        panic!("Expected Gpu variant");
    };
    let [layer_view, _, _] = borrowed_layers.layers;
    let result = layer_view.into_instances();

    // Verify all 3 instances were copied correctly.
    assert_eq!(
        result.len(),
        3,
        "Rc<HeapQuadAllocator>::apply_to should copy all instances"
    );
    assert_eq!(result[0].position[0], 0.0);
    assert_eq!(result[1].position[0], 1.0);
    assert_eq!(result[2].position[0], 2.0);
}

/// Regression coverage for the instanced-rendering vertex layout (task #447).
///
/// The instanced pipeline binds two vertex buffers -- `CornerVertex` (one
/// static set of 4 corners, `step_mode: Vertex`) and `QuadInstance` (one
/// record per quad, `step_mode: Instance`) -- and there is no single place
/// where a mismatch between them shows up as a compile error. Two distinct
/// ways to get it wrong both actually happened while building this, and
/// neither was caught by the build, by clippy, or by launching the app for a
/// few seconds:
///
///  1. **Colliding locations.** Shader attribute locations must be unique
///     across *all* of a pipeline's vertex buffers, not just within one
///     buffer. Numbering `QuadInstance`'s attributes from 0 collided with
///     `CornerVertex`'s location 0. wgpu rejects that at
///     `create_render_pipeline` time, and since it treats validation errors
///     as fatal by default, the failure arrives as a *panic on the render
///     thread* -- which the window's renderer-rebuild logic then retries,
///     fails identically, and finally gives up on by closing the window
///     (taking every tab in it). Nothing reaches stderr, because the GUI
///     binary is `windows_subsystem = "windows"`; nothing reaches Windows
///     Error Reporting either, because closing the window is a *graceful*
///     shutdown, not a crash. The only trace is OnlyTerm's own per-PID log
///     file under `config::RUNTIME_DIR`.
///
///  2. **Right locations, wrong fields.** `vertex_attr_array!` derives each
///     attribute's byte offset from its position in the list, so the list's
///     order has to match `QuadInstance`'s field declaration order, and
///     shader.wgsl has to bind each location to the field that actually
///     lands at that offset. Swapping two same-sized fields (e.g. `tex` and
///     `fg_color`, both `vec4<f32>`) passes validation and simply renders
///     wrong -- texture coordinates fed in as a color.
///
/// These tests pin all three sides (the Rust struct layout, the
/// `VertexBufferLayout` descriptors, and the WGSL declarations) against one
/// shared table, so changing any one of them without the others fails here
/// rather than at runtime on a user's screen.
#[cfg(test)]
mod pipeline_layout {
    use super::*;

    /// One per-instance attribute, as it must appear in all three places.
    struct AttrSpec {
        /// `@location(N)` in shader.wgsl, and `shader_location` in the
        /// `VertexAttribute`.
        location: u32,
        /// Byte offset of the backing field within `QuadInstance`, taken
        /// from the struct itself rather than hand-counted.
        offset: u64,
        format: wgpu::VertexFormat,
        /// Field name as declared in shader.wgsl.
        wgsl_name: &'static str,
        wgsl_type: &'static str,
    }

    /// The single source of truth: which `QuadInstance` field feeds which
    /// shader location, in the order `vertex_attr_array!` lays them out.
    fn instance_spec() -> Vec<AttrSpec> {
        use std::mem::offset_of;
        vec![
            AttrSpec {
                location: 1,
                offset: offset_of!(QuadInstance, position) as u64,
                format: wgpu::VertexFormat::Float32x4,
                wgsl_name: "position",
                wgsl_type: "vec4<f32>",
            },
            AttrSpec {
                location: 2,
                offset: offset_of!(QuadInstance, fg_color) as u64,
                format: wgpu::VertexFormat::Float32x4,
                wgsl_name: "fg_color",
                wgsl_type: "vec4<f32>",
            },
            AttrSpec {
                location: 3,
                offset: offset_of!(QuadInstance, alt_color) as u64,
                format: wgpu::VertexFormat::Float32x4,
                wgsl_name: "alt_color",
                wgsl_type: "vec4<f32>",
            },
            AttrSpec {
                location: 4,
                offset: offset_of!(QuadInstance, tex) as u64,
                format: wgpu::VertexFormat::Float32x4,
                wgsl_name: "tex",
                wgsl_type: "vec4<f32>",
            },
            AttrSpec {
                location: 5,
                offset: offset_of!(QuadInstance, hsv) as u64,
                format: wgpu::VertexFormat::Float32x3,
                wgsl_name: "hsv",
                wgsl_type: "vec3<f32>",
            },
            AttrSpec {
                location: 6,
                offset: offset_of!(QuadInstance, has_color) as u64,
                format: wgpu::VertexFormat::Float32,
                wgsl_name: "has_color",
                wgsl_type: "f32",
            },
            AttrSpec {
                location: 7,
                offset: offset_of!(QuadInstance, mix_value) as u64,
                format: wgpu::VertexFormat::Float32,
                wgsl_name: "mix_value",
                wgsl_type: "f32",
            },
        ]
    }

    const SHADER_SRC: &str = include_str!("shader.wgsl");

    /// Extract `(location, field_name, wgsl_type)` for every `@location(..)`
    /// member of a WGSL `struct <name> { .. }` block, in declaration order.
    fn parse_wgsl_struct(src: &str, struct_name: &str) -> Vec<(u32, String, String)> {
        let header = format!("struct {struct_name} {{");
        let start = src
            .find(&header)
            .unwrap_or_else(|| panic!("shader.wgsl no longer declares `{}`", header));
        let body_start = start + header.len();
        let body_len = src[body_start..]
            .find('}')
            .unwrap_or_else(|| panic!("unterminated `struct {}` in shader.wgsl", struct_name));
        let body = &src[body_start..body_start + body_len];

        let mut out = vec![];
        for line in body.lines() {
            // Drop any trailing `// ...` comment before parsing; a
            // comment-only line then trims to empty and is skipped.
            let line = line.split("//").next().unwrap_or("").trim();
            let Some(rest) = line.strip_prefix("@location(") else {
                continue;
            };
            let close = rest
                .find(')')
                .unwrap_or_else(|| panic!("unterminated `@location(` in: {}", line));
            let location: u32 = rest[..close]
                .trim()
                .parse()
                .unwrap_or_else(|e| panic!("non-numeric location in `{}`: {}", line, e));
            let decl = rest[close + 1..].trim().trim_end_matches(',');
            let (name, ty) = decl
                .split_once(':')
                .unwrap_or_else(|| panic!("expected `name: type` in: {}", line));
            out.push((
                location,
                name.trim().to_string(),
                ty.trim().trim_end_matches(',').trim().to_string(),
            ));
        }
        out
    }

    /// `QuadInstance::desc()` must describe exactly the fields of
    /// `QuadInstance`, at the offsets those fields actually occupy.
    #[test]
    fn quad_instance_desc_matches_struct_layout() {
        let desc = QuadInstance::desc();
        let spec = instance_spec();

        assert_eq!(
            desc.step_mode,
            wgpu::VertexStepMode::Instance,
            "QuadInstance is the per-quad buffer; stepping it per-vertex would \
             replay one quad's data across the 4 shared corners"
        );
        assert_eq!(
            desc.array_stride,
            std::mem::size_of::<QuadInstance>() as wgpu::BufferAddress,
            "array_stride must be the full struct size or every instance after \
             the first reads misaligned data"
        );
        assert_eq!(
            desc.attributes.len(),
            spec.len(),
            "every QuadInstance field must be described exactly once"
        );

        for (attr, want) in desc.attributes.iter().zip(spec.iter()) {
            assert_eq!(
                attr.shader_location, want.location,
                "attribute for `{}` is at location {}, expected {}",
                want.wgsl_name, attr.shader_location, want.location
            );
            assert_eq!(
                attr.offset, want.offset,
                "attribute at location {} must read from `{}` (offset {}), but \
                 vertex_attr_array! placed it at offset {} -- the ATTRIBS list \
                 order no longer matches the struct's field order",
                want.location, want.wgsl_name, want.offset, attr.offset
            );
            assert_eq!(
                attr.format, want.format,
                "attribute at location {} (`{}`) has the wrong format",
                want.location, want.wgsl_name
            );
        }
    }

    /// The corner buffer is a fixed 4-entry, single-attribute buffer.
    #[test]
    fn corner_vertex_desc_matches_struct_layout() {
        let desc = CornerVertex::desc();
        assert_eq!(desc.step_mode, wgpu::VertexStepMode::Vertex);
        assert_eq!(
            desc.array_stride,
            std::mem::size_of::<CornerVertex>() as wgpu::BufferAddress
        );
        assert_eq!(desc.attributes.len(), 1);
        assert_eq!(desc.attributes[0].shader_location, 0);
        assert_eq!(desc.attributes[0].offset, 0);
        assert_eq!(desc.attributes[0].format, wgpu::VertexFormat::Float32x2);
    }

    /// The bug that actually shipped: shader locations are a single
    /// namespace shared by every vertex buffer in the pipeline, so the
    /// corner buffer's location 0 and the instance buffer's locations must
    /// not overlap. wgpu rejects a pipeline that violates this, and it does
    /// so by panicking on the render thread.
    #[test]
    fn pipeline_vertex_locations_do_not_collide() {
        let mut seen: Vec<(u32, &'static str)> = vec![];
        for attr in CornerVertex::desc().attributes {
            seen.push((attr.shader_location, "CornerVertex"));
        }
        for attr in QuadInstance::desc().attributes {
            seen.push((attr.shader_location, "QuadInstance"));
        }

        for i in 0..seen.len() {
            for j in (i + 1)..seen.len() {
                assert_ne!(
                    seen[i].0, seen[j].0,
                    "shader location {} is claimed by both {} and {}; locations \
                     must be unique across every vertex buffer bound to one \
                     pipeline, or create_render_pipeline fails validation (and \
                     wgpu turns that into a render-thread panic, which ends up \
                     closing the window)",
                    seen[i].0, seen[i].1, seen[j].1
                );
            }
        }
    }

    /// shader.wgsl's `InstanceInput` must bind each location to the same
    /// field the Rust side feeds into it. Two same-sized fields swapped here
    /// still passes GPU validation and just renders wrong.
    #[test]
    fn shader_instance_input_matches_rust_layout() {
        let parsed = parse_wgsl_struct(SHADER_SRC, "InstanceInput");
        let spec = instance_spec();

        assert_eq!(
            parsed.len(),
            spec.len(),
            "shader.wgsl's InstanceInput declares {} attributes, Rust supplies {}",
            parsed.len(),
            spec.len()
        );

        for (want, (location, name, ty)) in spec.iter().zip(parsed.iter()) {
            assert_eq!(
                *location, want.location,
                "shader.wgsl's InstanceInput is out of order: found location {} \
                 (`{}`) where the Rust layout supplies location {} (`{}`)",
                location, name, want.location, want.wgsl_name
            );
            assert_eq!(
                name, want.wgsl_name,
                "location {} is fed from QuadInstance::{} (offset {}), but \
                 shader.wgsl binds it to `{}`",
                want.location, want.wgsl_name, want.offset, name
            );
            assert_eq!(
                ty, want.wgsl_type,
                "location {} (`{}`) is declared `{}` in shader.wgsl but supplied \
                 as {:?}",
                want.location, want.wgsl_name, ty, want.format
            );
        }
    }

    #[test]
    fn shader_corner_input_matches_rust_layout() {
        let parsed = parse_wgsl_struct(SHADER_SRC, "CornerInput");
        assert_eq!(
            parsed.len(),
            1,
            "CornerInput should have exactly one member"
        );
        assert_eq!(parsed[0].0, 0, "corner attribute must be at location 0");
        assert_eq!(parsed[0].1, "corner_unit");
        assert_eq!(parsed[0].2, "vec2<f32>");
    }

    /// The 4 shared corners must reproduce, via
    /// `mix(pos_min, pos_max, corner_unit)` in the vertex shader, exactly
    /// the same (position, tex) pairs the pre-instancing code baked into 4
    /// full vertices -- see `Quad::set_position`/`set_texture_discrete`.
    /// A swapped pair here renders every glyph mirrored or degenerate
    /// without failing anything else.
    #[test]
    fn static_corners_match_the_pre_instancing_vertex_convention() {
        let corners = CornerVertex::static_corners();
        assert_eq!(corners.len(), VERTICES_PER_CELL);

        // Same rect the old 4-vertex path would have been handed.
        let (left, top, right, bottom) = (10.0f32, 20.0f32, 30.0f32, 40.0f32);
        let (x1, x2, y1, y2) = (0.1f32, 0.2f32, 0.3f32, 0.4f32);

        // What the old code baked into each vertex, by index.
        let expected_position = [
            [left, top],     // V_TOP_LEFT
            [right, top],    // V_TOP_RIGHT
            [left, bottom],  // V_BOT_LEFT
            [right, bottom], // V_BOT_RIGHT
        ];
        let expected_tex = [
            [x1, y1], // V_TOP_LEFT
            [x2, y1], // V_TOP_RIGHT
            [x1, y2], // V_BOT_LEFT
            [x2, y2], // V_BOT_RIGHT
        ];

        // The vertex shader's `mix(min, max, corner_unit)`, in Rust.
        let mix = |min: f32, max: f32, t: f32| min + (max - min) * t;

        for (idx, corner) in corners.iter().enumerate() {
            let [u, v] = corner.corner_unit;
            assert!(
                (u == 0.0 || u == 1.0) && (v == 0.0 || v == 1.0),
                "corner {idx} unit vector must select an edge, got {:?}",
                corner.corner_unit
            );

            let position = [mix(left, right, u), mix(top, bottom, v)];
            assert_eq!(
                position, expected_position[idx],
                "corner {idx} produces position {:?}, but the pre-instancing \
                 vertex at that index was {:?}",
                position, expected_position[idx]
            );

            // tex is packed as [x1, x2, y1, y2]: u picks between x1/x2, v
            // between y1/y2 (see vs_main's tex_min/tex_max).
            let tex = [mix(x1, x2, u), mix(y1, y2, v)];
            assert_eq!(
                tex, expected_tex[idx],
                "corner {idx} produces tex {:?}, but the pre-instancing vertex \
                 at that index was {:?}",
                tex, expected_tex[idx]
            );
        }

        // Sanity-check the named indices themselves still mean what the
        // table above assumes.
        assert_eq!(corners[V_TOP_LEFT].corner_unit, [0.0, 0.0]);
        assert_eq!(corners[V_TOP_RIGHT].corner_unit, [1.0, 0.0]);
        assert_eq!(corners[V_BOT_LEFT].corner_unit, [0.0, 1.0]);
        assert_eq!(corners[V_BOT_RIGHT].corner_unit, [1.0, 1.0]);
    }
}

/// Test that HeapQuadAllocator::apply_to_translated offsets quad positions correctly.
#[test]
fn test_apply_to_translated_offsets_correctly() {
    use crate::quad::TripleLayerQuadAllocator;
    use crate::renderstate::BorrowedLayers;

    // Build a real TripleVertexBuffer with capacity 10
    let vb = crate::renderstate::TripleVertexBuffer::new(vec![], 10);

    // Map instances to get MappedQuadsViews for all three layers
    let view0 = vb.map_instances();
    let view1 = vb.map_instances();
    let view2 = vb.map_instances();

    // Build real BorrowedLayers
    let borrowed_layers = BorrowedLayers {
        layers: [view0, view1, view2],
    };

    // Wrap in TripleLayerQuadAllocator::Gpu (the real production type)
    let mut dest = TripleLayerQuadAllocator::Gpu(borrowed_layers);

    // Build a HeapQuadAllocator with a quad at [0,0,10,10] in layer 0
    let mut heap = HeapQuadAllocator::default();
    let q = QuadInstance {
        position: [0.0, 0.0, 10.0, 10.0], // [left, top, right, bottom]
        ..Default::default()
    };
    heap.layer0.push(q);

    // Call apply_to_translated with dx=5.0, dy=7.0
    heap.apply_to_translated(&mut dest, 5.0, 7.0).unwrap();

    // Extract the result back out
    let TripleLayerQuadAllocator::Gpu(borrowed_layers) = dest else {
        panic!("Expected Gpu variant");
    };
    let [layer_view, _, _] = borrowed_layers.layers;
    let result = layer_view.into_instances();

    // Verify the quad was offset correctly: [0+5, 0+7, 10+5, 10+7] = [5, 7, 15, 17]
    assert_eq!(result.len(), 1, "Should have exactly 1 quad");
    assert_eq!(
        result[0].position,
        [5.0, 7.0, 15.0, 17.0],
        "apply_to_translated should add dx/dy to position"
    );
}

/// Test that HeapQuadAllocator::apply_to_translated with zero offset matches apply_to.
#[test]
fn test_apply_to_translated_zero_offset_matches_apply_to() {
    use crate::quad::TripleLayerQuadAllocator;
    use crate::renderstate::BorrowedLayers;

    // Build a HeapQuadAllocator with some quads in different layers
    let mut heap = HeapQuadAllocator::default();
    for i in 0..3 {
        let q = QuadInstance {
            position: [i as f32, i as f32 * 2.0, i as f32 * 3.0, i as f32 * 4.0],
            ..Default::default()
        };
        heap.layer0.push(q);
    }
    for i in 0..2 {
        let q = QuadInstance {
            position: [
                i as f32 + 10.0,
                i as f32 + 20.0,
                i as f32 + 30.0,
                i as f32 + 40.0,
            ],
            ..Default::default()
        };
        heap.layer1.push(q);
    }

    // Create two destinations
    let vb1 = crate::renderstate::TripleVertexBuffer::new(vec![], 20);
    let vb2 = crate::renderstate::TripleVertexBuffer::new(vec![], 20);

    let view0_1 = vb1.map_instances();
    let view1_1 = vb1.map_instances();
    let view2_1 = vb1.map_instances();
    let borrowed_layers1 = BorrowedLayers {
        layers: [view0_1, view1_1, view2_1],
    };
    let mut dest1 = TripleLayerQuadAllocator::Gpu(borrowed_layers1);

    let view0_2 = vb2.map_instances();
    let view1_2 = vb2.map_instances();
    let view2_2 = vb2.map_instances();
    let borrowed_layers2 = BorrowedLayers {
        layers: [view0_2, view1_2, view2_2],
    };
    let mut dest2 = TripleLayerQuadAllocator::Gpu(borrowed_layers2);

    // Call apply_to on dest1
    heap.apply_to(&mut dest1).unwrap();

    // Call apply_to_translated with zero offset on dest2
    heap.apply_to_translated(&mut dest2, 0.0, 0.0).unwrap();

    // Extract results
    let TripleLayerQuadAllocator::Gpu(borrowed_layers1) = dest1 else {
        panic!("Expected Gpu variant");
    };
    let [layer_view1_0, layer_view1_1, layer_view1_2] = borrowed_layers1.layers;
    let result1_0 = layer_view1_0.into_instances();
    let result1_1 = layer_view1_1.into_instances();
    let result1_2 = layer_view1_2.into_instances();

    let TripleLayerQuadAllocator::Gpu(borrowed_layers2) = dest2 else {
        panic!("Expected Gpu variant");
    };
    let [layer_view2_0, layer_view2_1, layer_view2_2] = borrowed_layers2.layers;
    let result2_0 = layer_view2_0.into_instances();
    let result2_1 = layer_view2_1.into_instances();
    let result2_2 = layer_view2_2.into_instances();

    // Compare results across all three layers
    assert_eq!(
        result1_0.len(),
        result2_0.len(),
        "Layer 0: same number of quads"
    );
    assert_eq!(
        result1_1.len(),
        result2_1.len(),
        "Layer 1: same number of quads"
    );
    assert_eq!(
        result1_2.len(),
        result2_2.len(),
        "Layer 2: same number of quads"
    );

    for i in 0..result1_0.len() {
        assert_eq!(
            result1_0[i].position, result2_0[i].position,
            "Layer 0 quad {}: position should match",
            i
        );
        assert_eq!(
            result1_0[i].fg_color, result2_0[i].fg_color,
            "Layer 0 quad {}: fg_color should match",
            i
        );
        assert_eq!(
            result1_0[i].alt_color, result2_0[i].alt_color,
            "Layer 0 quad {}: alt_color should match",
            i
        );
    }

    for i in 0..result1_1.len() {
        assert_eq!(
            result1_1[i].position, result2_1[i].position,
            "Layer 1 quad {}: position should match",
            i
        );
        assert_eq!(
            result1_1[i].fg_color, result2_1[i].fg_color,
            "Layer 1 quad {}: fg_color should match",
            i
        );
    }

    for i in 0..result1_2.len() {
        assert_eq!(
            result1_2[i].position, result2_2[i].position,
            "Layer 2 quad {}: position should match",
            i
        );
    }
}
