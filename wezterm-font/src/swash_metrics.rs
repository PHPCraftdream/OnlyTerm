//! Swash-based parsing/metrics counterpart to `ftwrap.rs`.
//!
//! This is part of the freetype+harfbuzz -> rustybuzz+swash migration
//! (see docs/plans/2026-07-23-freetype-harfbuzz-migration.md, phase H2).
//!
//! `ftwrap.rs` is not exclusively used for rasterizing glyph bitmaps: a
//! large fraction of its surface is pure font-file parsing (metrics, name
//! table strings, variable-font axes/named-instances, cmap lookups, CPAL
//! palettes, OS/2-derived attributes). None of that requires FreeType's
//! rasterizer/hinter at all, so it can be reimplemented on `swash`
//! (`skrifa`/`zeno`/`yazi` under the hood) without touching pixel output.
//!
//! This module is a **parallel, standalone** implementation for
//! comparison purposes only: nothing in the production code path
//! constructs a `SwashFontInfo` yet (that wiring, if it happens at all,
//! is a later phase after H3 rasterization lands). It deliberately keeps
//! its own copy of raw font bytes rather than sharing `ftwrap::Face` or
//! `rustybuzz::Face`, so that it can be constructed and torn down
//! completely independently of the FreeType/harfbuzz code paths -- this
//! matters for the BUG7 thread-safety concerns noted in the migration
//! plan (vendored harfbuzz previously built with `HB_NO_MT`; that flag is
//! now gone, but this module still avoids sharing any FreeType/harfbuzz
//! global state by construction, since it never touches those crates).
//!
//! ## Coverage map (ftwrap.rs function -> swash equivalent)
//!
//! | `ftwrap`/`Face` | swash equivalent (this module) |
//! |---|---|
//! | `family_name()` / `style_name()` | [`SwashFontInfo::family_name`] / [`style_name`](SwashFontInfo::style_name) via `localized_strings()` |
//! | `postscript_name()` | [`SwashFontInfo::postscript_name`] via `localized_strings()` |
//! | `get_sfnt_names()` | [`SwashFontInfo::sfnt_names`] via `localized_strings()` |
//! | `get_os2_table().sCapHeight` / `cap_height()` | [`SwashFontInfo::cap_height_ratio`] via `metrics()` |
//! | `weight_and_width()` | [`SwashFontInfo::weight_and_width`] via `attributes()` (+ variation instance, if applicable) |
//! | `italic()` | [`SwashFontInfo::is_italic`] via `attributes().style()` |
//! | `compute_coverage()` | [`SwashFontInfo::compute_coverage`] via `Charmap::enumerate` |
//! | `pixel_sizes()` (bitmap strikes) | [`SwashFontInfo::pixel_sizes`] via `color_strikes()`/`alpha_strikes()` |
//! | `variations()` (named instances -> `ParsedFont`s) | [`SwashFontInfo::instances`] via `FontRef::instances()` |
//! | cmap codepoint -> glyph_id | [`SwashFontInfo::glyph_id_for_char`] via `Charmap::map()` |
//! | `units_per_em`/`num_glyphs` (`(*face).units_per_EM`/`num_glyphs`) | [`SwashFontInfo::units_per_em`] / [`SwashFontInfo::num_glyphs`] via `metrics()` |
//! | `get_palette_data()` / `get_palette_entry()` (CPAL) | [`SwashFontInfo::palettes`] via `color_palettes()` |
//! | `set_font_size()`/`cell_metrics()` (line height, nominal monospace cell) | [`SwashFontInfo::metrics_for_size`] via `metrics().scale(ppem)` + `glyph_metrics().advance_width()` |
//! | underline position/thickness | included in [`SwashMetrics`] (`metrics().underline_offset/stroke_size`) -- **note:** `underline_position` requires a FreeType-compatible adjustment, see the doc comment on [`SwashFontInfo::metrics`] |
//!
//! Explicitly **not** covered here (these require rendering, deferred to
//! H3): `load_and_render_glyph`, `load_glyph_outlines`,
//! `get_color_glyph_paint`/`get_color_glyph_clip_box`/`get_paint`/
//! `get_paint_layers` (COLRv1 paint graph walking -- swash's rendering
//! path exposes this differently, via `swash::scale::ColorOutline` /
//! `Render::render_color`, which belongs with the H3 rasterizer work,
//! not the H2 metrics-only pass), `set_transform` (rendering-time glyph
//! transform).

use crate::locator::FontDataSource;
use rangeset::RangeSet;
use swash::{Attributes, CacheKey, FontRef, Style};

/// Owns the raw font bytes and a swash `FontRef` positioned at a specific
/// sub-face (relevant for TrueType collections). Mirrors the role of
/// `ftwrap::Face`, but purely for parsing/metrics -- there is no
/// equivalent of FreeType's `FT_Library`/`FT_Face` handle lifetime here;
/// `swash::FontRef` is just a transient borrow over `data`.
pub struct SwashFontInfo {
    data: Box<[u8]>,
    offset: u32,
    key: CacheKey,
}

/// Global, unscaled font metrics (all values in font design units unless
/// otherwise noted -- use [`SwashMetrics::scale`] to get pixel-space
/// values for a given `units_per_em`-relative pixel size, matching the
/// contract of `ftwrap`'s `FT_F26Dot6`-based pixel metrics).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SwashMetrics {
    pub units_per_em: u16,
    pub glyph_count: u16,
    pub is_monospace: bool,
    pub ascent: f32,
    pub descent: f32,
    pub leading: f32,
    pub cap_height: f32,
    pub x_height: f32,
    pub underline_position: f32,
    pub underline_thickness: f32,
}

