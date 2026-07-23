use crate::hbwrap::{
    hb_color, hb_color_get_alpha, hb_color_get_blue, hb_color_get_green, hb_color_get_red,
    hb_color_t, hb_paint_composite_mode_t, hb_tag_to_string, DrawOp as HbDrawOp, Font, PaintOp,
    IS_PNG,
};
use crate::rasterizer::colr::{
    apply_draw_ops_to_context, paint_linear_gradient, paint_radial_gradient, paint_sweep_gradient,
    DrawOp,
};
use crate::rasterizer::paint::Painter;
use crate::rasterizer::FAKE_ITALIC_SKEW;
use crate::units::PixelLength;
use crate::{FontRasterizer, ParsedFont, RasterizedGlyph};
use image::DynamicImage::{ImageLuma8, ImageLumaA8};
use image::GenericImageView;
use tiny_skia::{BlendMode, Color, FilterQuality, IntSize, Pixmap, SpreadMode, Transform};

pub struct HarfbuzzRasterizer {
    font: Font,
}

impl HarfbuzzRasterizer {
    pub fn from_locator(parsed: &ParsedFont) -> anyhow::Result<Self> {
        let mut font = Font::from_locator(&parsed.handle)?;
        font.set_ot_funcs();

        if parsed.synthesize_italic {
            font.set_synthetic_slant(FAKE_ITALIC_SKEW as f32);
        }
        if parsed.synthesize_bold {
            font.set_synthetic_bold(0.02, 0.02, false);
        }

        Ok(Self { font })
    }
}

