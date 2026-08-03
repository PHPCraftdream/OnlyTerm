use crate::parser::ParsedFont;
use crate::units::*;
use config::FontRasterizerSelection;

/// The amount, as a number in [0,1], to horizontally skew a glyph when rendering synthetic
/// italics
pub(crate) const FAKE_ITALIC_SKEW: f64 = 0.2;

pub mod colr;
pub mod colr_paint;
pub mod paint;
/// `FontRasterizer` for non-COLR glyphs on top of `swash::scale` (phase H3
/// of the freetype+harfbuzz -> rustybuzz+swash migration), wired in as
/// the default `FontRasterizerSelection::Swash` option as of phase H3.5.
/// See the module doc comment in `swash.rs` for scope/limitations
/// (notably: it internally delegates COLR/COLRv1/CBDT/sbix color glyphs to
/// `colr_paint::ColrRasterizer`, a pure-Rust COLR paint-graph rasterizer
/// built on `ttf_parser::colr` (phase H4) that replaced the former
/// HarfBuzz-paint-API-based fallback, so `new_rasterizer` below does not
/// need any special-casing for color glyphs).
pub mod swash;

/// A bitmap representation of a glyph.
/// The data is stored as pre-multiplied RGBA 32bpp.
#[derive(Debug)]
pub struct RasterizedGlyph {
    pub data: Vec<u8>,
    pub height: usize,
    pub width: usize,
    pub bearing_x: PixelLength,
    pub bearing_y: PixelLength,
    pub has_color: bool,
    /// if true, glyphcache shouldn't need to scale the
    /// glyph to match metrics
    pub is_scaled: bool,
}

/// Rasterizes the specified glyph index in the associated font
/// and returns the generated bitmap
pub trait FontRasterizer {
    fn rasterize_glyph(
        &self,
        glyph_pos: u32,
        size: f64,
        dpi: u32,
    ) -> anyhow::Result<RasterizedGlyph>;
}

pub fn new_rasterizer(
    rasterizer: FontRasterizerSelection,
    handle: &ParsedFont,
    pixel_geometry: config::DisplayPixelGeometry,
) -> anyhow::Result<Box<dyn FontRasterizer>> {
    match rasterizer {
        // `SwashRasterizer` doesn't support LCD/subpixel targets yet (see
        // its module doc comment); it always renders grayscale alpha
        // masks, so `pixel_geometry` isn't needed here.
        FontRasterizerSelection::Swash => {
            Ok(Box::new(swash::SwashRasterizer::from_locator(handle)?))
        }
        FontRasterizerSelection::FreeType => {
            let _ = pixel_geometry;
            anyhow::bail!(
                "The FreeType rasterizer (and the vendored freetype C library backing it) has \
                 been removed; use the default Swash rasterizer instead"
            );
        }
        FontRasterizerSelection::Harfbuzz => {
            anyhow::bail!(
                "The Harfbuzz paint rasterizer (and the vendored harfbuzz C++ library backing \
                 it) has been removed; use the default Swash rasterizer instead, which now \
                 renders COLR/COLRv1 color glyphs via colr_paint::ColrRasterizer"
            );
        }
    }
}
