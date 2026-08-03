//! Pure-Rust COLR/COLRv1 paint-graph rasterizer, built directly on
//! `ttf_parser::colr` instead of HarfBuzz's `hb_paint_funcs_t` paint API
//! (see `rasterizer/harfbuzz.rs`, which this module replaces).
//!
//! This is part of the freetype+harfbuzz -> rustybuzz+swash migration,
//! phase H4. H3's `SwashRasterizer` covers ordinary (non-color) glyph
//! outlines, but swash itself can only detect COLRv0
//! (`Scaler::has_color_outlines`) -- it has no COLRv1 paint-graph walker
//! at all, and even its COLRv0 support is limited to querying whether a
//! color glyph exists, not painting one. `ttf-parser` 0.25 (already in
//! the dependency graph via `rustybuzz`) ships a complete, pure-Rust
//! COLRv0 *and* COLRv1 implementation, including the paint graph,
//! gradients (linear/radial/sweep), composite modes, and clip paths --
//! exposed as `ttf_parser::Face::paint_color_glyph`, which walks the
//! graph and drives a caller-supplied `ttf_parser::colr::Painter` trait
//! object.
//!
//! This module implements that trait (`ColrGraphPainter`) as an adapter
//! onto the existing `crate::rasterizer::paint::Painter` (tiny-skia
//! based) and the gradient math already ported from HarfBuzz/
//! BlackRenderer in `rasterizer/colr.rs` (`paint_linear_gradient`/
//! `paint_radial_gradient`/`paint_sweep_gradient`, `ColorLine`/
//! `ColorStop`/`DrawOp`). None of that math is HarfBuzz-specific -- it
//! operates purely in terms of this crate's own types -- so it is reused
//! verbatim; only the *traversal* that used to come from
//! `hb_font_get_paint_ops_for_glyph` (parsing COLR/CPAL and walking the
//! v0/v1 paint graph) is replaced, by `ttf_parser::colr::Table::paint`.
//!
//! ## Two-pass bbox+render protocol
//!
//! Same as `HarfbuzzRasterizer`: tiny-skia has no `ink_extents()`
//! equivalent, so every glyph is painted twice -- once into a dry-run
//! `Painter` (`Painter::new_dry_run`) purely to compute the ink bbox, and
//! once for real into a `Pixmap` sized to that bbox. `paint_color_glyph`
//! has no side effects of its own (parsing is read-only), so it is simply
//! invoked twice with two different `ColrGraphPainter` instances wrapping
//! two different `crate::rasterizer::paint::Painter`s.
//!
//! ## Coordinate system
//!
//! `ttf_parser::colr`'s paint graph is expressed in the font's design
//! units (unscaled, y-up) -- unlike `hb_font_get_paint_ops_for_glyph`
//! (which returns 26.6 fixed-point pixels once `hb_font_set_scale` has
//! been applied), `ttf_parser` never applies any scale of its own. This
//! module therefore scales by `pixels_per_em / units_per_em` and flips Y
//! (font space is y-up, `Painter`/tiny-skia is y-down) itself, rather
//! than the `1./64.` factor `harfbuzz.rs` uses (which undoes *harfbuzz's*
//! internal fixed-point scale -- a different thing entirely, not
//! applicable here since `ttf_parser` has no such scale to undo).
//!
//! ## Outline callback (`outline_glyph`)
//!
//! `ttf_parser::colr::Painter::outline_glyph(&mut self, glyph_id)` is
//! called by the graph walker to ask us to "outline a glyph and store
//! it" for a subsequent `push_clip()`, but the trait method only receives
//! a `GlyphId`, not a `&Face` to resolve it against. `ColrGraphPainter`
//! is therefore constructed with a reference to the same `Face` that
//! `paint_color_glyph` is being called on, and `outline_glyph` uses
//! `Face::outline_glyph` (the plain glyf/CFF/CFF2 outline accessor) to
//! resolve and store it, exactly mirroring how `harfbuzz.rs`'s
//! `PaintOp::PushGlyphClip { draw, .. }` already carried a pre-resolved
//! draw-op list from HarfBuzz's own equivalent callback.

use crate::rasterizer::colr::{
    apply_draw_ops_to_context, paint_linear_gradient, paint_radial_gradient, paint_sweep_gradient,
    ColorLine, ColorStop, DrawOp,
};
use crate::rasterizer::paint::Painter;
use crate::rasterizer::FAKE_ITALIC_SKEW;
use crate::units::PixelLength;
use crate::{FontRasterizer, ParsedFont, RasterizedGlyph};
use tiny_skia::{BlendMode, Color, SpreadMode, Transform};
use ttf_parser::colr::{ClipBox, CompositeMode, GradientExtend, Paint};
use ttf_parser::{Face, GlyphId, OutlineBuilder, RgbaColor};
use wezterm_color_types::SrgbaPixel;

