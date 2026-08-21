use crate::renderstate::BorrowedLayers;
use ::window::bitmaps::TextureRect;
use ::window::color::LinearRgba;
use config::HsbTransform;
use std::ops::Deref;

// `QuadInstance`/`CornerVertex` (and the corner-ordering constants) live in the
// `wezterm-gpu-render` crate now -- they are the GPU wire format shared with the
// `--gpu-tab-host` child process -- and are re-exported here so this crate's
// existing `crate::quad::...` paths keep working.
pub use wezterm_gpu_render::{
    QuadInstance, VERTICES_PER_CELL, V_BOT_LEFT, V_BOT_RIGHT, V_TOP_LEFT, V_TOP_RIGHT,
};

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
    assert_eq!(std::mem::size_of::<wezterm_gpu_render::CornerVertex>(), 8);
}

/// Test that HeapQuadAllocator::apply_to now preserves ALL quads even when
/// the initial capacity is exceeded. The old bug (task #453) silently dropped
/// overflow quads via capacity clamps; this test confirms that all quads
/// survive after the fix.
///
/// This test calls the REAL production code: HeapQuadAllocator::apply_to -> BorrowedLayers::extend_from_slice.
/// It would fail if BorrowedLayers::extend_from_slice still had truncation logic.
#[cfg(test)]
#[test]
fn test_apply_to_preserves_all_quads_beyond_capacity() {
    use crate::quad::TripleLayerQuadAllocator;
    use crate::renderstate::BorrowedLayers;

    // Build a real TripleVertexBuffer with capacity 2 (small by design)
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
    heap.apply_to(&mut dest).unwrap();

    // Extract the result back out to verify ALL quads survived
    let TripleLayerQuadAllocator::Gpu(borrowed_layers) = dest else {
        panic!("Expected Gpu variant");
    };

    // Extract layer 0 from the borrowed layers
    let [layer_view, _, _] = borrowed_layers.layers;

    // Extract the instances from layer 0
    let result = layer_view.into_instances();

    // The destination must have ALL 5 instances, not just capacity (2)
    // This proves the old truncation logic is gone
    assert_eq!(
        result.len(),
        5,
        "All 5 instances should survive, even when exceeding capacity"
    );
    // Verify all quads were copied in order
    assert_eq!(
        result[0].position[0], 0.0,
        "First instance should be position[0]=0.0"
    );
    assert_eq!(
        result[1].position[0], 1.0,
        "Second instance should be position[0]=1.0"
    );
    assert_eq!(
        result[2].position[0], 2.0,
        "Third instance should be position[0]=2.0"
    );
    assert_eq!(
        result[3].position[0], 3.0,
        "Fourth instance should be position[0]=3.0"
    );
    assert_eq!(
        result[4].position[0], 4.0,
        "Fifth instance should be position[0]=4.0"
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
    use crate::renderstate::{BorrowedLayers, TripleVertexBuffer};
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

/// Test the exact scenario from the original bug report: >1024 glyph quads
/// in one frame must all survive. The main content layer is created with
/// capacity 1024 (see RenderLayer::new in renderstate.rs), and a dense
/// terminal screen can easily need 1500-2500+ glyph quads. This test pins
/// that fix.
#[test]
fn test_apply_to_glyphs_exceed_1024_capacity() {
    use crate::quad::TripleLayerQuadAllocator;
    use crate::renderstate::BorrowedLayers;

    // Build a TripleVertexBuffer with exactly the content layer's initial capacity
    let vb = crate::renderstate::TripleVertexBuffer::new(vec![], 1024);

    let view0 = vb.map_instances();
    let view1 = vb.map_instances();
    let view2 = vb.map_instances();
    let borrowed_layers = BorrowedLayers {
        layers: [view0, view1, view2],
    };
    let mut dest = TripleLayerQuadAllocator::Gpu(borrowed_layers);

    // Build a HeapQuadAllocator with 1500 quads in layer 1 (glyphs)
    // - This exceeds the 1024 capacity by a significant margin
    // - Simulates a dense terminal screen with many non-blank cells
    let mut heap = HeapQuadAllocator::default();
    for i in 0..1500 {
        let mut q = QuadInstance::default();
        q.position[0] = i as f32;
        heap.layer1.push(q);
    }
    assert_eq!(heap.layer1.len(), 1500);

    heap.apply_to(&mut dest).unwrap();

    let TripleLayerQuadAllocator::Gpu(borrowed_layers) = dest else {
        panic!("Expected Gpu variant");
    };
    let [_, layer_view, _] = borrowed_layers.layers;
    let result = layer_view.into_instances();

    // ALL 1500 instances must survive
    assert_eq!(
        result.len(),
        1500,
        "All 1500 glyph quads should survive beyond the 1024 initial capacity"
    );
    // Spot-check a few values to ensure order and content are correct
    assert_eq!(result[0].position[0], 0.0);
    assert_eq!(result[511].position[0], 511.0);
    assert_eq!(result[1023].position[0], 1023.0);
    assert_eq!(result[1024].position[0], 1024.0);
    assert_eq!(result[1499].position[0], 1499.0);
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