impl FontRasterizer for HarfbuzzRasterizer {
    fn rasterize_glyph(
        &self,
        glyph_pos: u32,
        size: f64,
        dpi: u32,
    ) -> anyhow::Result<RasterizedGlyph> {
        let pixel_size = (size * dpi as f64 / 72.) as u32;

        let scale = pixel_size as i32 * 64;
        let ppem = pixel_size;

        self.font.set_ppem(ppem, ppem);
        self.font.set_ptem(size as f32);
        self.font.set_font_scale(scale, scale);

        let white = hb_color(0xff, 0xff, 0xff, 0xff);

        let palette_index = 0;
        let ops = self
            .font
            .get_paint_ops_for_glyph(glyph_pos, palette_index, white)?;

        log::trace!("ops: {ops:#?}");

        // Pass 1: dry-run over a `Painter` in bbox-only mode to compute the
        // ink extents. tiny-skia has no `RecordingSurface`/`ink_extents()`
        // equivalent, so the two-pass protocol documented on `Painter` is
        // used instead: replay every op once against a dry-run painter to
        // get a conservative bbox (see "Тонкое место 1" in the migration
        // plan), then replay again for real into a pixmap of that size.
        let mut dry = Painter::new_dry_run();
        dry.scale(1. / 64., -1. / 64.);
        apply_paint_ops(&mut dry, &ops)?;
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
        log::trace!("extents: left={left} top={top} width={width} height={height}");

        // Pass 2: render for real into a pixmap sized to the bbox,
        // translated so that the bbox origin maps to (0, 0) - mirroring
        // cairo's `bounds_adjust` translate + `set_source_surface` +
        // `paint()` dance.
        let mut painter = Painter::new(width, height)?;
        painter.translate(left as f32 * -1., top as f32 * -1.);
        painter.scale(1. / 64., -1. / 64.);
        let has_color = apply_paint_ops(&mut painter, &ops)?;

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

/// Replays a sequence of HarfBuzz paint ops against `painter`, mirroring
/// what `record_to_cairo_surface` used to do against a cairo `Context`.
/// Works in both dry-run (bbox-only) and real rendering modes, since every
/// operation is expressed purely in terms of the `Painter` API. Returns
/// whether the glyph has color (i.e. was painted with something other than
/// solid white), matching the pre-existing `has_color` semantics that
/// determine whether the glyph gets tinted with the terminal's foreground
/// color.
fn apply_paint_ops(painter: &mut Painter, paint_ops: &[PaintOp]) -> anyhow::Result<bool> {
    let mut has_color = false;

    for pop in paint_ops {
        match pop {
            PaintOp::PushTransform {
                xx,
                yx,
                xy,
                yy,
                dx,
                dy,
            } => {
                painter.save();
                painter.transform(Transform::from_row(*xx, *yx, *xy, *yy, *dx, *dy));
            }
            PaintOp::PopTransform => {
                painter.restore();
            }
            PaintOp::PushGlyphClip { glyph: _, draw } => {
                painter.save();
                let draw_ops = hb_draw_ops_to_colr(draw);
                apply_draw_ops_to_context(&draw_ops, painter)?;
                painter.clip();
            }
            PaintOp::PushRectClip {
                xmin,
                ymin,
                ymax,
                xmax,
            } => {
                painter.save();
                painter.new_path();
                painter.move_to(*xmin, *ymin);
                painter.line_to(*xmax, *ymin);
                painter.line_to(*xmax, *ymax);
                painter.line_to(*xmin, *ymax);
                painter.close_path();
                painter.clip();
            }
            PaintOp::PopClip => {
                painter.restore();
            }
            PaintOp::PushGroup => {
                painter.save();
                painter.push_group();
            }
            PaintOp::PopGroup { mode } => {
                painter.pop_group(hb_paint_mode_to_blend_mode(*mode));
                painter.restore();
            }
            PaintOp::PaintSolid {
                is_foreground: _,
                color,
            } => {
                if *color != 0xffffffff {
                    has_color = true;
                }
                let (r, g, b, a) = hb_color_to_rgba_u8(*color);
                painter.paint_solid(Color::from_rgba8(r, g, b, a));
            }
            PaintOp::PaintLinearGradient {
                x0,
                y0,
                x1,
                y1,
                x2,
                y2,
                color_line,
            } => {
                has_color = true;
                paint_linear_gradient(
                    painter,
                    (*x0).into(),
                    (*y0).into(),
                    (*x1).into(),
                    (*y1).into(),
                    (*x2).into(),
                    (*y2).into(),
                    color_line.clone(),
                )?;
            }
            PaintOp::PaintRadialGradient {
                x0,
                y0,
                r0,
                x1,
                y1,
                r1,
                color_line,
            } => {
                has_color = true;
                paint_radial_gradient(
                    painter,
                    (*x0).into(),
                    (*y0).into(),
                    (*r0).into(),
                    (*x1).into(),
                    (*y1).into(),
                    (*r1).into(),
                    color_line.clone(),
                )?;
            }
            PaintOp::PaintSweepGradient {
                x0,
                y0,
                start_angle,
                end_angle,
                color_line,
            } => {
                has_color = true;
                paint_sweep_gradient(
                    painter,
                    (*x0).into(),
                    (*y0).into(),
                    (*start_angle).into(),
                    (*end_angle).into(),
                    color_line.clone(),
                )?;
            }
            PaintOp::PaintImage {
                image,
                width: _,
                height: _,
                format,
                slant,
                extents,
            } => {
                if *format != IS_PNG {
                    anyhow::bail!("NOT IMPL: PaintImage {}", hb_tag_to_string(*format));
                }

                let decoded = image::ImageReader::new(std::io::Cursor::new(image.as_slice()))
                    .with_guessed_format()?
                    .decode()?;

                if !matches!(&decoded, ImageLuma8(_) | ImageLumaA8(_)) {
                    // Not a monochrome image
                    has_color = true;
                }

                let (img_width, img_height) = decoded.dimensions();
                let mut data = decoded.into_rgba8().into_vec();
                // tiny-skia's `Pixmap` stores premultiplied RGBA (unlike
                // the `image` crate's straight-alpha output), so
                // premultiply in place before handing it to `Pixmap`.
                // Unlike the cairo path's `rgba_to_argb_and_multiply`, no
                // byte-swizzle to ARGB is needed: `Pixmap` keeps RGBA
                // order.
                premultiply_rgba(&mut data);

                let size = IntSize::from_wh(img_width, img_height).ok_or_else(|| {
                    anyhow::anyhow!("invalid PaintImage dimensions {img_width}x{img_height}")
                })?;
                let pixmap = Pixmap::from_vec(data, size).ok_or_else(|| {
                    anyhow::anyhow!("failed to build Pixmap from decoded PNG data")
                })?;

                let extents = (*extents).ok_or_else(|| {
                    anyhow::anyhow!("expected to have extents for non-svg image data")
                })?;

                // `hb_glyph_extents_t`'s fields are `hb_position_t`
                // (26.6-independent, plain integer font units here since
                // these come from `hb_font_get_glyph_extents`, not glyph
                // outlines) - cast up to f32 for use with `Painter`.
                let x_bearing = extents.x_bearing as f32;
                let y_bearing = extents.y_bearing as f32;
                let ext_width = extents.width as f32;
                let ext_height = extents.height as f32;

                painter.save();
                // Ensure that we clip to the image rectangle.
                painter.new_path();
                painter.move_to(x_bearing, y_bearing);
                painter.line_to(x_bearing + ext_width, y_bearing);
                painter.line_to(x_bearing + ext_width, y_bearing + ext_height);
                painter.line_to(x_bearing, y_bearing + ext_height);
                painter.close_path();
                painter.clip();

                let slanted_width = ext_width as f64 - ext_height as f64 * *slant as f64;
                let slanted_x_bearing = x_bearing as f64 - y_bearing as f64 * *slant as f64;

                // Mirrors the cairo path's
                // `context.transform(Matrix::new(1., 0., slant, 1., 0., 0.))`
                // followed by `translate(slanted_x_bearing, y_bearing)` and
                // `scale(slanted_width, height)`, which together map the
                // unit square onto the (slant-adjusted) glyph image
                // extents.
                painter.transform(Transform::from_row(1., 0., *slant, 1., 0., 0.));
                painter.translate(slanted_x_bearing as f32, y_bearing);
                painter.scale(slanted_width as f32, ext_height);

                // The pattern's own transform maps the pixmap's pixel grid
                // down onto that same unit square (mirroring the cairo
                // path's `pattern.set_matrix(Matrix::new(width, 0, 0,
                // height, 0, 0))`, which is the inverse scale of what we
                // want the *pattern* to do to its own coordinate space).
                let pattern_transform = Transform::from_scale(
                    1.0 / pixmap.width() as f32,
                    1.0 / pixmap.height() as f32,
                );
                let shader = tiny_skia::Pattern::new(
                    pixmap.as_ref(),
                    SpreadMode::Pad,
                    FilterQuality::Bilinear,
                    1.0,
                    pattern_transform,
                );
                painter.paint_shader(shader);
                painter.restore();
            }
        }
    }

    Ok(has_color)
}

/// Converts HarfBuzz's own `DrawOp` (from `hbwrap`) into `colr::DrawOp`
/// (from `rasterizer::colr`), so that the shared
/// `apply_draw_ops_to_context` helper (already ported to `Painter` as part
/// of phase C) can be reused verbatim for glyph clip paths coming from the
/// HarfBuzz paint API.
fn hb_draw_ops_to_colr(ops: &[HbDrawOp]) -> Vec<DrawOp> {
    ops.iter()
        .map(|op| match *op {
            HbDrawOp::MoveTo { to_x, to_y } => DrawOp::MoveTo { to_x, to_y },
            HbDrawOp::LineTo { to_x, to_y } => DrawOp::LineTo { to_x, to_y },
            HbDrawOp::QuadTo {
                control_x,
                control_y,
                to_x,
                to_y,
            } => DrawOp::QuadTo {
                control_x,
                control_y,
                to_x,
                to_y,
            },
            HbDrawOp::CubicTo {
                control1_x,
                control1_y,
                control2_x,
                control2_y,
                to_x,
                to_y,
            } => DrawOp::CubicTo {
                control1_x,
                control1_y,
                control2_x,
                control2_y,
                to_x,
                to_y,
            },
            HbDrawOp::ClosePath => DrawOp::ClosePath,
        })
        .collect()
}

/// Premultiplies straight-alpha RGBA pixel data (as produced by the
/// `image` crate's `into_rgba8()`) in place, matching the format
/// `tiny_skia::Pixmap` requires. Replaces the cairo-era
/// `rgba_to_argb_and_multiply`, which additionally byte-swizzled into
/// cairo's native `ARgb32` (BGRA-on-little-endian) layout; tiny-skia's
/// `Pixmap` keeps RGBA byte order, so only the premultiplication step
/// remains necessary.
fn premultiply_rgba(data: &mut [u8]) {
    for pixel in data.chunks_exact_mut(4) {
        let [r, g, b, a] = *pixel else {
            unreachable!()
        };
        if a != 0xff {
            pixel[0] = multiply_alpha(a, r);
            pixel[1] = multiply_alpha(a, g);
            pixel[2] = multiply_alpha(a, b);
        }
    }
}

fn multiply_alpha(alpha: u8, color: u8) -> u8 {
    let temp: u32 = alpha as u32 * (color as u32 + 0x80);
    ((temp + (temp >> 8)) >> 8) as u8
}

/// Retained only because `freetype.rs` (not yet ported to `Painter` - see
/// phase E) still imports and calls this on cairo's `ARgb32`-formatted
/// `ImageSurface` data. Once phase E ports `freetype.rs` to `Painter` as
/// well, this function (and its cairo-specific byte-order assumptions) can
/// be removed entirely.
pub fn argb_to_rgba(data: &mut [u8]) {
    for pixel in data.chunks_exact_mut(4) {
        #[cfg(target_endian = "little")]
        let [b, g, r, a] = *pixel
        else {
            unreachable!()
        };
        #[cfg(target_endian = "big")]
        let [a, r, g, b] = *pixel
        else {
            unreachable!()
        };
        pixel.copy_from_slice(&[r, g, b, a]);
    }
}

/// Maps a HarfBuzz paint composite mode (`hb_paint_composite_mode_t`, the
/// same 29-variant set as CSS/Porter-Duff compositing operators) onto the
/// equivalent `tiny_skia::BlendMode`. This is a 1:1 mapping - tiny-skia's
/// `BlendMode` covers exactly the same operators (including the HSL blend
/// modes) that cairo's `Operator` did, so this replaces
/// `hb_paint_mode_to_operator` with no loss of fidelity.
fn hb_paint_mode_to_blend_mode(mode: hb_paint_composite_mode_t) -> BlendMode {
    use hb_paint_composite_mode_t::*;
    match mode {
        HB_PAINT_COMPOSITE_MODE_CLEAR => BlendMode::Clear,
        HB_PAINT_COMPOSITE_MODE_SRC => BlendMode::Source,
        HB_PAINT_COMPOSITE_MODE_DEST => BlendMode::Destination,
        HB_PAINT_COMPOSITE_MODE_SRC_OVER => BlendMode::SourceOver,
        HB_PAINT_COMPOSITE_MODE_DEST_OVER => BlendMode::DestinationOver,
        HB_PAINT_COMPOSITE_MODE_SRC_IN => BlendMode::SourceIn,
        HB_PAINT_COMPOSITE_MODE_DEST_IN => BlendMode::DestinationIn,
        HB_PAINT_COMPOSITE_MODE_SRC_OUT => BlendMode::SourceOut,
        HB_PAINT_COMPOSITE_MODE_DEST_OUT => BlendMode::DestinationOut,
        HB_PAINT_COMPOSITE_MODE_SRC_ATOP => BlendMode::SourceAtop,
        HB_PAINT_COMPOSITE_MODE_DEST_ATOP => BlendMode::DestinationAtop,
        HB_PAINT_COMPOSITE_MODE_XOR => BlendMode::Xor,
        HB_PAINT_COMPOSITE_MODE_PLUS => BlendMode::Plus,
        HB_PAINT_COMPOSITE_MODE_SCREEN => BlendMode::Screen,
        HB_PAINT_COMPOSITE_MODE_OVERLAY => BlendMode::Overlay,
        HB_PAINT_COMPOSITE_MODE_DARKEN => BlendMode::Darken,
        HB_PAINT_COMPOSITE_MODE_LIGHTEN => BlendMode::Lighten,
        HB_PAINT_COMPOSITE_MODE_COLOR_DODGE => BlendMode::ColorDodge,
        HB_PAINT_COMPOSITE_MODE_COLOR_BURN => BlendMode::ColorBurn,
        HB_PAINT_COMPOSITE_MODE_HARD_LIGHT => BlendMode::HardLight,
        HB_PAINT_COMPOSITE_MODE_SOFT_LIGHT => BlendMode::SoftLight,
        HB_PAINT_COMPOSITE_MODE_DIFFERENCE => BlendMode::Difference,
        HB_PAINT_COMPOSITE_MODE_EXCLUSION => BlendMode::Exclusion,
        HB_PAINT_COMPOSITE_MODE_MULTIPLY => BlendMode::Multiply,
        HB_PAINT_COMPOSITE_MODE_HSL_HUE => BlendMode::Hue,
        HB_PAINT_COMPOSITE_MODE_HSL_SATURATION => BlendMode::Saturation,
        HB_PAINT_COMPOSITE_MODE_HSL_COLOR => BlendMode::Color,
        HB_PAINT_COMPOSITE_MODE_HSL_LUMINOSITY => BlendMode::Luminosity,
    }
}

fn hb_color_to_rgba_u8(color: hb_color_t) -> (u8, u8, u8, u8) {
    let red = unsafe { hb_color_get_red(color) };
    let green = unsafe { hb_color_get_green(color) };
    let blue = unsafe { hb_color_get_blue(color) };
    let alpha = unsafe { hb_color_get_alpha(color) };
    (red, green, blue, alpha)
}