pub struct ColrRasterizer {
    /// Owned raw font bytes; `ttf_parser::Face<'_>` borrows from this
    /// (like `OwnedRbFace` in `shaper/rustybuzz.rs`), so we re-parse a
    /// short-lived `Face` from it on every call rather than trying to
    /// store a self-referential `Face` alongside its backing bytes.
    data: Box<[u8]>,
    face_index: u32,
    units_per_em: f64,
    synthesize_italic: bool,
}

impl ColrRasterizer {
    pub fn from_locator(parsed: &ParsedFont) -> anyhow::Result<Self> {
        let handle = &parsed.handle;
        let data = handle.source.load_data()?.into_owned().into_boxed_slice();
        let face_index = handle.index;
        let face = Face::parse(&data, face_index)
            .map_err(|e| anyhow::anyhow!("ttf-parser failed to parse font face: {e:?}"))?;
        let units_per_em = face.units_per_em() as f64;

        Ok(Self {
            data,
            face_index,
            units_per_em,
            synthesize_italic: parsed.synthesize_italic,
        })
    }

    fn as_face(&self) -> anyhow::Result<Face<'_>> {
        Face::parse(&self.data, self.face_index)
            .map_err(|e| anyhow::anyhow!("ttf-parser failed to parse font face: {e:?}"))
    }
}

impl FontRasterizer for ColrRasterizer {
    fn rasterize_glyph(
        &self,
        glyph_pos: u32,
        size: f64,
        dpi: u32,
    ) -> anyhow::Result<RasterizedGlyph> {
        let pixel_size = size * dpi as f64 / 72.;
        let scale = (pixel_size / self.units_per_em) as f32;
        let glyph_id = GlyphId(glyph_pos as u16);

        let face = self.as_face()?;

        let base_transform = {
            // Font design space is y-up with an arbitrary unitsPerEm;
            // Painter/tiny-skia is y-down pixel space. Flip Y and scale
            // by pixels-per-design-unit, mirroring the
            // `scale(1./64., -1./64.)` step in harfbuzz.rs (there,
            // undoing HarfBuzz's internal 26.6 scale; here, converting
            // design units straight to pixels).
            let mut t = Transform::from_scale(scale, -scale);
            if self.synthesize_italic {
                // Match HarfbuzzRasterizer/FreeTypeRasterizer's synthetic
                // italic shear (see FAKE_ITALIC_SKEW), applied in the
                // same (unscaled, pre-flip) space as harfbuzz.rs's
                // `font.set_synthetic_slant`.
                t = t.pre_concat(Transform::from_row(
                    1.,
                    0.,
                    FAKE_ITALIC_SKEW as f32,
                    1.,
                    0.,
                    0.,
                ));
            }
            t
        };

        // Pass 1: dry run to compute ink extents.
        let mut dry = Painter::new_dry_run();
        dry.transform(base_transform);
        let mut dry_adapter = ColrGraphPainter::new(&face, &mut dry);
        let painted = face
            .paint_color_glyph(
                glyph_id,
                0,
                RgbaColor::new(255, 255, 255, 255),
                &mut dry_adapter,
            )
            .is_some();

        if !painted {
            anyhow::bail!(
                "ColrRasterizer: glyph {glyph_pos} has no COLR paint definition (v0 or v1)"
            );
        }

        let bbox = dry.bbox();
        if bbox.is_empty() || bbox.width() <= 0. || bbox.height() <= 0. {
            return Ok(RasterizedGlyph {
                data: vec![],
                height: 0,
                width: 0,
                bearing_x: PixelLength::new(0.),
                bearing_y: PixelLength::new(0.),
                has_color: false,
                is_scaled: true,
            });
        }

        let left = bbox.x0 as f64;
        let top = bbox.y0 as f64;
        let width = bbox.width().ceil() as u32;
        let height = bbox.height().ceil() as u32;

        // Pass 2: render for real into a pixmap sized to the bbox.
        let mut painter = Painter::new(width, height)?;
        painter.translate(left as f32 * -1., top as f32 * -1.);
        painter.transform(base_transform);
        let mut adapter = ColrGraphPainter::new(&face, &mut painter);
        face.paint_color_glyph(
            glyph_id,
            0,
            RgbaColor::new(255, 255, 255, 255),
            &mut adapter,
        )
        .ok_or_else(|| anyhow::anyhow!("ColrRasterizer: paint_color_glyph failed on pass 2"))?;
        let has_color = adapter.has_color;

        let pixmap = painter
            .into_pixmap()
            .ok_or_else(|| anyhow::anyhow!("Painter::into_pixmap: expected a real pixmap"))?;
        let data = pixmap.data().to_vec();

        Ok(RasterizedGlyph {
            data,
            height: height as usize,
            width: width as usize,
            bearing_x: PixelLength::new(left.min(0.)),
            bearing_y: PixelLength::new(top * -1.),
            has_color,
            is_scaled: true,
        })
    }
}