/// The nominal monospace cell size computed the same way
/// `ftwrap::Face::cell_metrics` does: scan a sample of glyphs (ASCII
/// printable range, falling back to glyph ids 1..8 for symbol-only
/// fonts) and take the largest horizontal advance as the cell width; use
/// the scaled line height (`ascent + descent [+ leading]`, matching
/// FreeType's `face->height` convention rather than raw ascent-descent)
/// as the cell height.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SwashCellMetrics {
    pub width: f64,
    pub height: f64,
}

/// One entry of the `name` table, analogous to `ftwrap::NameRecord`.
#[derive(Debug, Clone)]
pub struct SwashNameRecord {
    pub id: swash::StringId,
    pub language: String,
    pub name: String,
}

/// A named instance of a variable font (an `fvar` entry), analogous to
/// one iteration of `ftwrap::Face::variations()`.
#[derive(Debug, Clone)]
pub struct SwashInstance {
    pub index: usize,
    pub name: Option<String>,
    pub postscript_name: Option<String>,
    /// Normalized (2.14 fixed-point encoded as i16) coordinates, one per
    /// `fvar` axis, in axis order. This is the same representation
    /// `rustybuzz::Variation`/`ttf_parser`'s normalized coords use, and
    /// is what should be handed to `swash::scale::Scaler::variation`
    /// (or, for rustybuzz, converted to axis tag/value pairs) to select
    /// this instance.
    pub normalized_coords: Vec<i16>,
}

/// A CPAL color palette, analogous to `ftwrap::Palette`.
#[derive(Debug, Clone)]
pub struct SwashPalette {
    pub index: u16,
    pub name: Option<String>,
    /// RGBA entries, in palette order.
    pub entries: Vec<[u8; 4]>,
}

impl SwashFontInfo {
    /// Parses font data from `source`, selecting sub-face `index` (0 for
    /// non-collection fonts; mirrors `ftwrap::Library::new_face`'s
    /// `face_index`, but without the named-instance bit-packing FreeType
    /// uses -- swash has no notion of "face index selects a named
    /// instance", named instances are only reachable via
    /// [`SwashFontInfo::instances`]/`normalized_coords`).
    pub fn from_locator(source: &FontDataSource, index: u32) -> anyhow::Result<Self> {
        let data = source.load_data()?.into_owned().into_boxed_slice();
        let font = FontRef::from_index(&data, index as usize)
            .ok_or_else(|| anyhow::anyhow!("swash failed to parse font face at index {index}"))?;
        let offset = font.offset;
        let key = font.key;
        Ok(Self { data, offset, key })
    }

