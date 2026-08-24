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
use std::ops::Deref;
use std::sync::Arc;
use swash::{Attributes, CacheKey, FontRef, Style};

/// Backing storage for [`SwashFontInfo`]'s raw font bytes: either a
/// heap-allocated owned buffer (the historical path, still used when the
/// caller already has the bytes some other way, e.g. `BuiltIn`/`Memory`
/// sources, or when the data needs to outlive a transient scan) or a
/// read-only memory map of an on-disk file, shared via `Arc` so that every
/// sub-face of a `.ttc`/`.otc` collection can reuse the same mapping
/// without re-mapping or copying.
///
/// Enumerating font directories (`ls-fonts --list-system`,
/// `FontDatabase::with_font_dirs`) only needs a handful of small tables
/// (`name`, `head`, `OS/2`, `CPAL`, plus a cmap walk for coverage) out of
/// each file, but large CJK system font collections routinely run into
/// the tens of megabytes (e.g. `mingliub.ttc` at ~37MB). Reading the whole
/// file with `std::fs::read` (the previous approach, still used via
/// [`FontDataSource::load_data`]) forces the OS to fault in and copy every
/// page even though the parser only ever touches a small fraction of
/// them. Memory-mapping the file instead lets the OS demand-page only the
/// ranges the parser actually dereferences, which is where the measured
/// win comes from (see task #319 investigation notes on
/// `parse_and_collect_font_info`).
enum FontBytes {
    Owned(Box<[u8]>),
    Mapped(Arc<memmap2::Mmap>),
}

impl Deref for FontBytes {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        match self {
            FontBytes::Owned(b) => b,
            FontBytes::Mapped(m) => m,
        }
    }
}

/// Owns the raw font bytes and a swash `FontRef` positioned at a specific
/// sub-face (relevant for TrueType collections). Mirrors the role of
/// `ftwrap::Face`, but purely for parsing/metrics -- there is no
/// equivalent of FreeType's `FT_Library`/`FT_Face` handle lifetime here;
/// `swash::FontRef` is just a transient borrow over `data`.
pub struct SwashFontInfo {
    data: FontBytes,
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
        cpal_table
            .get(off..off + 2)
            .map(|b| u16::from_be_bytes([b[0], b[1]]))
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
        Self::from_data(data, index)
    }

    fn from_font_bytes(data: FontBytes, index: u32) -> anyhow::Result<Self> {
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

    /// Like [`SwashFontInfo::from_data`], but backed by a memory-mapped
    /// on-disk file instead of an owned, fully-read buffer.
    ///
    /// This is what lets `parser::parse_and_collect_font_info` (font
    /// directory enumeration -- `ls-fonts --list-system` and friends)
    /// avoid reading large `.ttc`/`.otc` collections (tens of megabytes
    /// for CJK system fonts such as `mingliub.ttc`) into memory in full
    /// just to read a `name`/`OS-2`/`head` table's worth of metadata:
    /// with a memory map, the OS only faults in the pages the parser
    /// actually touches. `mmap` is shared via `Arc` so every sub-face of
    /// a collection can reuse the same mapping (cheap `Arc::clone`, no
    /// new syscall, no copy) instead of each sub-face mapping the file
    /// again.
    pub(crate) fn from_mmap(mmap: Arc<memmap2::Mmap>, index: u32) -> anyhow::Result<Self> {
        Self::from_font_bytes(FontBytes::Mapped(mmap), index)
    }

    /// Like [`SwashFontInfo::from_locator`], but takes already-loaded font
    /// file bytes instead of reading `source` itself.
    ///
    /// This exists for callers (namely
    /// `parser::parse_and_collect_font_info`, used by font enumeration --
    /// `ls-fonts --list-system` and friends) that already have the whole
    /// file in memory (e.g. because they needed it to compute
    /// `ttf_parser::fonts_in_collection` for a `.ttc`/`.otc` collection)
    /// and iterate every sub-face index in that same file: without this,
    /// each sub-face would independently re-read (and re-heap-allocate) the
    /// *entire* file via [`SwashFontInfo::from_locator`], even though
    /// collection sub-faces all share one on-disk file. For large CJK
    /// collections (e.g. `mingliub.ttc` at ~37MB with 3 faces, or
    /// `Sitka*.ttc` with 6 faces each) that turns one file read into
    /// `num_faces + 1` full re-reads of the same bytes -- a measured,
    /// avoidable contributor to `enumerate_all_fonts`/`ls-fonts
    /// --list-system` being slower than the old FreeType-based path (see
    /// the PERF1 investigation notes).
    pub fn from_data(data: Box<[u8]>, index: u32) -> anyhow::Result<Self> {
        Self::from_font_bytes(FontBytes::Owned(data), index)
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
    /// onlyterm actually consults elsewhere.
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
    /// since onlyterm's cell-underline placement is presumably tuned
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
mod test;