/// Adapts `crate::rasterizer::paint::Painter` to implement
/// `ttf_parser::colr::Painter`, the trait `Face::paint_color_glyph` drives
/// while walking the COLRv0/v1 paint graph. This plays the same role that
/// `apply_paint_ops` (a plain function iterating over HarfBuzz's
/// `Vec<PaintOp>`) played in `harfbuzz.rs`, just restructured as a trait
/// impl since `ttf_parser` calls back into us incrementally while
/// recursing through the graph, rather than handing over a flat op list
/// up front.
struct ColrGraphPainter<'a, 'f, 'p> {
    face: &'f Face<'a>,
    painter: &'p mut Painter,
    /// Outline path most recently produced by `outline_glyph`, pending a
    /// `push_clip()` call - mirrors the "outline a glyph and store it,
    /// then a later call consumes the stored outline" contract
    /// documented on `ttf_parser::colr::Painter`.
    pending_outline: Option<Vec<DrawOp>>,
    /// Composite mode passed to the most recent unmatched `push_layer`,
    /// consumed by the matching `pop_layer`.
    pending_layer_mode: Vec<CompositeMode>,
    has_color: bool,
}

impl<'a, 'f, 'p> ColrGraphPainter<'a, 'f, 'p> {
    fn new(face: &'f Face<'a>, painter: &'p mut Painter) -> Self {
        Self {
            face,
            painter,
            pending_outline: None,
            pending_layer_mode: vec![],
            has_color: false,
        }
    }
}

/// Collects `ttf_parser::OutlineBuilder` callbacks into `colr::DrawOp`s,
/// reusing the same op representation `apply_draw_ops_to_context` (from
/// the cairo->tiny-skia migration) already knows how to replay against a
/// `Painter`. `ttf_parser::OutlineBuilder`'s method set is a 1:1 match for
/// `DrawOp`'s variants (move_to/line_to/quad_to/curve_to/close), so this
/// is a direct translation with no coordinate adjustments.
#[derive(Default)]
struct DrawOpCollector {
    ops: Vec<DrawOp>,
}

impl OutlineBuilder for DrawOpCollector {
    fn move_to(&mut self, x: f32, y: f32) {
        self.ops.push(DrawOp::MoveTo { to_x: x, to_y: y });
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.ops.push(DrawOp::LineTo { to_x: x, to_y: y });
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.ops.push(DrawOp::QuadTo {
            control_x: x1,
            control_y: y1,
            to_x: x,
            to_y: y,
        });
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.ops.push(DrawOp::CubicTo {
            control1_x: x1,
            control1_y: y1,
            control2_x: x2,
            control2_y: y2,
            to_x: x,
            to_y: y,
        });
    }

    fn close(&mut self) {
        self.ops.push(DrawOp::ClosePath);
    }
}

fn extend_to_spread_mode(extend: GradientExtend) -> SpreadMode {
    match extend {
        GradientExtend::Pad => SpreadMode::Pad,
        GradientExtend::Repeat => SpreadMode::Repeat,
        GradientExtend::Reflect => SpreadMode::Reflect,
    }
}

fn rgba_to_srgba_pixel(c: RgbaColor) -> SrgbaPixel {
    SrgbaPixel::rgba(c.red, c.green, c.blue, c.alpha)
}

fn color_line_from_stops<I: Iterator<Item = ttf_parser::colr::ColorStop>>(
    stops: I,
    extend: GradientExtend,
) -> ColorLine {
    ColorLine {
        color_stops: stops
            .map(|s| ColorStop {
                offset: s.stop_offset as f64,
                color: rgba_to_srgba_pixel(s.color),
            })
            .collect(),
        extend: extend_to_spread_mode(extend),
    }
}