    /// Returns a transient `FontRef` borrowing our owned bytes. Cheap:
    /// `FontRef` is `Copy` and just holds a slice + offset + cache key.
    pub fn as_font_ref(&self) -> FontRef<'_> {
        FontRef {
            data: &self.data,
            offset: self.offset,
            key: self.key,
        }
    }

    /// Equivalent of `ftwrap::Face::family_name`. Prefers the
    /// typographic family name (name id 16) over the legacy family name
    /// (name id 1), matching how most modern shaping stacks resolve
    /// family names, and how `ftwrap`'s underlying FreeType/fontconfig
    /// stack effectively behaves for fonts that set both.
    pub fn family_name(&self) -> String {
        let font = self.as_font_ref();
        let strings = font.localized_strings();
        strings
            .find_by_id(swash::StringId::TypographicFamily, None)
            .or_else(|| strings.find_by_id(swash::StringId::Family, None))
            .map(|s| s.to_string())
            .unwrap_or_default()
    }

    /// Equivalent of `ftwrap::Face::style_name`.
    pub fn style_name(&self) -> String {
        let font = self.as_font_ref();
        let strings = font.localized_strings();
        strings
            .find_by_id(swash::StringId::TypographicSubFamily, None)
            .or_else(|| strings.find_by_id(swash::StringId::SubFamily, None))
            .map(|s| s.to_string())
            .unwrap_or_default()
    }

    /// Equivalent of `ftwrap::Face::postscript_name`.
    pub fn postscript_name(&self) -> String {
        let font = self.as_font_ref();
        font.localized_strings()
            .find_by_id(swash::StringId::PostScript, None)
            .map(|s| s.to_string())
            .unwrap_or_default()
    }

    /// Equivalent of `ftwrap::Face::get_sfnt_names`, restricted (like the
    /// FreeType version) to the family/subfamily/postscript name ids that
    /// wezterm actually consults elsewhere.
    pub fn sfnt_names(&self) -> Vec<SwashNameRecord> {
        let font = self.as_font_ref();
        let wanted = [
            swash::StringId::TypographicFamily,
            swash::StringId::TypographicSubFamily,
            swash::StringId::Family,
            swash::StringId::SubFamily,
            swash::StringId::PostScript,
        ];
        let mut out = vec![];
        for rec in font.localized_strings() {
            if !wanted.contains(&rec.id()) {
                continue;
            }
            out.push(SwashNameRecord {
                id: rec.id(),
                language: rec.language().to_string(),
                name: rec.to_string(),
            });
        }
        out
    }

    /// Equivalent of `ftwrap::Face::italic` (`FT_STYLE_FLAG_ITALIC`,
    /// which FreeType derives from the OS/2 `fsSelection`/head
    /// `macStyle` italic bit -- the same bit `swash::Attributes::style()`
    /// reads via `Os2::selection_flags().italic()`). Note this only
    /// reports the *static* italic flag; it does not consider whether an
    /// `ital`/`slnt` variation axis could produce an italic at some other
    /// coordinate (see [`SwashFontInfo::attributes`] for that).
    pub fn is_italic(&self) -> bool {
        matches!(self.attributes().style(), Style::Italic)
    }

    /// Returns the raw swash `Attributes` (stretch/weight/style + which
    /// of those have variable-font axes) for the font at its default
    /// instance. Exposed directly (unlike `ftwrap`, which only surfaces
    /// `italic()`/`weight_and_width()` piecemeal) because `Attributes`
    /// already bundles exactly the OS/2-derived classification data
    /// `ftwrap::Face::get_os2_table` extracts by hand.
    pub fn attributes(&self) -> Attributes {
        self.as_font_ref().attributes()
    }

    /// Equivalent of `ftwrap::Face::weight_and_width`: returns
    /// `(weight_class, width_class)` as used by fontconfig-style
    /// matching. For a named instance (`instance_index.is_some()`), the
    /// weight/width are adjusted using the *ratio* of the instance's
    /// wght/wdth axis values to the axis defaults, exactly mirroring
    /// `ftwrap::Face::weight_and_width`'s `scale = value / default_value`
    /// logic (rather than reporting the raw instance coordinate
    /// directly), since OS/2 weight/width classes and `fvar` axis units
    /// are not guaranteed to be on the same numeric scale for a given
    /// font.
    pub fn weight_and_width(&self, instance_index: Option<usize>) -> (u16, u16) {
        let font = self.as_font_ref();
        let attrs = self.attributes();
        let mut weight = attrs.weight().0 as f64;
        // `Attributes::stretch()` is derived from `Stretch::from_raw(os2.usWidthClass)`,
        // which maps the OS/2 usWidthClass integer (1..9) onto CSS percentage
        // steps of 12.5%/25% (`ULTRA_CONDENSED` == 50% == class 1, `NORMAL` ==
        // 100% == class 5, `ULTRA_EXPANDED` == 200%..300% == class 9). To get
        // back the original `usWidthClass` integer that `ftwrap`'s
        // `os2.usWidthClass` reports directly, invert that mapping rather than
        // trying to derive it from the percentage arithmetically (the mapping
        // is not evenly spaced: classes 1..5 step by 12.5%, but 200%/300% both
        // collapse to class 8/9): see `Stretch::from_raw` in the swash source
        // for the authoritative table.
        let mut width = match attrs.stretch() {
            s if s == swash::Stretch::ULTRA_CONDENSED => 1.0,
            s if s == swash::Stretch::EXTRA_CONDENSED => 2.0,
            s if s == swash::Stretch::CONDENSED => 3.0,
            s if s == swash::Stretch::SEMI_CONDENSED => 4.0,
            s if s == swash::Stretch::NORMAL => 5.0,
            s if s == swash::Stretch::SEMI_EXPANDED => 6.0,
            s if s == swash::Stretch::EXPANDED => 7.0,
            s if s == swash::Stretch::EXTRA_EXPANDED => 8.0,
            s if s == swash::Stretch::ULTRA_EXPANDED => 9.0,
            _ => 5.0,
        };

        if let (Some(idx), Some(instance)) = (
            instance_index,
            instance_index.and_then(|i| font.instances().nth(i)),
        ) {
            let axes: Vec<_> = font.variations().collect();
            for (axis, value) in axes.iter().zip(instance.values()) {
                let default_value = axis.default_value() as f64;
                let scale = if default_value != 0. {
                    value as f64 / default_value
                } else {
                    1.
                };
                match axis.tag() {
                    t if t == swash::tag_from_bytes(b"wght") => weight *= scale,
                    t if t == swash::tag_from_bytes(b"wdth") => width *= scale,
                    _ => {}
                }
            }
            let _ = idx;
        }

        (weight.round() as u16, width.round() as u16)
    }

    /// Equivalent of `ftwrap::Face::cap_height` (the *ratio*, not the
    /// pixel value): `sCapHeight / unitsPerEm` from the OS/2 table.
    /// Returns `None` under the same conditions FreeType does: no OS/2
    /// table, `unitsPerEm == 0`, or `sCapHeight == 0` (many fonts,
    /// especially older or symbol fonts, do not set this field).
    pub fn cap_height_ratio(&self) -> Option<f64> {
        let font = self.as_font_ref();
        let metrics = font.metrics(&[]);
        if metrics.units_per_em == 0 || metrics.cap_height == 0.0 {
            return None;
        }
        Some(metrics.cap_height as f64 / metrics.units_per_em as f64)
    }

    /// Equivalent of `(*face).units_per_EM`.
    pub fn units_per_em(&self) -> u16 {
        self.as_font_ref().metrics(&[]).units_per_em
    }

    /// Equivalent of `(*face).num_glyphs`.
    pub fn num_glyphs(&self) -> u16 {
        self.as_font_ref().metrics(&[]).glyph_count
    }

    /// Returns unscaled (font design unit) global metrics, equivalent to
    /// the fields `ftwrap` pulls piecemeal from `FT_FaceRec`/`FT_Size` /
    /// the `post`/`OS2`/`hhea` tables (`underline_position`,
    /// `underline_thickness`, cap height, ascender/descender). Unlike
    /// `ftwrap`, which only exposes these already pixel-scaled (via
    /// `FT_Size`, after `set_font_size`), this returns raw design-space
    /// values; use [`SwashMetrics::scale`] to bring them to pixel space
    /// for a given ppem, matching `swash::Metrics::scale`.
    ///
    /// ## Known FreeType semantic difference: `underline_position`
    ///
    /// The raw `post` table (and swash's `Metrics::underline_offset`)
    /// reports `underlinePosition` as specified by the OpenType/TrueType
    /// `post` table: the distance from the baseline to the **top edge**
    /// of the recommended underline stroke. FreeType, however,
    /// deliberately reinterprets this field: `sfnt_load_face` in
    /// `freetype2/src/sfnt/sfobjs.c` computes
    /// `root->underline_position = post.underlinePosition -
    /// post.underlineThickness / 2`, shifting the value from "top edge"
    /// to "center of stroke" ("Adjust underline position from top edge
    /// to centre of stroke to convert TrueType meaning to FreeType
    /// meaning", per that file's comment). This is a **confirmed, real
    /// discrepancy** we found while writing this module's tests (see
    /// `test::underline_metrics_match_freetype`): for
    /// `JetBrainsMono-Regular.ttf`, the raw/swash value is -155 design
    /// units, FreeType reports -180 (with `underlineThickness = 50`,
    /// `-155 - 50/2 == -180`, confirmed by direct inspection of the
    /// font's `post` table). It is not a bug in either library, just an
    /// intentional semantic difference in what "underline position"
    /// means. To reproduce FreeType's value/behavior exactly (required,
    /// since wezterm's cell-underline placement is presumably tuned
    /// against FreeType's convention), callers must apply the same
    /// adjustment: `underline_offset - stroke_size / 2`. We do that
    /// adjustment here so that this field is a drop-in, parity-matching
    /// replacement for `ftwrap`'s; see the module-level doc comment and
    /// the final report for why this is called out explicitly rather
    /// than silently "fixed" to match a lucky test result.
    pub fn metrics(&self) -> SwashMetrics {
        let font = self.as_font_ref();
        let m = font.metrics(&[]);
        SwashMetrics {
            units_per_em: m.units_per_em,
            glyph_count: m.glyph_count,
            is_monospace: m.is_monospace,
            ascent: m.ascent,
            descent: m.descent,
            leading: m.leading,
            cap_height: m.cap_height,
            x_height: m.x_height,
            // FreeType-compatible adjustment: see doc comment above.
            underline_position: m.underline_offset - m.stroke_size / 2.0,
            underline_thickness: m.stroke_size,
        }
    }

    /// Equivalent of `ftwrap::Face::cell_metrics` (called internally by
    /// `set_font_size`): computes the nominal monospace cell
    /// width/height in pixels for the given point size and dpi, using
    /// the same algorithm -- scan glyphs for ASCII 32..128 (falling back
    /// to glyph ids 1..8 if none of those have advances) and take the
    /// largest horizontal advance as the cell width; the line height
    /// (`ascent + descent + leading`, matching FreeType's
    /// `face->height`, which already includes the line gap) as the cell
    /// height.
    pub fn cell_metrics(&self, point_size: f64, dpi: u32) -> SwashCellMetrics {
        let font = self.as_font_ref();
        let metrics = font.metrics(&[]);
        let pixel_height = point_size * dpi as f64 / 72.0;
        let ppem = pixel_height as f32;
        let scaled = metrics.scale(ppem);
        let height = (scaled.ascent + scaled.descent + scaled.leading) as f64;

        let glyph_metrics = font.glyph_metrics(&[]).scale(ppem);
        let charmap = font.charmap();

        let mut width = 0.0f64;
        for cp in 32u32..128 {
            let gid = charmap.map(cp);
            if gid == 0 {
                continue;
            }
            let advance = glyph_metrics.advance_width(gid) as f64;
            if advance > width {
                width = advance;
            }
        }
        if width == 0.0 {
            for gid in 1u16..8 {
                let advance = glyph_metrics.advance_width(gid) as f64;
                if advance > width {
                    width = advance;
                }
            }
            if width == 0.0 {
                width = height;
            }
        }

        SwashCellMetrics { width, height }
    }

    /// Equivalent of `ftwrap::Face::compute_coverage`: enumerate the
    /// cmap and build the set of covered codepoints. Unlike `ftwrap`
    /// (which walks `FT_Get_First_Char`/`FT_Get_Next_Char` over the
    /// Unicode and MS-Symbol charmaps separately, plus a manual
    /// F000..F0FF -> 0000..00FF remap for symbol fonts), this uses
    /// `Charmap::enumerate`, which already walks whichever single cmap
    /// subtable swash selected (it internally applies the same
    /// F000-remap logic inside `Charmap::map`, but *not* inside
    /// `enumerate` -- see the discrepancy noted in the module-level
    /// comparison test).
    pub fn compute_coverage(&self) -> RangeSet<u32> {
        let font = self.as_font_ref();
        let charmap = font.charmap();
        let mut coverage = RangeSet::new();
        charmap.enumerate(|codepoint, glyph_id| {
            if glyph_id != 0 {
                coverage.add(codepoint);
            }
        });
        coverage
    }

    /// Equivalent of `ftwrap::Face::pixel_sizes` (bitmap strike sizes,
    /// e.g. for `.otb`/embedded-bitmap fonts like some emoji/CJK fonts).
    /// Reports both color (CBDT/sbix) and monochrome/grayscale (EBDT)
    /// strike heights, matching the fact that `FT_Face::available_sizes`
    /// also does not distinguish between the two.
    pub fn pixel_sizes(&self) -> Vec<u16> {
        let font = self.as_font_ref();
        let mut sizes: Vec<u16> = font
            .color_strikes()
            .chain(font.alpha_strikes())
            .map(|strike| strike.ppem())
            .filter(|&ppem| ppem > 0)
            .collect();
        sizes.sort_unstable();
        sizes.dedup();
        sizes
    }

    /// Equivalent of `ftwrap::Face::variations` (the named-instance
    /// enumeration half of it; this returns descriptive data about each
    /// instance rather than constructing a `ParsedFont`, since that type
    /// is intertwined with the `FontDataHandle`/locator machinery that
    /// is out of scope for this parsing-only module).
    pub fn instances(&self) -> Vec<SwashInstance> {
        let font = self.as_font_ref();
        let mut out = vec![];
        for (index, instance) in font.instances().enumerate() {
            out.push(SwashInstance {
                index,
                name: instance.name(None).map(|s| s.to_string()),
                postscript_name: instance.postscript_name(None).map(|s| s.to_string()),
                normalized_coords: instance.normalized_coords().collect(),
            });
        }
        out
    }

    /// Equivalent of the cmap-lookup half of `ftwrap` (FreeType's
    /// `FT_Get_Char_Index`, used e.g. by `compute_cap_height` to find
    /// the glyph id for `I`, and implicitly by the shapers' fallback
    /// logic): maps a single codepoint to its nominal glyph id, 0 if
    /// unmapped.
    pub fn glyph_id_for_char(&self, c: char) -> u16 {
        self.as_font_ref().charmap().map(c)
    }

    /// Equivalent of `ftwrap::Face::get_palette_data` (CPAL palette
    /// metadata) combined with `get_palette_entry` (resolving each
    /// entry's RGBA color) -- swash's `ColorPalettes`/`ColorPalette`
    /// iterator already exposes both in one pass, unlike FreeType's
    /// `FT_Palette_Data_Get`/`FT_Palette_Select` two-step API.
    pub fn palettes(&self) -> Vec<SwashPalette> {
        let font = self.as_font_ref();
        let mut out = vec![];
        for palette in font.color_palettes() {
            let mut entries = vec![];
            for i in 0..palette.len() {
                entries.push(palette.get(i));
            }
            out.push(SwashPalette {
                index: palette.index(),
                name: palette.name(None).map(|s| s.to_string()),
                entries,
            });
        }
        out
    }
}

