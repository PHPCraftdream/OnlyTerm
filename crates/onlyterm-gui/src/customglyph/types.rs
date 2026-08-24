use onlyterm_font::units::IntPixelLength;
use tiny_skia::{FillRule, Paint, Path, PathBuilder, PixmapMut, Stroke, Transform};
use window::Size;

#[derive(Copy, Clone, Eq, PartialEq)]
pub enum PolyAA {
    AntiAlias,
    MoarPixels,
}

bitflags::bitflags! {
    //  ╭──────╮
    //  │UL╱╲UR│
    //  │ ╱  ╲ │
    //  │╱    ╲│
    //  │╲    ╱│
    //  │ ╲  ╱ │
    //  │LL╲╱LR│
    //  ╰──────╯
    pub struct CellDiagonal: u8{
        const UPPER_LEFT = 1<<1;
        const UPPER_RIGHT = 1<<2;
        const LOWER_LEFT = 1<<3;
        const LOWER_RIGHT = 1<<4;
    }
}

bitflags::bitflags! {
    // ╭────╮
    // │╲U ╱│
    // │ ╲╱R│
    // │L╱╲ │
    // │╱ D╲│
    // ╰────╯
    pub struct Triangle: u8{
        const UPPER = 1<<1;
        const RIGHT = 1<<2;
        const LOWER = 1<<3;
        const LEFT = 1<<4;
    }
}

bitflags::bitflags! {
    pub struct ProgressChunk: u8{
        const LEFT = 1<<1;
        const RIGHT = 1<<2;
        const MIDDLE = 1<<3;
        const FULL = 1<<4;
    }
}

