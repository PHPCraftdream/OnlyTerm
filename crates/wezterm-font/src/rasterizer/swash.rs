//! Swash-based rasterizer for **non-COLR** glyphs.
//!
//! This is part of the freetype+harfbuzz -> rustybuzz+swash migration (see
//! `docs/plans/2026-07-23-freetype-harfbuzz-migration.md`, phase H3). H2
//! (`crate::swash_metrics`) already covers font-file parsing/metrics/cmap
//! without touching pixel output; this module is the rendering
//! counterpart: it hints and rasterizes plain (monochrome/grayscale)
//! glyph outlines through `swash::scale`, mirroring the non-COLR path of
//! `rasterizer/freetype.rs` (`FreeTypeRasterizer::rasterize_glyph`'s
//! `FT_Load_Glyph`/`FT_Render_Glyph` call, *not* the
//! `rasterize_outlines`/`Walker` COLRv0/v1 paint-graph path a few
//! hundred lines down in that same file -- that machinery belongs to the
//! separate cairo->tiny-skia initiative and stays untouched here).
//!
//! ## Scope: what this module does and does not cover
//!
//! - **Covered**: ordinary scalable (non-color) TrueType/OpenType/CFF
//!   outlines, hinted and rasterized to an 8-bit grayscale alpha mask,
//!   matching FreeType's `FT_RENDER_MODE_NORMAL` (the default
//!   `FreeTypeLoadTarget::Normal`/`Light`/`Mono` targets all ultimately
//!   produce an `FT_PIXEL_MODE_GRAY` or `FT_PIXEL_MODE_MONO` bitmap for
//!   non-LCD configurations; this module always targets grayscale,
//!   matching the common case -- LCD/subpixel targets are a smaller,
//!   separate concern noted below).
//! - **Not covered** (delegates to `colr_paint::ColrRasterizer`, a
//!   pure-Rust COLR/COLRv1 paint-graph rasterizer built on
//!   `ttf_parser::colr` -- see that module's doc comment; it replaced the
//!   former HarfBuzz-paint-API-based `HarfbuzzRasterizer` in phase H4):
//!   any glyph carrying COLR/COLRv1/CBDT/sbix color data. This module
//!   always tries the plain-outline `Source::Outline` path first (via
//!   `Render`); if that comes back empty (no scalable monochrome outline
//!   at all, or a zero-size result), it checks whether the *font* has any
//!   color capability at all (`Scaler::has_color_outlines()`/
//!   `has_color_bitmaps()`) and if so, delegates to
//!   `colr_paint::ColrRasterizer` rather than returning the empty result
//!   or an error. Fonts with no color capability skip straight to
//!   treating an empty outline as a legitimately blank glyph (space, ZWJ,
//!   etc.), so this doesn't add a COLR-parsing round-trip to the common
//!   case.
//!
//!   Note this can't be a precise *per-glyph* "is this glyph id color"
//!   check: `swash::scale::color::ColorProxy::layers` (backing
//!   `scale_color_outline`/`has_color_outlines`) only understands
//!   COLRv0's simple `BaseGlyphList` layer format. `NotoColorEmoji.ttf`
//!   in this repo's test assets is actually COLRv1 (confirmed: FreeType's
//!   own `ftwrap::Face::load_and_render_glyph` can't render it directly
//!   either, and raises `IsColr1OrLater` for exactly this reason), so a
//!   COLRv1 glyph's `scale_color_outline` call also returns `None` --
//!   indistinguishable, from swash's API alone, from "this glyph
//!   genuinely has no color data". The font-level fallback (delegate
//!   whenever the font has *any* color table and the plain outline is
//!   empty) is the pragmatic way to still catch COLRv1 without swash
//!   exposing a documented "is this glyph id COLRv1" query on this API
//!   surface. See `colr_glyph_falls_back_to_harfbuzz_paint_rasterizer`
//!   in the test module for the regression this guards.
//! - **LCD/subpixel rendering** (`FreeTypeLoadTarget::HorizontalLcd`/
//!   `VerticalLcd`, i.e. `rasterize_lcd`/`rasterize_lcd_v` in
//!   `freetype.rs`): swash's `Format::Subpixel`/`CustomSubpixel` exist and
//!   could support this, but reproducing FreeType's exact LCD filter
//!   (5-tap FIR + gamma) is out of scope for this first pass -- this
//!   module always renders `Format::Alpha` (grayscale), which is also
//!   wezterm's default (`FreeTypeLoadTarget::default()` is `Normal`, and
//!   the LCD targets are opt-in). Revisit if/when this module is promoted
//!   beyond "parallel, not wired up".
//!
//! ## Hinting
//!
//! Every `Scaler` built here uses `.hint(true)`. Internally swash then
//! builds (and caches, via `ScaleContext`'s `HintingCache`) a
//! `skrifa`-based `HintingInstance`, which auto-hints TrueType-bytecode
//! (`glyf`) outlines and applies CFF/PostScript hinting for CFF-flavored
//! outlines -- this is swash's equivalent of FreeType's bytecode
//! interpreter + autohinter combination invoked implicitly by
//! `FT_Load_Glyph`/`FT_Render_Glyph` without `FT_LOAD_NO_HINTING`. There
//! is no dedicated "off" switch exposed here deliberately: disabling
//! hinting would defeat the entire point of this phase (see the plan's
//! "hinting parity" risk -- hinting is what's being compared/ported, not
//! worked around).
//!
//! ## Not a bitwise match, by design
//!
//! Per the plan's explicit acceptance criterion for this phase: the goal
//! is "readable, not worse" visually compared to FreeType, verified with
//! `dump_glyph`/`diff_glyph` at several sizes -- *not* pixel-for-pixel
//! identity (swash's hinter and FreeType's are different implementations
//! of similar ideas and will disagree on individual pixel coverage,
//! especially at small sizes). What **is** required to match exactly:
//! bearing/advance metrics (already covered by H2's
//! `swash_metrics::tests`), so that cell-grid alignment in the terminal
//! is unaffected regardless of which rasterizer produced the bitmap.