/// Maps `ttf_parser::colr::CompositeMode` (identical 28-variant set to
/// HarfBuzz's `hb_paint_composite_mode_t`/CSS Porter-Duff compositing
/// operators) onto `tiny_skia::BlendMode`, the same mapping
/// `hb_paint_mode_to_blend_mode` performed in `harfbuzz.rs`.
fn composite_mode_to_blend_mode(mode: CompositeMode) -> BlendMode {
    match mode {
        CompositeMode::Clear => BlendMode::Clear,
        CompositeMode::Source => BlendMode::Source,
        CompositeMode::Destination => BlendMode::Destination,
        CompositeMode::SourceOver => BlendMode::SourceOver,
        CompositeMode::DestinationOver => BlendMode::DestinationOver,
        CompositeMode::SourceIn => BlendMode::SourceIn,
        CompositeMode::DestinationIn => BlendMode::DestinationIn,
        CompositeMode::SourceOut => BlendMode::SourceOut,
        CompositeMode::DestinationOut => BlendMode::DestinationOut,
        CompositeMode::SourceAtop => BlendMode::SourceAtop,
        CompositeMode::DestinationAtop => BlendMode::DestinationAtop,
        CompositeMode::Xor => BlendMode::Xor,
        CompositeMode::Plus => BlendMode::Plus,
        CompositeMode::Screen => BlendMode::Screen,
        CompositeMode::Overlay => BlendMode::Overlay,
        CompositeMode::Darken => BlendMode::Darken,
        CompositeMode::Lighten => BlendMode::Lighten,
        CompositeMode::ColorDodge => BlendMode::ColorDodge,
        CompositeMode::ColorBurn => BlendMode::ColorBurn,
        CompositeMode::HardLight => BlendMode::HardLight,
        CompositeMode::SoftLight => BlendMode::SoftLight,
        CompositeMode::Difference => BlendMode::Difference,
        CompositeMode::Exclusion => BlendMode::Exclusion,
        CompositeMode::Multiply => BlendMode::Multiply,
        CompositeMode::Hue => BlendMode::Hue,
        CompositeMode::Saturation => BlendMode::Saturation,
        CompositeMode::Color => BlendMode::Color,
        CompositeMode::Luminosity => BlendMode::Luminosity,
    }
}