bitflags::bitflags! {
    /// Components to make up graph branch diagrams (eg. for git history)
    pub struct Branch: u16{
        const VERTICAL = 1<<1;
        const HORIZONTAL = 1<<2;
        /// 
        const RIGHT_TO_DOWN = 1<<3;
        /// 
        const RIGHT_TO_UP = 1<<4;
        /// 
        const LEFT_TO_DOWN = 1<<5;
        /// 
        const LEFT_TO_UP = 1<<6;
        /// 
        const CIRCLE_FILLED = 1<<7;
        /// 
        const CIRCLE_OUTLINE = 1<<8;
        const LEFT = 1<<9;
        const RIGHT = 1<<10;
        const UP = 1<<11;
        const DOWN = 1<<12;
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum BlockAlpha {
    /// 100%
    Full,
    /// 75%
    Dark,
    /// 50%
    Medium,
    /// 25%
    Light,
}

impl BlockAlpha {
    pub fn to_scale(self) -> f32 {
        match self {
            BlockAlpha::Full => 1.0,
            BlockAlpha::Dark => 0.75,
            BlockAlpha::Medium => 0.5,
            BlockAlpha::Light => 0.25,
        }
    }
}

/// Represents a scaled width of the underline thickness.
/// Can either multiple or divide by the specified amount
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum LineScale {
    Mul(i8),
    Div(i8),
}

impl LineScale {
    fn to_scale(self) -> f32 {
        match self {
            Self::Mul(n) => n as f32,
            Self::Div(n) => 1. / n as f32,
        }
    }
}

/// Represents a coordinate in a glyph expressed in relation
/// to the dimension of the glyph.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum BlockCoord {
    /// 0 pixels in; either the leftmost or topmost pixel position
    Zero,
    /// 100% of the dimension; either the rightmost or bottom pixel
    /// position
    One,
    /// A fraction of the width/height.  The first value is the
    /// numerator, the second is the denominator.
    Frac(i8, i8),

    /// Like Frac() above, but also specifies a scale to use
    /// together with the underline height to adjust the position.
    /// This is helpful because the line drawing routines stroke
    /// along the center of the line in the direction of the line,
    /// but don't pad the end of the line out by the width automatically.
    /// zeno has Cap::Square to specify that, but we can't use it
    /// directly and it isn't necessarily the adjustment that we want.
    /// This is most useful when joining lines that have different
    /// stroke widths; if the widths were all the same then you'd
    /// just specify the points in the path and not worry about it.
    FracWithOffset(i8, i8, LineScale),
    /// Like Zero but for a square centered in the glyph
    SquareZero,
    /// Like One but for a square centered in the glyph
    SquareOne,
    /// Like Frac but for a square centered in the glyph
    SquareFrac(i8, i8),
}

impl BlockCoord {
    /// Compute the actual pixel value given the max dimension.
    pub fn to_pixel(self, max: usize, underline_height: f32, max_square: usize) -> f32 {
        /// For interior points, adjust so that we get the middle of the row;
        /// in AA modes with 1px wide strokes this gives better results.
        fn hint(v: f32) -> f32 {
            if v.fract() == 0. {
                v - 0.5
            } else {
                v
            }
        }
        match self {
            Self::Zero => 0.,
            Self::One => max as f32,
            Self::Frac(num, den) => hint(max as f32 * num as f32 / den as f32),
            Self::FracWithOffset(num, den, under) => {
                hint((max as f32 * num as f32 / den as f32) + (underline_height * under.to_scale()))
            }
            Self::SquareZero => 0. + (max - max_square) as f32 / 2.0,
            Self::SquareOne => max as f32 - (max - max_square) as f32 / 2.0,
            Self::SquareFrac(num, den) => {
                hint(max_square as f32 * num as f32 / den as f32 + (max - max_square) as f32 / 2.0)
            }
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
// Variant names mirror the Unicode block-element glyph names (e.g. "UPPER
// HALF BLOCK"); "Block" is the domain term, not redundant boilerplate.
#[allow(clippy::enum_variant_names)]
pub enum Block {
    /// Number of 1/8ths: x0, x1, y0, y1 with custom alpha
    Custom(u8, u8, u8, u8, BlockAlpha),
    /// Number of 1/8ths in the upper half
    UpperBlock(u8),
    /// Number of 1/8ths in the lower half
    LowerBlock(u8),
    /// Number of 1/8ths in the left half
    LeftBlock(u8),
    /// Number of 1/8ths in the right half
    RightBlock(u8),
    /// Number of 1/8ths: x0, x1
    VerticalBlock(u8, u8),
    /// Number of 1/8ths: y0, y1
    HorizontalBlock(u8, u8),
    /// Quadrants
    // ╭──┬──╮
    // │UL│UR│
    // ├──┼──┤
    // │LL│LR│
    // ╰──┴──╯
    QuadrantUL,
    QuadrantUR,
    QuadrantLL,
    QuadrantLR,
}

/// Represents a Block Element glyph, decoded from
/// <https://en.wikipedia.org/wiki/Block_Elements>
/// <https://www.unicode.org/charts/PDF/U2580.pdf>
/// <https://unicode.org/charts/PDF/U1FB00.pdf>
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum BlockKey {
    /// List of block rectangles
    Blocks(&'static [Block]),
    /// List of triangles
    Triangles(Triangle, BlockAlpha),
    /// A combination of small diagonal lines
    CellDiagonals(CellDiagonal),
    /// A sextant block pattern
    Sextant(u8),
    /// An octant block pattern
    Octant(u8),
    /// A braille dot pattern
    Braille(u8),
    /// A progress bar pattern
    Progress(ProgressChunk),
    /// A graph branch pattern
    Branches(Branch),
    Spinner(u8),

    Poly(&'static [Poly]),

    PolyWithCustomMetrics {
        polys: &'static [Poly],
        underline_height: IntPixelLength,
        cell_size: Size,
    },
}

/// Filled polygon used to describe the more complex shapes in
/// <https://unicode.org/charts/PDF/U1FB00.pdf>
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct Poly {
    pub path: &'static [PolyCommand],
    pub intensity: BlockAlpha,
    pub style: PolyStyle,
}

pub type BlockPoint = (BlockCoord, BlockCoord);

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum PolyCommand {
    MoveTo(BlockCoord, BlockCoord),
    LineTo(BlockCoord, BlockCoord),
    QuadTo {
        control: BlockPoint,
        to: BlockPoint,
    },
    Oval {
        center: BlockPoint,
        radiuses: BlockPoint,
    },
    Circle {
        center: BlockPoint,
        radius: BlockCoord,
    },
    Close,
}

impl PolyCommand {
    pub(super) fn to_skia(
        self,
        width: usize,
        height: usize,
        underline_height: f32,
        pb: &mut PathBuilder,
    ) {
        match self {
            Self::MoveTo(x, y) => pb.move_to(
                x.to_pixel(width, underline_height, width.min(height)),
                y.to_pixel(height, underline_height, width.min(height)),
            ),
            Self::LineTo(x, y) => pb.line_to(
                x.to_pixel(width, underline_height, width.min(height)),
                y.to_pixel(height, underline_height, width.min(height)),
            ),
            Self::QuadTo {
                control: (x1, y1),
                to: (x, y),
            } => pb.quad_to(
                x1.to_pixel(width, underline_height, width.min(height)),
                y1.to_pixel(height, underline_height, width.min(height)),
                x.to_pixel(width, underline_height, width.min(height)),
                y.to_pixel(height, underline_height, width.min(height)),
            ),
            Self::Oval {
                center: (x, y),
                radiuses: (w, h),
            } => {
                let x = x.to_pixel(width, underline_height, width.min(height)) - width as f32;
                let y = y.to_pixel(height, underline_height, width.min(height)) - height as f32;
                let w = w.to_pixel(width, underline_height, width.min(height)) * 2.0;
                let h = h.to_pixel(height, underline_height, width.min(height)) * 2.0;

                if let Some(oval) = tiny_skia::Rect::from_xywh(x, y, w, h) {
                    pb.push_oval(oval);
                } else {
                    log::error!("Can't push oval, values: {:?}", self);
                }
            }
            Self::Circle {
                center: (x, y),
                radius: r,
            } => {
                let x = x.to_pixel(width, underline_height, width.min(height));
                let y = y.to_pixel(height, underline_height, width.min(height));
                let r = r.to_pixel(width.min(height), underline_height, width.min(height));

                pb.push_circle(x, y, r);
            }
            Self::Close => pb.close(),
        };
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum PolyStyle {
    Fill,
    OutlineAlpha,
    OutlineThin,
    // A line with the thickness as underlines
    Outline,
    // A line with twice the thickness of underlines
    OutlineHeavy,
}

impl PolyStyle {
    pub(super) fn apply(self, width: f32, paint: &Paint, path: &Path, pixmap: &mut PixmapMut) {
        match self {
            PolyStyle::Fill => {
                pixmap.fill_path(path, paint, FillRule::Winding, Transform::identity(), None);
            }

            PolyStyle::OutlineThin
            | PolyStyle::Outline
            | PolyStyle::OutlineHeavy
            | PolyStyle::OutlineAlpha => {
                let mut stroke = Stroke {
                    width,
                    ..Default::default()
                };
                if self == PolyStyle::OutlineHeavy {
                    stroke.width *= 3.01; // NOTE: Changing this makes block cursor disproportionate at different font sizes and resolutions
                } else if self == PolyStyle::OutlineThin {
                    stroke.width = 1.2;
                } else if self == PolyStyle::OutlineAlpha {
                    stroke.width = 0.25; // NOTE: This is for filling antialiased border between triangles when using the alpha style
                }

                pixmap.stroke_path(path, paint, &stroke, Transform::identity(), None);
            }
        }
    }
}
