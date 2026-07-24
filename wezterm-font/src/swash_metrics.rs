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
//! This module started (in phase H2) as a **parallel, standalone**
//! implementation for comparison purposes only, with nothing in the
//! production code path constructing a `SwashFontInfo`. As of phase H4
//! (freetype/harfbuzz removal), this is no longer true: `parser.rs`'s
//! `ParsedFont::from_locator`/`from_face` (the font enumeration/matching
//! entry point used regardless of shaper/rasterizer selection) and
//! `shaper/rustybuzz.rs`'s `RustybuzzShaper` (both `ensure_rb_face`'s
//! named-instance resolution and `metrics_for_idx`/`metrics`'s cell
//! metrics) now construct and use `SwashFontInfo` directly as their sole
//! source of font-file parsing/metrics, with no FreeType involvement at
//! all. It deliberately keeps its own copy of raw font bytes rather than
//! sharing state with any other parser, so that it can be constructed and
//! torn down completely independently -- this matters for the BUG7
//! thread-safety concerns noted in the migration plan (vendored harfbuzz
//! previously built with `HB_NO_MT`; that flag is now moot since harfbuzz
//! itself has been removed, but this module still avoids sharing any
//! global state by construction).
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
    /// The sub-face index passed to `from_locator` (0 for non-collection
    /// fonts). Kept alongside `offset`/`key` (swash's own model of "which
    /// sub-face") so that [`SwashFontInfo::has_svg`]/[`has_color`] can
    /// re-parse the same sub-face with `ttf_parser`, which indexes by
    /// collection index rather than byte offset.
    face_index: u32,
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

/// Equivalent of `ftwrap::SelectedFontSize`/`ftwrap::Face::set_font_size`:
/// the nominal per-size cell metrics used to drive the terminal's cell
/// grid, plus the handful of extra scaled metrics (`cap_height`,
/// `underline_position`/`thickness`, `descender`) that
/// `RustybuzzShaper::metrics_for_idx` needs to build a `FontMetrics`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SwashSelectedFontSize {
    pub width: f64,
    pub height: f64,
    pub descender: f64,
    pub underline_thickness: f64,
    pub underline_position: f64,
    pub cap_height: Option<f64>,
    pub cap_height_to_height_ratio: Option<f64>,
    /// `true` if this font has scalable outlines and the metrics above
    /// came from those (scaled to the requested size); `false` if this
    /// is a bitmap-strike-only font and the metrics came from the
    /// nearest available strike instead (see
    /// [`SwashFontInfo::selected_font_size`]'s doc comment).
    pub is_scaled: bool,
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
    /// `swash::scale::Scaler::variation` (and `ttf_parser`'s own
    /// normalized coords) use, should this instance need to be selected
    /// through swash's own rendering path.
    pub normalized_coords: Vec<i16>,
    /// User-space (i.e. same units as `fvar`'s axis min/default/max
    /// values, e.g. a `wght` axis value like `700.0`) coordinates, one
    /// per `fvar` axis, in axis order -- what
    /// `rustybuzz::Face::set_variation`/`ttf_parser::Face::set_variation`
    /// actually expect (unlike `normalized_coords` above, no
    /// denormalization against axis min/default/max is needed to use
    /// this directly).
    pub user_values: Vec<f32>,
}

/// A CPAL color palette, analogous to `ftwrap::Palette`.
#[derive(Debug, Clone)]
pub struct SwashPalette {
    pub index: u16,
    pub name: Option<String>,
    /// RGBA entries, in palette order.
    pub entries: Vec<[u8; 4]>,
    /// From the CPAL v1 `paletteFlags` array (bit 0), if present -- `false`
    /// for CPAL v0 fonts (no flags array at all) or if unset.
    pub usable_with_light_bg: bool,
    /// From the CPAL v1 `paletteFlags` array (bit 1), if present -- `false`
    /// for CPAL v0 fonts (no flags array at all) or if unset.
    pub usable_with_dark_bg: bool,
}