use crate::locator::FontDataHandle;
use crate::parser::ParsedFont;
use crate::rasterizer::colr_paint::ColrRasterizer;
use crate::rasterizer::{FontRasterizer, FAKE_ITALIC_SKEW};
use crate::units::*;
use crate::RasterizedGlyph;
use std::cell::RefCell;
use swash::scale::{Render, ScaleContext, Source};
use swash::zeno::{Format, Transform};
use swash::{CacheKey, FontRef};
use wezterm_color_types::linear_u8_to_srgb8;

/// Non-COLR-glyph rasterizer built on `swash::scale`. See the module doc
/// comment for exactly what this does and does not cover.
pub struct SwashRasterizer {
    /// Owned raw font bytes; mirrors `swash_metrics::SwashFontInfo`'s
    /// approach of keeping its own copy rather than sharing
    /// `ftwrap::Face`/`rustybuzz::Face`, so this type has zero shared
    /// state with the FreeType/HarfBuzz code paths (relevant to the
    /// BUG7 thread-safety note in the migration plan, even though that
    /// bug was specifically about vendored harfbuzz's now-fixed
    /// `HB_NO_MT` build flag -- this module simply never touches that
    /// global state in the first place).
    data: Box<[u8]>,
    offset: u32,
    key: CacheKey,
    context: RefCell<ScaleContext>,
    synthesize_bold: bool,
    synthesize_italic: bool,
    scale: f64,
    /// Fallback for glyphs with no plain scalable outline (color
    /// glyphs: COLR/COLRv1/CBDT/sbix). See module doc comment.
    color_fallback: ColrRasterizer,
}

impl SwashRasterizer {
    pub fn from_locator(parsed: &ParsedFont) -> anyhow::Result<Self> {
        log::trace!("SwashRasterizer wants {:?}", parsed);
        let handle: &FontDataHandle = &parsed.handle;
        let data = handle.source.load_data()?.into_owned().into_boxed_slice();
        let font = FontRef::from_index(&data, handle.index as usize).ok_or_else(|| {
            anyhow::anyhow!(
                "swash failed to parse font face at index {}",
                handle.index
            )
        })?;
        let offset = font.offset;
        let key = font.key;

        Ok(Self {
            data,
            offset,
            key,
            context: RefCell::new(ScaleContext::new()),
            synthesize_bold: parsed.synthesize_bold,
            synthesize_italic: parsed.synthesize_italic,
            scale: parsed.scale.unwrap_or(1.),
            color_fallback: ColrRasterizer::from_locator(parsed)?,
        })
    }

    fn as_font_ref(&self) -> FontRef<'_> {
        FontRef {
            data: &self.data,
            offset: self.offset,
            key: self.key,
        }
    }
}