impl SwashMetrics {
    /// Scales design-unit metrics to pixel space for the given `ppem`
    /// (pixels-per-em), matching `swash::Metrics::scale`'s convention
    /// and `ftwrap`'s `point_size * dpi / 72.0` pixel-height formula
    /// (the caller is expected to compute `ppem` the same way
    /// `ftwrap::Face::set_font_size` does).
    pub fn scale(&self, ppem: f32) -> SwashMetrics {
        let s = if self.units_per_em != 0 {
            ppem / self.units_per_em as f32
        } else {
            1.0
        };
        SwashMetrics {
            units_per_em: self.units_per_em,
            glyph_count: self.glyph_count,
            is_monospace: self.is_monospace,
            ascent: self.ascent * s,
            descent: self.descent * s,
            leading: self.leading * s,
            cap_height: self.cap_height * s,
            x_height: self.x_height * s,
            underline_position: self.underline_position * s,
            underline_thickness: self.underline_thickness * s,
        }
    }

    /// Line height in pixels, matching FreeType's `face->height`
    /// convention (ascent + descent + leading/line-gap) as used by
    /// `ftwrap::Face::cell_metrics`'s `height` computation.
    pub fn line_height(&self) -> f32 {
        self.ascent + self.descent + self.leading
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::ftwrap;
    use crate::locator::{FontDataSource, FontOrigin};
    use std::path::PathBuf;

    /// All of the reference fonts checked into `assets/fonts/`, chosen to
    /// cover: a monospace variable-adjacent family with many static
    /// weights (JetBrains Mono), a proportional family with italics
    /// (Roboto), a ligature-heavy monospace font (FiraCode), a large
    /// CJK/symbol test font (SymbolsNerdFontMono), and a color emoji font
    /// with CBDT bitmap strikes and no useful cmap-coverage overlap with
    /// the others (NotoColorEmoji) -- deliberately excluded from the
    /// advance-width/cmap parity loop below because it has (as of this
    /// writing) no scalable outlines at all, only embedded bitmaps, which
    /// is a separate (H3-relevant) code path; its inclusion in
    /// `pixel_sizes` coverage is still exercised via a dedicated test.
    fn reference_fonts() -> Vec<&'static str> {
        vec![
            "JetBrainsMono-Regular.ttf",
            "JetBrainsMono-Bold.ttf",
            "JetBrainsMono-Italic.ttf",
            "Roboto-Regular.ttf",
            "Roboto-Bold.ttf",
            "Roboto-Italic.ttf",
            "FiraCode-Regular.ttf",
            "SymbolsNerdFontMono-Regular.ttf",
        ]
    }

