use crate::renderstate::BorrowedLayers;
use ::window::bitmaps::TextureRect;
use ::window::color::LinearRgba;
use config::HsbTransform;

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
        const ATTRIBS: [wgpu::VertexAttribute; 7] = wgpu::vertex_attr_array![
            0 => Float32x4,
            1 => Float32x4,
            2 => Float32x4,
            3 => Float32x4,
            4 => Float32x3,
            5 => Float32,
            6 => Float32,
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

pub enum QuadImpl<'a> {
    Vert(Quad<'a>),
    Boxed(&'a mut BoxedQuad),
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
}

/// We prefer to allocate a quad at a time for HeapQuadAllocator
/// because we tend to end up with fairly large arrays of Vertex
/// and the total amount of contiguous memory is in the MB range,
/// which is a bit gnarly to reallocate, and can waste several MB
/// in unused capacity
#[derive(Default)]
pub struct BoxedQuad {
    position: (f32, f32, f32, f32),
    fg_color: [f32; 4],
    alt_color: [f32; 4],
    tex: (f32, f32, f32, f32),
    hsv: [f32; 3],
    has_color: f32,
    mix_value: f32,
}

impl QuadTrait for BoxedQuad {
    fn set_texture_discrete(&mut self, x1: f32, x2: f32, y1: f32, y2: f32) {
        self.tex = (x1, x2, y1, y2);
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
        self.position = (left, top, right, bottom);
    }
}

impl BoxedQuad {
    /// Convert to QuadInstance (GPU wire format) for instanced rendering.
    fn to_quad_instance(&self) -> QuadInstance {
        let (left, top, right, bottom) = self.position;
        let (x1, x2, y1, y2) = self.tex;
        QuadInstance {
            position: [left, top, right, bottom],
            tex: [x1, x2, y1, y2],
            fg_color: self.fg_color,
            alt_color: self.alt_color,
            hsv: self.hsv,
            has_color: self.has_color,
            mix_value: self.mix_value,
        }
    }
}

#[derive(Default)]
// `vec_box` wants `Vec<BoxedQuad>` here. That is exactly the layout
// `BoxedQuad`'s own doc comment above explains this code is avoiding: a
// single contiguous allocation in the megabyte range, gnarly to grow and
// prone to wasting several MB in unused capacity. The extra indirection is
// the point, not an oversight.
#[allow(clippy::vec_box)]
pub struct HeapQuadAllocator {
    layer0: Vec<Box<BoxedQuad>>,
    layer1: Vec<Box<BoxedQuad>>,
    layer2: Vec<Box<BoxedQuad>>,
}

impl std::fmt::Debug for HeapQuadAllocator {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        fmt.debug_struct("HeapQuadAllocator").finish()
    }
}

impl HeapQuadAllocator {
    pub fn apply_to(&self, other: &mut TripleLayerQuadAllocator) -> anyhow::Result<()> {
        let start = std::time::Instant::now();
        for (layer_num, quads) in [(0, &self.layer0), (1, &self.layer1), (2, &self.layer2)] {
            for quad in quads {
                // Write instances directly instead of expanding to vertices
                other.extend_with_instance(layer_num, quad.to_quad_instance());
            }
        }
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

        quads.push(Box::new(BoxedQuad::default()));

        let quad = quads.last_mut().unwrap();
        Ok(QuadImpl::Boxed(quad))
    }

    fn extend_with_instance(&mut self, layer_num: usize, instance: QuadInstance) {
        // Convert QuadInstance back to BoxedQuad for storage in the heap allocator
        let (left, top, right, bottom) = (
            instance.position[0],
            instance.position[1],
            instance.position[2],
            instance.position[3],
        );
        let (x1, x2, y1, y2) = (
            instance.tex[0],
            instance.tex[1],
            instance.tex[2],
            instance.tex[3],
        );

        let boxed = BoxedQuad {
            position: (left, top, right, bottom),
            tex: (x1, x2, y1, y2),
            fg_color: instance.fg_color,
            alt_color: instance.alt_color,
            hsv: instance.hsv,
            has_color: instance.has_color,
            mix_value: instance.mix_value,
        };

        let dest_quads = match layer_num {
            0 => &mut self.layer0,
            1 => &mut self.layer1,
            2 => &mut self.layer2,
            _ => unreachable!(),
        };
        dest_quads.push(Box::new(boxed));
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
}

#[cfg(test)]
#[test]
fn size() {
    // Old: 4 vertices per quad, each 68 bytes = 272 bytes per quad
    assert_eq!(std::mem::size_of::<Vertex>() * VERTICES_PER_CELL, 272);
    // BoxedQuad is still 84 bytes (unchanged, still used for heap allocator)
    assert_eq!(std::mem::size_of::<BoxedQuad>(), 84);
    // QuadInstance is the GPU instance format, also 84 bytes
    assert_eq!(std::mem::size_of::<QuadInstance>(), 84);
    // CornerVertex is 8 bytes (2 f32s)
    assert_eq!(std::mem::size_of::<CornerVertex>(), 8);
}