impl FontRasterizer for SwashRasterizer {
    fn rasterize_glyph(
        &self,
        glyph_pos: u32,
        size: f64,
        dpi: u32,
    ) -> anyhow::Result<RasterizedGlyph> {
        let pixel_height = size * self.scale * dpi as f64 / 72.0;
        let ppem = pixel_height as f32;

        let font = self.as_font_ref();
        let mut context = self.context.borrow_mut();

        let mut scaler = context.builder(font).size(ppem).hint(true).build();
        let has_any_color_data = scaler.has_color_outlines() || scaler.has_color_bitmaps();

        let mut render = Render::new(&[Source::Outline]);
        render.format(Format::Alpha);

        // Synthetic bold: swash's `embolden` thickens the outline before
        // rasterization, the same conceptual operation as FreeType's
        // `FT_GlyphSlot_Embolden` (called in
        // `ftwrap::Face::load_and_render_glyph` when `synthesize_bold` is
        // set). FreeType's embolden strength there is expressed in 26.6
        // fractional pixels derived from the face's units-per-em; swash's
        // `embolden` is a flat pixel strength applied post-scaling, so an
        // exact strength match isn't attempted here (matches this phase's
        // "readable, not worse" bar rather than the metrics-exact bar).
        if self.synthesize_bold {
            let strength = (ppem * 0.02).max(0.5);
            render.embolden(strength);
        }

        // Synthetic italic: apply the same horizontal shear FreeType uses
        // (`ftwrap::Face::set_transform`'s `FT_Matrix { xy: FAKE_ITALIC_SKEW, .. }`,
        // consumed by `FreeTypeRasterizer::from_locator`). FreeType's
        // matrix convention is `x' = xx*x + xy*y`; zeno's `Transform`
        // convention (see `Transform::transform_point`) is
        // `x' = xx*x + yx*y`, so the skew factor goes in the `yx` slot
        // here to reproduce the same visual shear.
        let transform = if self.synthesize_italic {
            Some(Transform {
                xx: 1.,
                yx: FAKE_ITALIC_SKEW as f32,
                xy: 0.,
                yy: 1.,
                x: 0.,
                y: 0.,
            })
        } else {
            None
        };
        render.transform(transform);

        let image = render.render(&mut scaler, glyph_pos as u16);

        let width = image.as_ref().map(|i| i.placement.width as usize).unwrap_or(0);
        let height = image.as_ref().map(|i| i.placement.height as usize).unwrap_or(0);

        if width == 0 || height == 0 {
            // Either `Render::render` couldn't scale a plain outline for
            // this glyph id at all (`None`), or it scaled to a
            // zero-size/empty-ink result (`Some` with an empty
            // `Placement`) -- both cases look the same from here and
            // both need the same follow-up question: is this glyph
            // actually blank (e.g. a space), or does it just have no
            // *plain* outline because it's a color glyph?
            //
            // We can't answer that per-glyph the cheap way (probing
            // `scale_color_outline`/`scale_color_bitmap` for this glyph
            // id) because that only reliably detects COLRv0 layers --
            // `NotoColorEmoji.ttf` in this repo's test assets is
            // COLRv1 (confirmed: FreeType's own `load_and_render_glyph`
            // also can't render it directly and raises `IsColr1OrLater`,
            // see `ftwrap.rs`), and swash's COLRv0-only `layers()`
            // lookup (see `swash::scale::color::ColorProxy::layers`)
            // returns `None` for COLRv1 glyphs just like it would for a
            // glyph that legitimately has no color data. So instead we
            // use the font-level signal: if this face has *any* color
            // capability at all (COLR of any version, or CBDT/sbix
            // bitmap strikes), treat an empty plain-outline result as
            // "probably a color glyph swash can't render on this path"
            // and delegate to `colr_paint::ColrRasterizer`, our pure-Rust
            // COLR/COLRv1 paint-graph rasterizer built on
            // `ttf_parser::colr`. Fonts with no color capability at all
            // (the overwhelming common case) skip straight to treating
            // this as a legitimately blank glyph (space, zero-width
            // joiner, etc.), avoiding an unnecessary COLR-parsing
            // round-trip for every whitespace character.
            if has_any_color_data {
                drop(scaler);
                drop(context);
                return self.color_fallback.rasterize_glyph(glyph_pos, size, dpi);
            }
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

        let image = image.expect("width/height > 0 implies image is Some");

        // `image.data` for `Format::Alpha`/`Content::Mask` is a tightly
        // packed 8-bit coverage mask, one byte per pixel, row-major,
        // top-to-bottom (there is no separate "pitch"/stride concept the
        // way FreeType's `FT_Bitmap` has, since swash never pads rows).
        // This is the direct equivalent of FreeType's
        // `FT_PIXEL_MODE_GRAY` bitmap consumed by `rasterize_gray` in
        // `freetype.rs`; the conversion to premultiplied sRGB-with-linear-alpha
        // RGBA below mirrors that function exactly.
        anyhow::ensure!(
            image.data.len() == width * height,
            "unexpected swash alpha image size: {} bytes for {width}x{height}",
            image.data.len()
        );

        let mut rgba = vec![0u8; width * height * 4];
        for y in 0..height {
            let src_offset = y * width;
            let dest_offset = y * width * 4;
            for x in 0..width {
                let linear_gray = image.data[src_offset + x];
                let gray = linear_u8_to_srgb8(linear_gray);

                // Texture is SRGBA: RGB channels are gamma-adjusted
                // (non-linear), alpha is linear -- see the identical
                // comment in `FreeTypeRasterizer::rasterize_gray`.
                rgba[dest_offset + (x * 4)] = gray;
                rgba[dest_offset + (x * 4) + 1] = gray;
                rgba[dest_offset + (x * 4) + 2] = gray;
                rgba[dest_offset + (x * 4) + 3] = linear_gray;
            }
        }

        // swash's `Placement` uses a bottom-left origin (`Origin::BottomLeft`,
        // the default used internally by `Render::render_into`'s
        // `Mask::origin`): `left`/`top` are the offset from the glyph's
        // origin (baseline) to the top-left corner of the bitmap, with
        // `top` measured *upward* from the baseline -- the same
        // convention as FreeType's `bitmap_left`/`bitmap_top` consumed by
        // `rasterize_gray`/`rasterize_mono` in `freetype.rs`. No sign
        // flip or axis remap is needed here.
        Ok(RasterizedGlyph {
            data: rgba,
            height,
            width,
            bearing_x: PixelLength::new(image.placement.left as f64),
            bearing_y: PixelLength::new(image.placement.top as f64),
            has_color: false,
            is_scaled: true,
        })
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::locator::{FontDataSource, FontOrigin};
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

    /// Regression test for a transcription bug found while verifying this
    /// rasterizer visually with `dump_glyph`: the grayscale-to-RGBA
    /// conversion loop originally computed `dest_offset` once per row
    /// (`y * width * 4`) but forgot to add `x * 4` for each column (the
    /// version in `freetype.rs`'s `rasterize_gray` does add it), so every
    /// pixel in a row overwrote the same 4 bytes and the final bitmap was
    /// almost entirely transparent/black except for whatever pixel
    /// happened to be written last. This asserts that a real glyph (not
    /// just the empty/space case covered by `empty_glyph_does_not_error`)
    /// produces plausible non-zero coverage across more than a handful of
    /// pixels, which the buggy version would have failed.
    #[test]
    fn nonempty_glyph_has_broad_alpha_coverage() {
        let parsed = parsed_font_for("JetBrainsMono-Regular.ttf");
        let raster = SwashRasterizer::from_locator(&parsed).unwrap();
        let font_info = crate::swash_metrics::SwashFontInfo::from_locator(
            &FontDataSource::OnDisk(asset_path("JetBrainsMono-Regular.ttf")),
            0,
        )
        .unwrap();
        let gid = font_info.glyph_id_for_char('A');
        assert_ne!(gid, 0);

        let glyph = raster.rasterize_glyph(gid as u32, 8.0, 96).unwrap();
        let nonzero_alpha_pixels = glyph.data.chunks_exact(4).filter(|p| p[3] > 0).count();
        let total_pixels = glyph.width * glyph.height;
        // The buggy version produced at most 1-2 nonzero pixels (whatever
        // was written last into the aliased row slot) out of ~56 total;
        // a correctly rendered capital A at 8pt should have meaningfully
        // more ink than that -- require at least a quarter of the pixels
        // to carry some coverage.
        assert!(
            nonzero_alpha_pixels * 4 >= total_pixels,
            "expected broad alpha coverage for 'A', got {nonzero_alpha_pixels}/{total_pixels} \
             nonzero-alpha pixels -- possible regression of the dest_offset bug"
        );
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

    /// Sanity check: rasterizing a plain ASCII glyph at a handful of
    /// sizes produces a non-empty, correctly-shaped bitmap, and the
    /// reported bearing/advance-adjacent dimensions are sane (no
    /// zero-size or wildly-out-of-bounds results). This is intentionally
    /// not a pixel-parity test against FreeType -- see the module doc
    /// comment for why -- that comparison is done visually via
    /// `dump_glyph --rasterizer swash` per the migration plan.
    #[test]
    fn rasterizes_ascii_glyphs_at_several_sizes() {
        let parsed = parsed_font_for("JetBrainsMono-Regular.ttf");
        let raster = SwashRasterizer::from_locator(&parsed).unwrap();

        let font_info = crate::swash_metrics::SwashFontInfo::from_locator(
            &FontDataSource::OnDisk(asset_path("JetBrainsMono-Regular.ttf")),
            0,
        )
        .unwrap();

        for &(size, dpi) in &[(8.0f64, 96u32), (12.0, 96), (24.0, 96), (48.0, 144)] {
            for c in ['A', 'g', 'W', 'i', '.'] {
                let gid = font_info.glyph_id_for_char(c);
                assert_ne!(gid, 0, "no glyph id for {c:?}");

                let glyph = raster.rasterize_glyph(gid as u32, size, dpi).unwrap();
                assert!(
                    glyph.width > 0 && glyph.height > 0,
                    "expected non-empty bitmap for {c:?} at size={size} dpi={dpi}, got {}x{}",
                    glyph.width,
                    glyph.height
                );
                assert_eq!(
                    glyph.data.len(),
                    glyph.width * glyph.height * 4,
                    "data length mismatch for {c:?} at size={size} dpi={dpi}"
                );
                assert!(!glyph.has_color);
                assert!(glyph.is_scaled);
            }
        }
    }

    /// `.notdef`/space-like glyphs (or otherwise empty-ink glyphs) should
    /// not error out -- they should just come back as a zero-size
    /// `RasterizedGlyph`, matching how `rasterize_from_ops` in
    /// `freetype.rs` handles an empty bbox.
    #[test]
    fn empty_glyph_does_not_error() {
        let parsed = parsed_font_for("JetBrainsMono-Regular.ttf");
        let raster = SwashRasterizer::from_locator(&parsed).unwrap();

        let font_info = crate::swash_metrics::SwashFontInfo::from_locator(
            &FontDataSource::OnDisk(asset_path("JetBrainsMono-Regular.ttf")),
            0,
        )
        .unwrap();
        let gid = font_info.glyph_id_for_char(' ');
        assert_ne!(gid, 0);
        let glyph = raster.rasterize_glyph(gid as u32, 16.0, 96).unwrap();
        assert_eq!(glyph.width, 0);
        assert_eq!(glyph.height, 0);
        assert!(glyph.data.is_empty());
    }

    /// Regression test for a second bug found during visual verification:
    /// `NotoColorEmoji.ttf` (this repo's test asset) turns out to carry
    /// COLRv1 glyphs, not CBDT/sbix bitmaps as the H2 module doc comment
    /// assumed. swash's `scale_color_outline`/`Scaler::has_color_outlines`
    /// only understand COLRv0's simple `BaseGlyphList` layer format (see
    /// `swash::scale::color::ColorProxy::layers`), so a COLRv1 glyph's
    /// plain `Source::Outline` render comes back as an empty (0x0)
    /// result and a naive per-glyph "does this glyph have COLRv0 layers"
    /// probe also says no -- with no distinct signal from swash itself
    /// that this is "a color glyph rendered a different way" rather than
    /// "genuinely blank". This asserts that such a glyph still ends up
    /// correctly delegated to `colr_paint::ColrRasterizer` (via the
    /// font-level `has_any_color_data` check) instead of silently coming
    /// back as an empty/invisible glyph.
    #[test]
    fn colr_glyph_falls_back_to_colr_paint_rasterizer() {
        let parsed = parsed_font_for("NotoColorEmoji.ttf");
        let raster = SwashRasterizer::from_locator(&parsed).unwrap();

        let font_info = crate::swash_metrics::SwashFontInfo::from_locator(
            &FontDataSource::OnDisk(asset_path("NotoColorEmoji.ttf")),
            0,
        )
        .unwrap();
        let gid = font_info.glyph_id_for_char('\u{1F600}'); // grinning face
        assert_ne!(gid, 0, "no glyph id for grinning face emoji");

        let glyph = raster.rasterize_glyph(gid as u32, 32.0, 96).unwrap();
        assert!(
            glyph.width > 0 && glyph.height > 0,
            "expected a non-empty rasterized emoji glyph via the ColrRasterizer \
             fallback, got {}x{}",
            glyph.width,
            glyph.height
        );
        assert!(
            glyph.has_color,
            "expected the COLR emoji glyph to come back tagged has_color=true"
        );
    }
}