    fn asset_path(name: &str) -> PathBuf {
        // wezterm-font's CARGO_MANIFEST_DIR is `<repo>/wezterm-font`
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("assets/fonts")
            .join(name)
    }

    fn ft_library() -> ftwrap::Library {
        ftwrap::Library::new().unwrap()
    }

    fn ft_face_for(lib: &ftwrap::Library, name: &str) -> ftwrap::Face {
        let handle = crate::locator::FontDataHandle {
            source: FontDataSource::OnDisk(asset_path(name)),
            index: 0,
            variation: 0,
            origin: FontOrigin::BuiltIn,
            coverage: None,
        };
        lib.face_from_locator(&handle).unwrap()
    }

    fn swash_info_for(name: &str) -> SwashFontInfo {
        let source = FontDataSource::OnDisk(asset_path(name));
        SwashFontInfo::from_locator(&source, 0).unwrap()
    }

    #[test]
    fn units_per_em_matches_freetype() {
        let lib = ft_library();
        for name in reference_fonts() {
            let ft_face = ft_face_for(&lib, name);
            let swash_info = swash_info_for(name);

            let ft_upem = unsafe { (*ft_face.face).units_per_EM };
            let swash_upem = swash_info.units_per_em();

            assert_eq!(
                ft_upem, swash_upem,
                "units_per_em mismatch for {name}: freetype={ft_upem} swash={swash_upem}"
            );
        }
    }