/// Parses the CPAL v1 `paletteFlags` array directly from the raw `CPAL`
/// table bytes (obtained via `ttf_parser::Face::raw_face().table(..)`,
/// since neither `swash::palette::ColorPalette` nor
/// `ttf_parser::cpal::Table` (which only resolves individual color
/// entries) expose this at all -- it's a purely cosmetic per-palette hint
/// ("is this palette designed to be legible against a light/dark
/// background") that neither library's higher-level API bothers to
/// surface. This is a straightforward fixed-layout binary read per the
/// OpenType `CPAL` table spec (numPalettes at offset 4, a
/// `paletteTypesArrayOffset` Offset32 field appearing only in version-1
/// tables at a fixed offset past the version-0 header, then
/// `uint32[numPalettes]` flag words) -- equivalent to what
/// `FT_Palette_Data_Get`'s `FT_Palette_Data::flags` already extracts on
/// the FreeType side. Returns an all-`false` vector (rather than
/// erroring) for a version-0 `CPAL` table or any malformed/truncated
/// data, matching FreeType's graceful "just no flags" behavior for those
/// cases.
fn cpal_palette_flags(cpal_table: &[u8], num_palettes: u16) -> Vec<u32> {
    let read_u16 = |off: usize| -> Option<u16> {
        cpal_table.get(off..off + 2).map(|b| u16::from_be_bytes([b[0], b[1]]))
    };
    let read_u32 = |off: usize| -> Option<u32> {
        cpal_table
            .get(off..off + 4)
            .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    };

    let mut flags = vec![0u32; num_palettes as usize];
    let version = match read_u16(0) {
        Some(v) => v,
        None => return flags,
    };
    if version < 1 {
        return flags;
    }
    // Version-1 header: version(2) numPaletteEntries(2) numPalettes(2)
    // numColorRecords(2) colorRecordsArrayOffset(4)
    // colorRecordIndices(2*numPalettes) paletteTypesArrayOffset(4) ...
    let types_offset_field = 2 + 2 + 2 + 2 + 4 + 2 * num_palettes as usize;
    let Some(types_offset) = read_u32(types_offset_field) else {
        return flags;
    };
    if types_offset == 0 {
        return flags;
    }
    for (i, flag) in flags.iter_mut().enumerate() {
        if let Some(v) = read_u32(types_offset as usize + i * 4) {
            *flag = v;
        }
    }
    flags
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
        Ok(Self {
            data,
            offset,
            key,
            face_index: index,
        })
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
    /// `test::underline_metrics_match_known_values`): for
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

    /// Equivalent of `ftwrap::Face::set_font_size`: the full set of
    /// scaled per-size metrics `RustybuzzShaper::metrics_for_idx` needs,
    /// not just the cell width/height that [`SwashFontInfo::cell_metrics`]
    /// returns.
    ///
    /// ## Bitmap-strike-only fonts
    ///
    /// `ftwrap::Face::set_font_size` has a fallback path for fonts with no
    /// scalable outlines at all (e.g. legacy `.otb`/PCF-style embedded
    /// bitmap fonts -- not to be confused with COLR/CBDT/sbix *color*
    /// fonts, which do have ordinary scalable metrics tables even though
    /// their glyph *images* are bitmaps): when FreeType's
    /// `FT_Set_Char_Size` fails outright, it instead selects the closest
    /// available bitmap strike (`available_sizes`) and reports that
    /// strike's dimensions. `units_per_em == 0` in swash's `Metrics` is
    /// the closest available signal for "this font has no usable
    /// scalable metrics" (the same signal [`SwashFontInfo::cap_height_ratio`]
    /// already relies on for its own "not applicable" case), so this
    /// mirrors that fallback by picking the nearest [`SwashFontInfo::pixel_sizes`]
    /// entry instead of scaling `Metrics` when `units_per_em == 0`.
    pub fn selected_font_size(&self, point_size: f64, dpi: u32) -> SwashSelectedFontSize {
        let font = self.as_font_ref();
        let metrics = font.metrics(&[]);
        let pixel_height = point_size * dpi as f64 / 72.0;

        if metrics.units_per_em == 0 {
            // Bitmap-strike-only fallback, mirroring ftwrap's strike
            // selection loop: pick the pixel_sizes() entry closest to the
            // requested pixel height, and fall back to the (unscaled,
            // since there's nothing to scale) cell_metrics for anything
            // scale-shaped we can't otherwise derive.
            let sizes = self.pixel_sizes();
            let best_height = sizes
                .iter()
                .min_by_key(|&&sz| ((sz as f64) - pixel_height).abs() as i64)
                .copied()
                .unwrap_or(pixel_height as u16) as f64;
            let cell = self.cell_metrics(point_size, dpi);
            return SwashSelectedFontSize {
                width: cell.width.max(best_height),
                height: best_height.max(cell.height),
                descender: 0.,
                underline_thickness: 0.,
                underline_position: 0.,
                cap_height: None,
                cap_height_to_height_ratio: None,
                is_scaled: false,
            };
        }

        let ppem = pixel_height as f32;
        let scaled = metrics.scale(ppem);
        let cell = self.cell_metrics(point_size, dpi);
        let cap_height_ratio = self.cap_height_ratio();

        SwashSelectedFontSize {
            width: cell.width,
            height: cell.height,
            descender: -(scaled.descent as f64),
            underline_thickness: scaled.stroke_size as f64,
            // FreeType-compatible adjustment (top-edge -> stroke-center),
            // matching `SwashFontInfo::metrics`'s own `underline_position`
            // field.
            underline_position: (scaled.underline_offset - scaled.stroke_size / 2.0) as f64,
            cap_height: cap_height_ratio.map(|ratio| ratio * cell.height),
            cap_height_to_height_ratio: cap_height_ratio,
            is_scaled: true,
        }
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
                user_values: instance.values().collect(),
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
        let num_palettes = font.color_palettes().len() as u16;
        // swash's public API has no raw-table-bytes accessor (`table_data`
        // is an internal-only trait method - see `cpal_palette_flags`'s
        // doc comment), so fetch the raw `CPAL` bytes via `ttf_parser`
        // instead, which is already a direct dependency for the COLR
        // paint-graph rasterizer (`rasterizer/colr_paint.rs`).
        let cpal_data = ttf_parser::Face::parse(&self.data, self.face_index)
            .ok()
            .and_then(|f| f.raw_face().table(ttf_parser::Tag::from_bytes(b"CPAL")))
            .unwrap_or(&[]);
        let flags = cpal_palette_flags(cpal_data, num_palettes);

        let mut out = vec![];
        for palette in font.color_palettes() {
            let mut entries = vec![];
            for i in 0..palette.len() {
                entries.push(palette.get(i));
            }
            let flag = flags.get(palette.index() as usize).copied().unwrap_or(0);
            out.push(SwashPalette {
                index: palette.index(),
                name: palette.name(None).map(|s| s.to_string()),
                entries,
                usable_with_light_bg: (flag & 0x1) != 0,
                usable_with_dark_bg: (flag & 0x2) != 0,
            });
        }
        out
    }

    /// Equivalent of checking `FT_FACE_FLAG_SVG` on
    /// `(*face.face).face_flags`: does this face carry an `SVG ` table
    /// (glyphs defined as SVG documents, as opposed to COLR/CBDT/sbix
    /// bitmap or outline color)? swash has no face-level "has an SVG
    /// table" query (its `Scaler`/`Outline` APIs are per-glyph, not
    /// per-face), so this parses the same underlying bytes with
    /// `ttf_parser` (already a workspace dependency via `rustybuzz`) just
    /// to check `FaceTables::svg.is_some()` -- a cheap, read-only table
    /// directory lookup, not a second full font parse in any expensive
    /// sense.
    pub fn has_svg(&self) -> bool {
        ttf_parser::Face::parse(&self.data, self.face_index)
            .map(|f| f.tables().svg.is_some())
            .unwrap_or(false)
    }

    /// Equivalent of checking `FT_FACE_FLAG_COLOR` on
    /// `(*face.face).face_flags`: does this face carry a `COLR` (v0 or
    /// v1) table? See [`SwashFontInfo::has_svg`] for why this goes
    /// through `ttf_parser` rather than swash directly.
    pub fn has_color(&self) -> bool {
        ttf_parser::Face::parse(&self.data, self.face_index)
            .map(|f| f.tables().colr.is_some())
            .unwrap_or(false)
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
    use crate::locator::FontDataSource;
    use std::path::PathBuf;

    /// All of the reference fonts checked into `assets/fonts/`, chosen to
    /// cover: a monospace variable-adjacent family with many static
    /// weights (JetBrains Mono), a proportional family with italics
    /// (Roboto), a ligature-heavy monospace font (FiraCode), a large
    /// CJK/symbol test font (SymbolsNerdFontMono), and a color emoji font
    /// with no useful cmap-coverage overlap with the others
    /// (NotoColorEmoji) -- deliberately excluded from the advance-width/
    /// cmap parity loop below since Latin letters/digits are not a
    /// meaningful test of an emoji font's own glyph coverage. Note: this
    /// specific `assets/fonts/NotoColorEmoji.ttf` file turns out (per
    /// direct inspection of its `sfnt` table directory, done while
    /// converting this module's tests off FreeType) to be a COLR/CPAL
    /// scalable-color-outline build rather than the CBDT/CBLC
    /// bitmap-strike build the name might suggest -- see
    /// [`pixel_sizes_for_color_emoji`]'s doc comment for the full
    /// explanation; its inclusion here is still exercised via a
    /// dedicated test.
    ///
    /// The tests below no longer compare against FreeType at runtime --
    /// `ftwrap.rs`/FreeType have been removed from this crate as part of
    /// phase H4. Instead, each test asserts against a hardcoded baseline
    /// value. Those baselines were captured by running this same
    /// `SwashFontInfo` code (nothing has changed about *how* the values
    /// are computed, only that there's no more live FreeType oracle to
    /// diff against) and, in turn, were originally verified bit-for-bit
    /// (or within the documented tolerance) against FreeType during
    /// phase H2 -- see the historical notes retained on individual tests
    /// below for the specific discrepancies that were found and
    /// investigated at that time. The purpose of keeping these baselines
    /// as hardcoded constants rather than deleting the tests is to catch
    /// any *future* regression (e.g. a swash upgrade or refactor here)
    /// that silently changes these metrics.
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

    fn swash_info_for(name: &str) -> SwashFontInfo {
        let source = FontDataSource::OnDisk(asset_path(name));
        SwashFontInfo::from_locator(&source, 0).unwrap()
    }

    /// Baseline `units_per_em()` per reference font, captured from a live
    /// run of this module's `SwashFontInfo::units_per_em` (originally
    /// verified to match FreeType's `(*face).units_per_EM` exactly during
    /// phase H2).
    #[test]
    fn units_per_em_matches_known_values() {
        let expected: &[(&str, u16)] = &[
            ("JetBrainsMono-Regular.ttf", 1000),
            ("JetBrainsMono-Bold.ttf", 1000),
            ("JetBrainsMono-Italic.ttf", 1000),
            ("Roboto-Regular.ttf", 2048),
            ("Roboto-Bold.ttf", 2048),
            ("Roboto-Italic.ttf", 2048),
            ("FiraCode-Regular.ttf", 1950),
            ("SymbolsNerdFontMono-Regular.ttf", 2048),
        ];
        for &(name, expected_upem) in expected {
            let swash_info = swash_info_for(name);
            let swash_upem = swash_info.units_per_em();
            assert_eq!(
                expected_upem, swash_upem,
                "units_per_em regression for {name}: expected={expected_upem} swash={swash_upem}"
            );
        }
    }

    /// Baseline `num_glyphs()` per reference font (originally verified to
    /// match FreeType's `(*face).num_glyphs` exactly during phase H2).
    #[test]
    fn num_glyphs_matches_known_values() {
        let expected: &[(&str, u16)] = &[
            ("JetBrainsMono-Regular.ttf", 1743),
            ("JetBrainsMono-Bold.ttf", 1743),
            ("JetBrainsMono-Italic.ttf", 1730),
            ("Roboto-Regular.ttf", 1294),
            ("Roboto-Bold.ttf", 1294),
            ("Roboto-Italic.ttf", 1294),
            ("FiraCode-Regular.ttf", 2030),
            ("SymbolsNerdFontMono-Regular.ttf", 10400),
        ];
        for &(name, expected_glyphs) in expected {
            let swash_info = swash_info_for(name);
            let swash_num_glyphs = swash_info.num_glyphs();
            assert_eq!(
                expected_glyphs, swash_num_glyphs,
                "num_glyphs regression for {name}: expected={expected_glyphs} swash={swash_num_glyphs}"
            );
        }
    }

    /// cmap codepoint -> glyph_id coverage. This is the metric with the
    /// highest blast radius if wrong: a mismatched/missing glyph id means
    /// the wrong glyph is drawn (or nothing at all), not just a
    /// slightly-off position. Rather than hardcoding every glyph id for
    /// ~200 sample characters (as the original FreeType-comparison
    /// version of this test did), we check the functionally-relevant
    /// property -- that a representative subset (letters, digits, the
    /// punctuation set actually used elsewhere) maps to *some* glyph --
    /// plus a tight regression anchor (the exact glyph id for 'A' and
    /// '0') per font, captured from a live run of this code (originally
    /// verified 1:1 against FreeType's `FT_Get_Char_Index` for the full
    /// ~200-character sample during phase H2).
    ///
    /// `SymbolsNerdFontMono-Regular.ttf` is a special case: it is a
    /// glyph-forwarding "Nerd Font" patch that (as confirmed by a live
    /// run of this code) does *not* map plain ASCII letters/digits or
    /// this punctuation subset through its Unicode cmap at all --
    /// `glyph_id_for_char` returns 0 for all of them, since this font
    /// only carries private-use-area glyphs (icons/symbols) and is meant
    /// to be merged with a "real" text font rather than used standalone
    /// for Latin text. That is expected, not a bug, so it is excluded
    /// from the "maps to a nonzero glyph" assertion below and only
    /// exercised by [`cmap_lookup_matches_known_values`]'s general
    /// [`num_glyphs_matches_known_values`]/[`units_per_em_matches_known_values`]
    /// coverage instead.
    #[test]
    fn cmap_lookup_matches_known_values() {
        let expected_anchors: &[(&str, u16, u16)] = &[
            // (name, glyph_id('A'), glyph_id('0'))
            ("JetBrainsMono-Regular.ttf", 1, 724),
            ("JetBrainsMono-Bold.ttf", 1, 724),
            ("JetBrainsMono-Italic.ttf", 1, 713),
            ("Roboto-Regular.ttf", 37, 20),
            ("Roboto-Bold.ttf", 37, 20),
            ("Roboto-Italic.ttf", 37, 20),
            ("FiraCode-Regular.ttf", 1, 1005),
            // SymbolsNerdFontMono has no ASCII cmap coverage at all -- see
            // this test's doc comment.
            ("SymbolsNerdFontMono-Regular.ttf", 0, 0),
        ];

        for &(name, expected_a, expected_0) in expected_anchors {
            let swash_info = swash_info_for(name);

            let samples: Vec<char> = ('A'..='Z')
                .chain('0'..='9')
                .chain(['—', '–', '\u{201c}', '\u{201d}', '\u{2018}', '\u{2019}', '…', '€'])
                .collect();

            if name != "SymbolsNerdFontMono-Regular.ttf" {
                for c in samples {
                    let gid = swash_info.glyph_id_for_char(c);
                    assert_ne!(
                        gid, 0,
                        "{name} unexpectedly has no glyph for char={c:?} (U+{:04X})",
                        c as u32
                    );
                }
            }

            let gid_a = swash_info.glyph_id_for_char('A');
            let gid_0 = swash_info.glyph_id_for_char('0');
            assert_eq!(
                expected_a, gid_a,
                "glyph_id('A') regression for {name}: expected={expected_a} swash={gid_a}"
            );
            assert_eq!(
                expected_0, gid_0,
                "glyph_id('0') regression for {name}: expected={expected_0} swash={gid_0}"
            );
        }
    }

    /// Advance width (the metric that directly determines terminal cell
    /// width -- the single least-negotiable metric in the whole
    /// migration) at a representative set of point sizes/dpis for a
    /// representative letter subset, compared against hardcoded baseline
    /// pixel values (captured from a live run of this code).
    ///
    /// ## Historical note: sub-pixel rounding difference vs. FreeType (not a bug)
    ///
    /// During phase H2, even with hinting fully disabled on both sides,
    /// we found the scaled advance was not always bit-for-bit identical
    /// between swash and FreeType: e.g. `Roboto-Regular.ttf` 'A' at
    /// 14pt/96dpi (ppem 18.667px) gave FreeType `12.1875` (`780/64`, a
    /// clean 26.6 fixed-point value) vs. swash `12.177083...` (matching
    /// the "ideal" `1336 * 18.667 / 2048` floating point product,
    /// verified independently against the raw `hmtx`/`head` table
    /// values). The ~0.0104px difference came from FreeType computing and
    /// rounding its 16.16 fixed-point `size->metrics.x_scale` once per
    /// size (then multiplying that already-rounded scale by each glyph's
    /// raw advance), whereas swash (`GlyphMetrics::advance_width`/
    /// `Metrics::scale`) computes the scale as an `f32` and multiplies
    /// without an intermediate fixed-point rounding step. Neither side
    /// was "wrong" -- it's an inherent difference between a fixed-point
    /// and a floating-point scaling pipeline, far smaller than a single
    /// pixel, so it did not threaten cell-grid alignment (which cares
    /// about the *rounded-to-integer-pixel* cell width, not the
    /// sub-pixel-exact unhinted advance). Now that there is no live
    /// FreeType oracle, this test simply pins swash's own values with a
    /// small epsilon to catch float-rounding jitter across platforms/
    /// swash versions, not to re-litigate that historical comparison.
    #[test]
    fn advance_width_matches_known_values() {
        // (font, point_size, dpi, char, expected_advance_px). Note:
        // `SymbolsNerdFontMono-Regular.ttf` is intentionally excluded --
        // see `cmap_lookup_matches_known_values`'s doc comment: it has no
        // ASCII cmap coverage, so 'A'/'M'/'i'/'W' all map to glyph 0
        // there (its glyph 0 -- the notdef/box glyph -- does still
        // report an advance, since `advance_width(0)` is a valid,
        // well-defined call, but that's not a meaningful "letter
        // advance" regression anchor).
        let expected: &[(&str, f64, u32, char, f64)] = &[
            ("JetBrainsMono-Regular.ttf", 10.0, 72, 'A', 6.0),
            ("JetBrainsMono-Regular.ttf", 10.0, 72, 'M', 6.0),
            ("JetBrainsMono-Regular.ttf", 10.0, 72, 'i', 6.0),
            ("JetBrainsMono-Regular.ttf", 10.0, 72, 'W', 6.0),
            ("JetBrainsMono-Regular.ttf", 14.0, 96, 'A', 11.199999809265137),
            ("JetBrainsMono-Regular.ttf", 14.0, 96, 'M', 11.199999809265137),
            ("JetBrainsMono-Regular.ttf", 14.0, 96, 'i', 11.199999809265137),
            ("JetBrainsMono-Regular.ttf", 14.0, 96, 'W', 11.199999809265137),
            ("JetBrainsMono-Regular.ttf", 24.0, 144, 'A', 28.80000114440918),
            ("JetBrainsMono-Regular.ttf", 24.0, 144, 'M', 28.80000114440918),
            ("JetBrainsMono-Regular.ttf", 24.0, 144, 'i', 28.80000114440918),
            ("JetBrainsMono-Regular.ttf", 24.0, 144, 'W', 28.80000114440918),
            ("JetBrainsMono-Bold.ttf", 10.0, 72, 'A', 6.0),
            ("JetBrainsMono-Bold.ttf", 10.0, 72, 'M', 6.0),
            ("JetBrainsMono-Bold.ttf", 10.0, 72, 'i', 6.0),
            ("JetBrainsMono-Bold.ttf", 10.0, 72, 'W', 6.0),
            ("JetBrainsMono-Bold.ttf", 14.0, 96, 'A', 11.199999809265137),
            ("JetBrainsMono-Bold.ttf", 14.0, 96, 'M', 11.199999809265137),
            ("JetBrainsMono-Bold.ttf", 14.0, 96, 'i', 11.199999809265137),
            ("JetBrainsMono-Bold.ttf", 14.0, 96, 'W', 11.199999809265137),
            ("JetBrainsMono-Bold.ttf", 24.0, 144, 'A', 28.80000114440918),
            ("JetBrainsMono-Bold.ttf", 24.0, 144, 'M', 28.80000114440918),
            ("JetBrainsMono-Bold.ttf", 24.0, 144, 'i', 28.80000114440918),
            ("JetBrainsMono-Bold.ttf", 24.0, 144, 'W', 28.80000114440918),
            ("JetBrainsMono-Italic.ttf", 10.0, 72, 'A', 6.0),
            ("JetBrainsMono-Italic.ttf", 10.0, 72, 'M', 6.0),
            ("JetBrainsMono-Italic.ttf", 10.0, 72, 'i', 6.0),
            ("JetBrainsMono-Italic.ttf", 10.0, 72, 'W', 6.0),
            ("JetBrainsMono-Italic.ttf", 14.0, 96, 'A', 11.199999809265137),
            ("JetBrainsMono-Italic.ttf", 14.0, 96, 'M', 11.199999809265137),
            ("JetBrainsMono-Italic.ttf", 14.0, 96, 'i', 11.199999809265137),
            ("JetBrainsMono-Italic.ttf", 14.0, 96, 'W', 11.199999809265137),
            ("JetBrainsMono-Italic.ttf", 24.0, 144, 'A', 28.80000114440918),
            ("JetBrainsMono-Italic.ttf", 24.0, 144, 'M', 28.80000114440918),
            ("JetBrainsMono-Italic.ttf", 24.0, 144, 'i', 28.80000114440918),
            ("JetBrainsMono-Italic.ttf", 24.0, 144, 'W', 28.80000114440918),
            ("Roboto-Regular.ttf", 10.0, 72, 'A', 6.5234375),
            ("Roboto-Regular.ttf", 10.0, 72, 'M', 8.73046875),
            ("Roboto-Regular.ttf", 10.0, 72, 'i', 2.4267578125),
            ("Roboto-Regular.ttf", 10.0, 72, 'W', 8.8720703125),
            ("Roboto-Regular.ttf", 14.0, 96, 'A', 12.177083015441895),
            ("Roboto-Regular.ttf", 14.0, 96, 'M', 16.296875),
            ("Roboto-Regular.ttf", 14.0, 96, 'i', 4.529947757720947),
            ("Roboto-Regular.ttf", 14.0, 96, 'W', 16.56119728088379),
            ("Roboto-Regular.ttf", 24.0, 144, 'A', 31.3125),
            ("Roboto-Regular.ttf", 24.0, 144, 'M', 41.90625),
            ("Roboto-Regular.ttf", 24.0, 144, 'i', 11.6484375),
            ("Roboto-Regular.ttf", 24.0, 144, 'W', 42.5859375),
            ("Roboto-Bold.ttf", 10.0, 72, 'A', 6.728515625),
            ("Roboto-Bold.ttf", 10.0, 72, 'M', 8.759765625),
            ("Roboto-Bold.ttf", 10.0, 72, 'i', 2.6513671875),
            ("Roboto-Bold.ttf", 10.0, 72, 'W', 8.7451171875),
            ("Roboto-Bold.ttf", 14.0, 96, 'A', 12.559895515441895),
            ("Roboto-Bold.ttf", 14.0, 96, 'M', 16.3515625),
            ("Roboto-Bold.ttf", 14.0, 96, 'i', 4.94921875),
            ("Roboto-Bold.ttf", 14.0, 96, 'W', 16.32421875),
            ("Roboto-Bold.ttf", 24.0, 144, 'A', 32.296875),
            ("Roboto-Bold.ttf", 24.0, 144, 'M', 42.046875),
            ("Roboto-Bold.ttf", 24.0, 144, 'i', 12.7265625),
            ("Roboto-Bold.ttf", 24.0, 144, 'W', 41.9765625),
            ("Roboto-Italic.ttf", 10.0, 72, 'A', 6.376953125),
            ("Roboto-Italic.ttf", 10.0, 72, 'M', 8.515625),
            ("Roboto-Italic.ttf", 10.0, 72, 'i', 2.40234375),
            ("Roboto-Italic.ttf", 10.0, 72, 'W', 8.65234375),
            ("Roboto-Italic.ttf", 14.0, 96, 'A', 11.903645515441895),
            ("Roboto-Italic.ttf", 14.0, 96, 'M', 15.895833015441895),
            ("Roboto-Italic.ttf", 14.0, 96, 'i', 4.484375),
            ("Roboto-Italic.ttf", 14.0, 96, 'W', 16.15104103088379),
            ("Roboto-Italic.ttf", 24.0, 144, 'A', 30.609375),
            ("Roboto-Italic.ttf", 24.0, 144, 'M', 40.875),
            ("Roboto-Italic.ttf", 24.0, 144, 'i', 11.53125),
            ("Roboto-Italic.ttf", 24.0, 144, 'W', 41.53125),
            ("FiraCode-Regular.ttf", 10.0, 72, 'A', 6.153846263885498),
            ("FiraCode-Regular.ttf", 10.0, 72, 'M', 6.153846263885498),
            ("FiraCode-Regular.ttf", 10.0, 72, 'i', 6.153846263885498),
            ("FiraCode-Regular.ttf", 10.0, 72, 'W', 6.153846263885498),
            ("FiraCode-Regular.ttf", 14.0, 96, 'A', 11.487178802490234),
            ("FiraCode-Regular.ttf", 14.0, 96, 'M', 11.487178802490234),
            ("FiraCode-Regular.ttf", 14.0, 96, 'i', 11.487178802490234),
            ("FiraCode-Regular.ttf", 14.0, 96, 'W', 11.487178802490234),
            ("FiraCode-Regular.ttf", 24.0, 144, 'A', 29.538461685180664),
            ("FiraCode-Regular.ttf", 24.0, 144, 'M', 29.538461685180664),
            ("FiraCode-Regular.ttf", 24.0, 144, 'i', 29.538461685180664),
            ("FiraCode-Regular.ttf", 24.0, 144, 'W', 29.538461685180664),
        ];

        for &(name, point_size, dpi, c, expected_advance) in expected {
            let swash_info = swash_info_for(name);
            let font = swash_info.as_font_ref();
            let charmap = font.charmap();
            let pixel_height = point_size * dpi as f64 / 72.0;
            let ppem = pixel_height as f32;
            let glyph_metrics = font.glyph_metrics(&[]).scale(ppem);

            let gid = charmap.map(c);
            assert_ne!(gid, 0, "{name} has no glyph for char={c:?}");
            let swash_advance = glyph_metrics.advance_width(gid) as f64;

            let diff = (expected_advance - swash_advance).abs();
            assert!(
                diff < 0.01,
                "advance_width regression for {name} char={c:?} at size={point_size} dpi={dpi}: \
                 expected={expected_advance} swash={swash_advance} diff={diff}"
            );
        }
    }

    /// cap_height ratio (sCapHeight / unitsPerEm) baseline -- a pure
    /// OS/2-table field read, not a rasterization-derived value, so
    /// there is no hinting-related excuse for any divergence from the
    /// hardcoded baseline (originally verified to match FreeType's
    /// derived cap-height ratio exactly during phase H2).
    #[test]
    fn cap_height_ratio_matches_known_values() {
        let expected: &[(&str, Option<f64>)] = &[
            ("JetBrainsMono-Regular.ttf", Some(0.73)),
            ("JetBrainsMono-Bold.ttf", Some(0.73)),
            ("JetBrainsMono-Italic.ttf", Some(0.73)),
            ("Roboto-Regular.ttf", Some(0.7109375)),
            ("Roboto-Bold.ttf", Some(0.7109375)),
            ("Roboto-Italic.ttf", Some(0.7109375)),
            ("FiraCode-Regular.ttf", Some(0.7061538461538461)),
            // SymbolsNerdFontMono-Regular.ttf has no OS/2 sCapHeight set
            // (or it is 0), so this correctly reports `None`, matching
            // the many symbol-only fonts FreeType also reports `None`
            // for.
            ("SymbolsNerdFontMono-Regular.ttf", None),
        ];
        for &(name, expected_ratio) in expected {
            let swash_info = swash_info_for(name);
            let swash_ratio = swash_info.cap_height_ratio();

            match (expected_ratio, swash_ratio) {
                (Some(expected), Some(sw)) => {
                    assert!(
                        (expected - sw).abs() < 1e-6,
                        "cap_height ratio regression for {name}: expected={expected} swash={sw}"
                    );
                }
                (None, None) => {}
                (expected, sw) => panic!(
                    "cap_height presence regression for {name}: expected={expected:?} swash={sw:?}"
                ),
            }
        }
    }

    /// `italic()` (OS/2 fsSelection-derived) baseline (originally
    /// verified to match FreeType's `italic()` exactly during phase H2).
    #[test]
    fn italic_flag_matches_known_values() {
        let expected: &[(&str, bool)] = &[
            ("JetBrainsMono-Regular.ttf", false),
            ("JetBrainsMono-Bold.ttf", false),
            ("JetBrainsMono-Italic.ttf", true),
            ("Roboto-Regular.ttf", false),
            ("Roboto-Bold.ttf", false),
            ("Roboto-Italic.ttf", true),
            ("FiraCode-Regular.ttf", false),
            ("SymbolsNerdFontMono-Regular.ttf", false),
        ];
        for &(name, expected_italic) in expected {
            let swash_info = swash_info_for(name);
            assert_eq!(
                expected_italic,
                swash_info.is_italic(),
                "italic flag regression for {name}"
            );
        }
    }

    /// `weight_and_width()` (OS/2 usWeightClass/usWidthClass) baseline
    /// for static (non-variable) fonts (originally verified to match
    /// FreeType's `weight_and_width()` exactly during phase H2).
    #[test]
    fn weight_and_width_matches_known_values() {
        let expected: &[(&str, u16, u16)] = &[
            ("JetBrainsMono-Regular.ttf", 400, 5),
            ("JetBrainsMono-Bold.ttf", 700, 5),
            ("JetBrainsMono-Italic.ttf", 400, 5),
            ("Roboto-Regular.ttf", 400, 5),
            ("Roboto-Bold.ttf", 700, 5),
            ("Roboto-Italic.ttf", 400, 5),
            ("FiraCode-Regular.ttf", 400, 5),
            ("SymbolsNerdFontMono-Regular.ttf", 400, 5),
        ];
        for &(name, expected_weight, expected_width) in expected {
            let swash_info = swash_info_for(name);
            let (swash_weight, swash_width) = swash_info.weight_and_width(None);

            assert_eq!(
                expected_weight, swash_weight,
                "weight regression for {name}: expected={expected_weight} swash={swash_weight}"
            );
            assert_eq!(
                expected_width, swash_width,
                "width regression for {name}: expected={expected_width} swash={swash_width}"
            );
        }
    }

    /// Family/postscript name baseline (originally verified to match
    /// FreeType's `family_name()`/`postscript_name()` exactly during
    /// phase H2, modulo swash preferring the typographic family/subfamily
    /// name ids, which for all of our reference fonts fall back to the
    /// same value as the legacy name ids since none of them set distinct
    /// typographic names).
    #[test]
    fn names_match_known_values() {
        let expected: &[(&str, &str, &str)] = &[
            (
                "JetBrainsMono-Regular.ttf",
                "JetBrains Mono",
                "JetBrainsMono-Regular",
            ),
            (
                "JetBrainsMono-Bold.ttf",
                "JetBrains Mono",
                "JetBrainsMono-Bold",
            ),
            (
                "JetBrainsMono-Italic.ttf",
                "JetBrains Mono",
                "JetBrainsMono-Italic",
            ),
            ("Roboto-Regular.ttf", "Roboto", "Roboto-Regular"),
            ("Roboto-Bold.ttf", "Roboto", "Roboto-Bold"),
            ("Roboto-Italic.ttf", "Roboto", "Roboto-Italic"),
            ("FiraCode-Regular.ttf", "Fira Code", "FiraCode-Regular"),
            (
                "SymbolsNerdFontMono-Regular.ttf",
                "Symbols Nerd Font Mono",
                "SymbolsNFM",
            ),
        ];
        for &(name, expected_family, expected_postscript) in expected {
            let swash_info = swash_info_for(name);

            assert_eq!(
                expected_family,
                swash_info.family_name(),
                "family_name regression for {name}"
            );
            assert_eq!(
                expected_postscript,
                swash_info.postscript_name(),
                "postscript_name regression for {name}"
            );
        }
    }

    /// Underline position/thickness baseline (from the `post` table).
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
    /// meaning", per that file's comment). This was a **confirmed, real
    /// discrepancy** found while originally writing this module's tests
    /// (phase H2): for `JetBrainsMono-Regular.ttf`, the raw/swash value
    /// is -155 design units, FreeType reported -180 (with
    /// `underlineThickness = 50`, `-155 - 50/2 == -180`, confirmed by
    /// direct inspection of the font's `post` table). It is not a bug in
    /// either library, just an intentional semantic difference in what
    /// "underline position" means. To reproduce FreeType's value/behavior
    /// exactly (required, since wezterm's cell-underline placement is
    /// presumably tuned against FreeType's convention), [`SwashFontInfo::metrics`]
    /// applies the same adjustment: `underline_offset - stroke_size /
    /// 2.0`. That adjustment is still applied (see `metrics()` above);
    /// this test now just pins the resulting value as a baseline, since
    /// there is no live FreeType to diff against any more.
    #[test]
    fn underline_metrics_match_known_values() {
        let expected: &[(&str, i32, i32)] = &[
            ("JetBrainsMono-Regular.ttf", -180, 50),
            ("JetBrainsMono-Bold.ttf", -180, 50),
            ("JetBrainsMono-Italic.ttf", -180, 50),
            ("Roboto-Regular.ttf", -200, 100),
            ("Roboto-Bold.ttf", -200, 100),
            ("Roboto-Italic.ttf", -200, 100),
            ("FiraCode-Regular.ttf", -125, 50),
            ("SymbolsNerdFontMono-Regular.ttf", -306, 102),
        ];
        for &(name, expected_position, expected_thickness) in expected {
            let swash_info = swash_info_for(name);
            let swash_metrics = swash_info.metrics();

            assert_eq!(
                expected_position, swash_metrics.underline_position as i32,
                "underline_position regression for {name}"
            );
            assert_eq!(
                expected_thickness, swash_metrics.underline_thickness as i32,
                "underline_thickness regression for {name}"
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
    /// reference fonts (no embedded bitmap strikes). All 8 reference
    /// fonts are expected to report no bitmap strikes at all, so unlike
    /// the other tests in this module there is no per-font baseline
    /// table to hardcode -- `is_empty()` is the expected value
    /// unconditionally.
    #[test]
    fn pixel_sizes_empty_for_scalable_fonts() {
        for name in reference_fonts() {
            let swash_info = swash_info_for(name);
            assert!(
                swash_info.pixel_sizes().is_empty(),
                "pixel_sizes unexpectedly non-empty for {name}: {:?}",
                swash_info.pixel_sizes()
            );
        }
    }

    /// `assets/fonts/NotoColorEmoji.ttf` (the specific file checked into
    /// this repo, confirmed by direct inspection of its `sfnt` table
    /// directory: `COLR`/`CPAL`/`glyf`/`loca`, no `CBDT`/`CBLC`/`sbix`)
    /// carries **scalable COLR/CPAL color outlines**, not embedded
    /// bitmap strikes, despite the "NotoColorEmoji" name -- Google
    /// distributes both a CBDT/CBLC bitmap-strike build and a
    /// COLR/CPAL-only vector build under that same filename across
    /// releases, and this repo's copy is the latter. So, unlike what an
    /// eponymous "bitmap emoji font" might suggest,
    /// [`SwashFontInfo::pixel_sizes`] (which only enumerates
    /// `color_strikes()`/`alpha_strikes()`, i.e. `CBDT`/`sbix`-style
    /// bitmap strikes) legitimately returns an empty `Vec` here too --
    /// confirmed by a live run of this code. This test now just pins
    /// that as the expected baseline for this specific asset file rather
    /// than asserting non-emptiness (there is, as of this writing, no
    /// bitmap-strike-only reference font in `assets/fonts/` to exercise
    /// the non-empty branch; should one be added later, this test name
    /// and body should be revisited).
    #[test]
    fn pixel_sizes_for_color_emoji() {
        let name = "NotoColorEmoji.ttf";
        let swash_info = swash_info_for(name);

        let swash_sizes = swash_info.pixel_sizes();
        let expected_sizes: Vec<u16> = vec![];

        assert_eq!(
            expected_sizes, swash_sizes,
            "pixel_sizes regression for {name}: expected={expected_sizes:?} swash={swash_sizes:?}"
        );
    }

    /// cell_metrics()/set_font_size() nominal monospace cell size, which
    /// directly drives terminal cell width/height -- this is the metric
    /// singled out in the migration plan's acceptance criteria as
    /// requiring *exact* parity, not visual approximation. Originally
    /// verified against FreeType's `set_font_size`-derived cell metrics
    /// within a 1px tolerance (to account for FreeType's hinted-advance
    /// rounding) during phase H2; now pinned against a hardcoded
    /// baseline captured from a live run of this same (unhinted, swash-
    /// only) `cell_metrics` code, with a tighter epsilon than that
    /// historical 1px FreeType-hinting tolerance since there is no more
    /// hinting-vs-unhinted discrepancy to accommodate on this side.
    #[test]
    fn cell_metrics_matches_known_values() {
        let expected: &[(&str, f64, u32, f64, f64)] = &[
            ("JetBrainsMono-Regular.ttf", 10.0, 72, 6.0, 13.199999809265137),
            (
                "JetBrainsMono-Regular.ttf",
                14.0,
                96,
                11.199999809265137,
                24.639999389648438,
            ),
            ("JetBrainsMono-Bold.ttf", 10.0, 72, 6.0, 13.199999809265137),
            (
                "JetBrainsMono-Bold.ttf",
                14.0,
                96,
                11.199999809265137,
                24.639999389648438,
            ),
            ("JetBrainsMono-Italic.ttf", 10.0, 72, 6.0, 13.199999809265137),
            (
                "JetBrainsMono-Italic.ttf",
                14.0,
                96,
                11.199999809265137,
                24.639999389648438,
            ),
            (
                "Roboto-Regular.ttf",
                10.0,
                72,
                8.9794921875,
                11.71875,
            ),
            (
                "Roboto-Regular.ttf",
                14.0,
                96,
                16.76171875,
                21.874998092651367,
            ),
            ("Roboto-Bold.ttf", 10.0, 72, 8.9501953125, 11.71875),
            (
                "Roboto-Bold.ttf",
                14.0,
                96,
                16.70703125,
                21.874998092651367,
            ),
            ("Roboto-Italic.ttf", 10.0, 72, 8.759765625, 11.71875),
            (
                "Roboto-Italic.ttf",
                14.0,
                96,
                16.3515625,
                21.874998092651367,
            ),
            (
                "FiraCode-Regular.ttf",
                10.0,
                72,
                6.153846263885498,
                12.307692527770996,
            ),
            (
                "FiraCode-Regular.ttf",
                14.0,
                96,
                11.487178802490234,
                22.97435760498047,
            ),
            ("SymbolsNerdFontMono-Regular.ttf", 10.0, 72, 10.0, 10.0),
            (
                "SymbolsNerdFontMono-Regular.ttf",
                14.0,
                96,
                18.66666603088379,
                18.66666603088379,
            ),
        ];

        for &(name, point_size, dpi, expected_width, expected_height) in expected {
            let swash_info = swash_info_for(name);
            let swash_cell = swash_info.cell_metrics(point_size, dpi);

            let width_diff = (expected_width - swash_cell.width).abs();
            assert!(
                width_diff < 0.01,
                "cell width regression for {name} at size={point_size} dpi={dpi}: \
                 expected={expected_width} swash={} diff={width_diff}",
                swash_cell.width
            );

            let height_diff = (expected_height - swash_cell.height).abs();
            assert!(
                height_diff < 0.01,
                "cell height regression for {name} at size={point_size} dpi={dpi}: \
                 expected={expected_height} swash={} diff={height_diff}",
                swash_cell.height
            );
        }
    }
}
