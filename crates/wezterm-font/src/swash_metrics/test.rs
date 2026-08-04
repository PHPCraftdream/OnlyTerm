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
    // wezterm-font's CARGO_MANIFEST_DIR is `<repo>/crates/wezterm-font`
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
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
            .chain([
                '—', '–', '\u{201c}', '\u{201d}', '\u{2018}', '\u{2019}', '…', '€',
            ])
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
        (
            "JetBrainsMono-Regular.ttf",
            14.0,
            96,
            'A',
            11.199999809265137,
        ),
        (
            "JetBrainsMono-Regular.ttf",
            14.0,
            96,
            'M',
            11.199999809265137,
        ),
        (
            "JetBrainsMono-Regular.ttf",
            14.0,
            96,
            'i',
            11.199999809265137,
        ),
        (
            "JetBrainsMono-Regular.ttf",
            14.0,
            96,
            'W',
            11.199999809265137,
        ),
        (
            "JetBrainsMono-Regular.ttf",
            24.0,
            144,
            'A',
            28.80000114440918,
        ),
        (
            "JetBrainsMono-Regular.ttf",
            24.0,
            144,
            'M',
            28.80000114440918,
        ),
        (
            "JetBrainsMono-Regular.ttf",
            24.0,
            144,
            'i',
            28.80000114440918,
        ),
        (
            "JetBrainsMono-Regular.ttf",
            24.0,
            144,
            'W',
            28.80000114440918,
        ),
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
        (
            "JetBrainsMono-Italic.ttf",
            14.0,
            96,
            'A',
            11.199999809265137,
        ),
        (
            "JetBrainsMono-Italic.ttf",
            14.0,
            96,
            'M',
            11.199999809265137,
        ),
        (
            "JetBrainsMono-Italic.ttf",
            14.0,
            96,
            'i',
            11.199999809265137,
        ),
        (
            "JetBrainsMono-Italic.ttf",
            14.0,
            96,
            'W',
            11.199999809265137,
        ),
        (
            "JetBrainsMono-Italic.ttf",
            24.0,
            144,
            'A',
            28.80000114440918,
        ),
        (
            "JetBrainsMono-Italic.ttf",
            24.0,
            144,
            'M',
            28.80000114440918,
        ),
        (
            "JetBrainsMono-Italic.ttf",
            24.0,
            144,
            'i',
            28.80000114440918,
        ),
        (
            "JetBrainsMono-Italic.ttf",
            24.0,
            144,
            'W',
            28.80000114440918,
        ),
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
        (
            "JetBrainsMono-Regular.ttf",
            10.0,
            72,
            6.0,
            13.199999809265137,
        ),
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
        (
            "JetBrainsMono-Italic.ttf",
            10.0,
            72,
            6.0,
            13.199999809265137,
        ),
        (
            "JetBrainsMono-Italic.ttf",
            14.0,
            96,
            11.199999809265137,
            24.639999389648438,
        ),
        ("Roboto-Regular.ttf", 10.0, 72, 8.9794921875, 11.71875),
        (
            "Roboto-Regular.ttf",
            14.0,
            96,
            16.76171875,
            21.874998092651367,
        ),
        ("Roboto-Bold.ttf", 10.0, 72, 8.9501953125, 11.71875),
        ("Roboto-Bold.ttf", 14.0, 96, 16.70703125, 21.874998092651367),
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