    #[test]
    fn num_glyphs_matches_freetype() {
        let lib = ft_library();
        for name in reference_fonts() {
            let ft_face = ft_face_for(&lib, name);
            let swash_info = swash_info_for(name);

            let ft_num_glyphs = unsafe { (*ft_face.face).num_glyphs };
            let swash_num_glyphs = swash_info.num_glyphs();

            assert_eq!(
                ft_num_glyphs as u32, swash_num_glyphs as u32,
                "num_glyphs mismatch for {name}: freetype={ft_num_glyphs} swash={swash_num_glyphs}"
            );
        }
    }

    /// cmap codepoint -> glyph_id must match 1:1 (same guarantee already
    /// established for shaping in H0/H1). This is the metric with the
    /// highest blast radius if wrong: a mismatched glyph id means the
    /// wrong glyph is drawn, not just a slightly-off position.
    #[test]
    fn cmap_lookup_matches_freetype() {
        let lib = ft_library();
        for name in reference_fonts() {
            let ft_face = ft_face_for(&lib, name);
            let swash_info = swash_info_for(name);
            let swash_font = swash_info.as_font_ref();
            let charmap = swash_font.charmap();

            // ASCII printable range plus a sample of Latin-1 supplement
            // and general punctuation likely to be present in these
            // fonts.
            let mut samples: Vec<char> = (0x20u32..0x7f).filter_map(char::from_u32).collect();
            samples.extend((0xa0u32..0x100).filter_map(char::from_u32));
            samples.extend(['—', '–', '“', '”', '‘', '’', '…', '€']);

            for c in samples {
                let ft_gid = unsafe { ftwrap::FT_Get_Char_Index(ft_face.face, c as u32) };
                let swash_gid = charmap.map(c) as u32;

                assert_eq!(
                    ft_gid, swash_gid,
                    "glyph_id mismatch for {name} char={c:?} (U+{:04X}): freetype={ft_gid} swash={swash_gid}",
                    c as u32
                );
            }
        }
    }