impl<'a, 'f, 'p> ttf_parser::colr::Painter<'a> for ColrGraphPainter<'a, 'f, 'p> {
    fn outline_glyph(&mut self, glyph_id: GlyphId) {
        let mut collector = DrawOpCollector::default();
        // A glyph with no outline at all (e.g. a fully clipped-out layer,
        // or a malformed reference) just yields an empty op list, which
        // `apply_draw_ops_to_context` + `clip()` correctly turns into an
        // empty (everything-clipped) region rather than erroring.
        let _ = self.face.outline_glyph(glyph_id, &mut collector);
        self.pending_outline = Some(collector.ops);
    }

    fn paint(&mut self, paint: Paint<'a>) {
        match paint {
            Paint::Solid(color) => {
                if color.red != 255 || color.green != 255 || color.blue != 255 {
                    self.has_color = true;
                }
                self.painter.paint_solid(Color::from_rgba8(
                    color.red,
                    color.green,
                    color.blue,
                    color.alpha,
                ));
            }
            Paint::LinearGradient(g) => {
                self.has_color = true;
                let color_line = color_line_from_stops(g.stops(0, &[]), g.extend);
                let _ = paint_linear_gradient(
                    self.painter,
                    g.x0 as f64,
                    g.y0 as f64,
                    g.x1 as f64,
                    g.y1 as f64,
                    g.x2 as f64,
                    g.y2 as f64,
                    color_line,
                );
            }
            Paint::RadialGradient(g) => {
                self.has_color = true;
                let color_line = color_line_from_stops(g.stops(0, &[]), g.extend);
                let _ = paint_radial_gradient(
                    self.painter,
                    g.x0 as f64,
                    g.y0 as f64,
                    g.r0 as f64,
                    g.x1 as f64,
                    g.y1 as f64,
                    g.r1 as f64,
                    color_line,
                );
            }
            Paint::SweepGradient(g) => {
                self.has_color = true;
                let color_line = color_line_from_stops(g.stops(0, &[]), g.extend);
                // ttf_parser's sweep angles are in degrees (see the COLR
                // spec's PaintSweepGradient), unlike HarfBuzz's paint API
                // (radians) - convert here so `paint_sweep_gradient`
                // (which expects radians, matching its atan2-based pixel
                // shader) sees the same units regardless of which
                // traversal produced them.
                let _ = paint_sweep_gradient(
                    self.painter,
                    g.center_x as f64,
                    g.center_y as f64,
                    (g.start_angle as f64).to_radians(),
                    (g.end_angle as f64).to_radians(),
                    color_line,
                );
            }
        }
    }

    fn push_clip(&mut self) {
        self.painter.save();
        if let Some(ops) = self.pending_outline.take() {
            self.painter.new_path();
            let _ = apply_draw_ops_to_context(&ops, self.painter);
            self.painter.clip();
        }
    }

    fn push_clip_box(&mut self, clipbox: ClipBox) {
        self.painter.save();
        self.painter.new_path();
        self.painter.move_to(clipbox.x_min, clipbox.y_min);
        self.painter.line_to(clipbox.x_max, clipbox.y_min);
        self.painter.line_to(clipbox.x_max, clipbox.y_max);
        self.painter.line_to(clipbox.x_min, clipbox.y_max);
        self.painter.close_path();
        self.painter.clip();
    }

    fn pop_clip(&mut self) {
        self.painter.restore();
    }

    fn push_layer(&mut self, mode: CompositeMode) {
        self.painter.save();
        self.painter.push_group();
        self.pending_layer_mode.push(mode);
    }

    fn pop_layer(&mut self) {
        let mode = self
            .pending_layer_mode
            .pop()
            .unwrap_or(CompositeMode::SourceOver);
        self.painter.pop_group(composite_mode_to_blend_mode(mode));
        self.painter.restore();
    }

    fn push_transform(&mut self, transform: ttf_parser::Transform) {
        self.painter.save();
        self.painter.transform(Transform::from_row(
            transform.a,
            transform.b,
            transform.c,
            transform.d,
            transform.e,
            transform.f,
        ));
    }

    fn pop_transform(&mut self) {
        self.painter.restore();
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::locator::{FontDataHandle, FontDataSource, FontOrigin};
    use std::path::PathBuf;

    fn asset_path(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("assets/fonts")
            .join(name)
    }

    fn parsed_font_for(name: &str) -> ParsedFont {
        let handle = FontDataHandle {
            source: FontDataSource::OnDisk(asset_path(name)),
            index: 0,
            variation: 0,
            origin: FontOrigin::BuiltIn,
            coverage: None,
        };
        ParsedFont::from_locator(&handle).unwrap()
    }

    #[test]
    fn renders_colr_emoji_glyph() {
        let parsed = parsed_font_for("NotoColorEmoji.ttf");
        let raster = ColrRasterizer::from_locator(&parsed).unwrap();

        let font_info = crate::swash_metrics::SwashFontInfo::from_locator(
            &FontDataSource::OnDisk(asset_path("NotoColorEmoji.ttf")),
            0,
        )
        .unwrap();
        let gid = font_info.glyph_id_for_char('\u{1F600}');
        assert_ne!(gid, 0, "no glyph id for grinning face emoji");

        let glyph = raster.rasterize_glyph(gid as u32, 32.0, 96).unwrap();
        assert!(
            glyph.width > 0 && glyph.height > 0,
            "expected a non-empty rasterized COLR emoji glyph, got {}x{}",
            glyph.width,
            glyph.height
        );
        assert!(
            glyph.has_color,
            "expected the COLR emoji glyph to come back tagged has_color=true"
        );
    }

    #[test]
    fn renders_multiple_colr_emoji_without_panicking() {
        let parsed = parsed_font_for("NotoColorEmoji.ttf");
        let raster = ColrRasterizer::from_locator(&parsed).unwrap();
        let font_info = crate::swash_metrics::SwashFontInfo::from_locator(
            &FontDataSource::OnDisk(asset_path("NotoColorEmoji.ttf")),
            0,
        )
        .unwrap();

        for c in [
            '\u{1F600}',
            '\u{1F44D}',
            '\u{1F308}',
            '\u{1F929}',
            '\u{1F970}',
        ] {
            let gid = font_info.glyph_id_for_char(c);
            assert_ne!(gid, 0, "no glyph id for {c:?}");
            let glyph = raster.rasterize_glyph(gid as u32, 32.0, 96).unwrap();
            assert!(
                glyph.width > 0 && glyph.height > 0,
                "expected non-empty glyph for {c:?}, got {}x{}",
                glyph.width,
                glyph.height
            );
        }
    }
}