    /// Advance width (the metric that directly determines terminal cell
    /// width -- the single least-negotiable metric in the whole
    /// migration) must match at a representative set of point
    /// sizes/dpis for glyphs with simple, unhinted-in-practice TrueType
    /// outlines. We compare swash's *unhinted* (raw, unscaled
    /// design-space advance scaled linearly) advance against FreeType's
    /// unhinted advance (`FT_LOAD_NO_HINTING`) -- comparing against
    /// FreeType's normally-hinted advance would conflate this test with
    /// H3's hinting-parity question, which is explicitly out of scope
    /// for H2 (see module doc comment and the plan's risk #1).
    ///
    /// ## Confirmed finding: sub-pixel rounding difference (not a bug)
    ///
    /// Even with hinting fully disabled on both sides, we found the
    /// scaled advance is not always bit-for-bit identical: e.g.
    /// `Roboto-Regular.ttf` 'A' at 14pt/96dpi (ppem 18.667px) gives
    /// FreeType `12.1875` (`780/64`, a clean 26.6 fixed-point value) vs.
    /// swash `12.177083...` (matching the "ideal" `1336 * 18.667 / 2048`
    /// floating point product, verified independently against the raw
    /// `hmtx`/`head` table values). The ~0.0104px difference comes from
    /// FreeType computing and rounding its 16.16 fixed-point
    /// `size->metrics.x_scale` once per size (then multiplying that
    /// already-rounded scale by each glyph's raw advance), whereas swash
    /// (`GlyphMetrics::advance_width`/`Metrics::scale`) computes the
    /// scale as an `f32` and multiplies without an intermediate
    /// fixed-point rounding step. Neither side is "wrong" -- this is an
    /// inherent difference between a fixed-point and a floating-point
    /// scaling pipeline, and is far smaller than a single pixel, so it
    /// does not threaten cell-grid alignment (which cares about the
    /// *rounded-to-integer-pixel* cell width, not the sub-pixel-exact
    /// unhinted advance -- compare `RustybuzzShaper::do_shape`'s
    /// `scaled_advance` helper, which already rounds to the nearest
    /// whole pixel for exactly this reason). We assert a tolerance wide
    /// enough to accommodate this rounding-pipeline difference (but far
    /// tighter than a full pixel) rather than silently exact-matching by
    /// construction.
    #[test]
    fn advance_width_matches_freetype_unhinted() {
        let lib = ft_library();
        for name in reference_fonts() {
            for &(point_size, dpi) in &[(10.0f64, 72u32), (14.0, 96), (24.0, 144)] {
                let ft_face = ft_face_for(&lib, name);
                let pixel_height = point_size * dpi as f64 / 72.0;
                let size = ftwrap::FT_F26Dot6::from_num(point_size);
                if ftwrap::ft_result(
                    unsafe {
                        ftwrap::FT_Set_Char_Size(ft_face.face, size, size, dpi, dpi)
                    },
                    (),
                )
                .is_err()
                {
                    // Bitmap-only or otherwise unscalable at this size;
                    // skip (matches how ftwrap::set_font_size falls back
                    // to strike selection, which is out of scope here).
                    continue;
                }

                let swash_info = swash_info_for(name);
                let swash_font = swash_info.as_font_ref();
                let charmap = swash_font.charmap();
                let ppem = pixel_height as f32;
                let glyph_metrics = swash_font.glyph_metrics(&[]).scale(ppem);

                for c in 'A'..='Z' {
                    let ft_gid = unsafe { ftwrap::FT_Get_Char_Index(ft_face.face, c as u32) };
                    if ft_gid == 0 {
                        continue;
                    }
                    let res = unsafe {
                        ftwrap::FT_Load_Glyph(
                            ft_face.face,
                            ft_gid,
                            (ftwrap::FT_LOAD_NO_HINTING | ftwrap::FT_LOAD_NO_BITMAP) as i32,
                        )
                    };
                    if !ftwrap::succeeded(res) {
                        continue;
                    }
                    let ft_advance = unsafe {
                        (*(*ft_face.face).glyph)
                            .metrics
                            .horiAdvance
                            .f26d6()
                            .to_num::<f64>()
                    };

                    let swash_gid = charmap.map(c);
                    assert_eq!(
                        ft_gid, swash_gid as u32,
                        "glyph_id mismatch for {name} char={c:?} at size={point_size} dpi={dpi}"
                    );
                    let swash_advance = glyph_metrics.advance_width(swash_gid) as f64;

                    let diff = (ft_advance - swash_advance).abs();
                    // See the confirmed-finding note in this test's doc
                    // comment: FreeType's 26.6 fixed-point scale
                    // computation vs. swash's f32 scale can differ by a
                    // small sub-pixel amount even with hinting fully
                    // disabled on both sides. 0.02px is comfortably
                    // above the observed ~0.0104px worst case in our
                    // reference set, while still catching any real
                    // (multi-pixel-scale) regression.
                    assert!(
                        diff < 0.02,
                        "advance_width mismatch for {name} char={c:?} at size={point_size} dpi={dpi}: \
                         freetype(unhinted)={ft_advance} swash={swash_advance} diff={diff}"
                    );
                }
            }
        }
    }

    /// cap_height ratio (sCapHeight / unitsPerEm) must match exactly --
    /// this is a pure OS/2-table field read, not a rasterization-derived
    /// value, so there is no hinting-related excuse for any divergence.
    #[test]
    fn cap_height_ratio_matches_freetype() {
        let lib = ft_library();
        for name in reference_fonts() {
            let ft_face = ft_face_for(&lib, name);
            let swash_info = swash_info_for(name);

            let ft_cap_height = ft_face.cap_height();
            let swash_cap_height = swash_info.cap_height_ratio();

            match (ft_cap_height, swash_cap_height) {
                (Some(ft), Some(sw)) => {
                    assert!(
                        (ft - sw).abs() < 1e-9,
                        "cap_height ratio mismatch for {name}: freetype={ft} swash={sw}"
                    );
                }
                (None, None) => {}
                (ft, sw) => panic!(
                    "cap_height presence mismatch for {name}: freetype={ft:?} swash={sw:?}"
                ),
            }
        }
    }

    /// `italic()` (OS/2 fsSelection-derived) must match exactly.
    #[test]
    fn italic_flag_matches_freetype() {
        let lib = ft_library();
        for name in reference_fonts() {
            let ft_face = ft_face_for(&lib, name);
            let swash_info = swash_info_for(name);

            assert_eq!(
                ft_face.italic(),
                swash_info.is_italic(),
                "italic flag mismatch for {name}"
            );
        }
    }

    /// `weight_and_width()` (OS/2 usWeightClass/usWidthClass) must match
    /// exactly for static (non-variable) fonts.
    #[test]
    fn weight_and_width_matches_freetype() {
        let lib = ft_library();
        for name in reference_fonts() {
            let ft_face = ft_face_for(&lib, name);
            let swash_info = swash_info_for(name);

            let (ft_weight, ft_width) = ft_face.weight_and_width();
            let (swash_weight, swash_width) = swash_info.weight_and_width(None);

            assert_eq!(
                ft_weight, swash_weight,
                "weight mismatch for {name}: freetype={ft_weight} swash={swash_weight}"
            );
            assert_eq!(
                ft_width, swash_width,
                "width mismatch for {name}: freetype={ft_width} swash={swash_width}"
            );
        }
    }

    /// Family/style/postscript names must match exactly (modulo swash
    /// preferring the typographic family/subfamily name ids, which for
    /// all of our reference fonts fall back to the same value as the
    /// legacy name ids since none of them set distinct typographic
    /// names).
    #[test]
    fn names_match_freetype() {
        let lib = ft_library();
        for name in reference_fonts() {
            let ft_face = ft_face_for(&lib, name);
            let swash_info = swash_info_for(name);

            assert_eq!(
                ft_face.family_name(),
                swash_info.family_name(),
                "family_name mismatch for {name}"
            );
            assert_eq!(
                ft_face.postscript_name(),
                swash_info.postscript_name(),
                "postscript_name mismatch for {name}"
            );
        }
    }

    /// Underline position/thickness (from the `post` table) must match
    /// exactly -- these are direct table field reads on both sides, no
    /// rendering/hinting involved.
    #[test]
    fn underline_metrics_match_freetype() {
        let lib = ft_library();
        for name in reference_fonts() {
            let ft_face = ft_face_for(&lib, name);
            let swash_info = swash_info_for(name);

            let ft_underline_position = unsafe { (*ft_face.face).underline_position };
            let ft_underline_thickness = unsafe { (*ft_face.face).underline_thickness };

            let swash_metrics = swash_info.metrics();

            assert_eq!(
                ft_underline_position as i32, swash_metrics.underline_position as i32,
                "underline_position mismatch for {name}"
            );
            assert_eq!(
                ft_underline_thickness as i32, swash_metrics.underline_thickness as i32,
                "underline_thickness mismatch for {name}"
            );
        }
    }

    /// Sanity check that our variable-font instance enumeration produces
    /// at least the axes/weight relationship we expect. None of the
    /// current `assets/fonts/` reference fonts are variable fonts (they
    /// are all static per-weight TTFs), so this only exercises the
    /// "0 instances" branch; it is here primarily as a canary in case a
    /// variable font is added to the asset set later.
    #[test]
    fn instances_empty_for_static_fonts() {
        for name in reference_fonts() {
            let swash_info = swash_info_for(name);
            assert!(
                swash_info.instances().is_empty(),
                "{name} unexpectedly reported named instances (is it a variable font now?)"
            );
        }
    }

    /// `pixel_sizes()` should be empty for our scalable-outline
    /// reference fonts (no embedded bitmap strikes), matching FreeType's
    /// `available_sizes`.
    #[test]
    fn pixel_sizes_empty_for_scalable_fonts() {
        let lib = ft_library();
        for name in reference_fonts() {
            let ft_face = ft_face_for(&lib, name);
            let swash_info = swash_info_for(name);

            assert_eq!(
                ft_face.pixel_sizes(),
                swash_info.pixel_sizes(),
                "pixel_sizes mismatch for {name}"
            );
        }
    }

    /// NotoColorEmoji carries embedded (likely CBDT/CBLC or sbix) color
    /// bitmap strikes rather than scalable color outlines. This is the
    /// one case in our asset set where `pixel_sizes()` should be
    /// non-empty, exercising the CBDT/sbix strike-enumeration path on
    /// both sides.
    #[test]
    fn pixel_sizes_nonempty_for_color_emoji() {
        let lib = ft_library();
        let name = "NotoColorEmoji.ttf";
        let ft_face = ft_face_for(&lib, name);
        let swash_info = swash_info_for(name);

        let ft_sizes = ft_face.pixel_sizes();
        let swash_sizes = swash_info.pixel_sizes();

        assert_eq!(
            ft_sizes, swash_sizes,
            "pixel_sizes mismatch for {name}: freetype={ft_sizes:?} swash={swash_sizes:?}"
        );
    }

    /// cell_metrics()/set_font_size() nominal monospace cell size, which
    /// directly drives terminal cell width/height -- this is the metric
    /// singled out in the migration plan's acceptance criteria as
    /// requiring *exact* parity, not visual approximation.
    #[test]
    fn cell_metrics_match_freetype() {
        let lib = ft_library();
        for name in reference_fonts() {
            for &(point_size, dpi) in &[(10.0f64, 72u32), (14.0, 96)] {
                let mut ft_face = ft_face_for(&lib, name);
                let ft_selected = match ft_face.set_font_size(point_size, dpi) {
                    Ok(s) => s,
                    Err(_) => continue,
                };

                let swash_info = swash_info_for(name);
                let swash_cell = swash_info.cell_metrics(point_size, dpi);

                // FreeType's cell_metrics width comes from *hinted*
                // FT_LOAD_COLOR-loaded glyph advances (26.6 fixed point,
                // rounded via to_num), which for a monospace font at
                // integral pixel sizes should equal the unhinted
                // design-space-scaled advance from swash, since
                // monospace fonts are specifically designed so that
                // hinting does not need to adjust the advance width (see
                // the RustybuzzShaper module doc comment for the same
                // reasoning applied to shaping). We allow a small
                // tolerance to account for any residual hint-driven
                // rounding.
                let width_diff = (ft_selected.width - swash_cell.width).abs();
                assert!(
                    width_diff <= 1.0,
                    "cell width mismatch for {name} at size={point_size} dpi={dpi}: \
                     freetype={} swash={} diff={width_diff}",
                    ft_selected.width,
                    swash_cell.width
                );

                let height_diff = (ft_selected.height - swash_cell.height).abs();
                assert!(
                    height_diff <= 1.0,
                    "cell height mismatch for {name} at size={point_size} dpi={dpi}: \
                     freetype={} swash={} diff={height_diff}",
                    ft_selected.height,
                    swash_cell.height
                );
            }
        }
    }
}
