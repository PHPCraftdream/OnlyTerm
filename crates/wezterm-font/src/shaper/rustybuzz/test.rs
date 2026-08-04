use super::*;
use crate::locator::{FontDataHandle, FontDataSource};
use crate::FontDatabase;
use config::FontAttributes;

fn hebrew_fallback_handle() -> ParsedFont {
    let db = FontDatabase::with_built_in().unwrap();
    db.resolve(
        &FontAttributes {
            family: "Cascadia Mono".into(),
            stretch: Default::default(),
            weight: Default::default(),
            is_fallback: false,
            is_synthetic: false,
            style: Default::default(),
            freetype_load_flags: None,
            freetype_load_target: None,
            freetype_render_target: None,
            harfbuzz_features: None,
            scale: None,
            assume_emoji_presentation: None,
        },
        14,
    )
    .unwrap()
    .clone()
}

/// Mirrors the real default font stack's shape: a primary font with no
/// Hebrew coverage (Lucida Console isn't available in this Linux/CI
/// build environment, so JetBrains Mono stands in for "primary font
/// without Hebrew glyphs") followed by the bundled Hebrew fallback, so
/// Hebrew codepoints only resolve after at least one no-glyphs/
/// "incomplete" pass through a font that can't shape them.
fn primary_then_hebrew_fallback_handles() -> Vec<ParsedFont> {
    vec![jetbrains_handle(), hebrew_fallback_handle()]
}

/// Same idea, but using the actual default primary font
/// (`default_font_style` on Windows), which -- unlike JetBrains Mono --
/// may have partial native Hebrew coverage (eg: base consonants but not
/// niqqud combining marks), producing a different pattern of
/// direct-vs-"incomplete" glyphs within the same rustybuzz cluster than
/// a font with zero Hebrew coverage at all.
#[cfg(windows)]
fn lucida_then_hebrew_fallback_handles() -> Vec<ParsedFont> {
    let lucida = ParsedFont::from_locator(&FontDataHandle {
        source: FontDataSource::OnDisk(std::path::PathBuf::from(
            "C:\\Windows\\Fonts\\lucon.ttf",
        )),
        index: 0,
        variation: 0,
        origin: crate::locator::FontOrigin::FontDirs,
        coverage: None,
    })
    .expect("C:\\Windows\\Fonts\\lucon.ttf (Lucida Console) must be present on Windows CI");

    fn built_in(family: &str) -> ParsedFont {
        let db = FontDatabase::with_built_in().unwrap();
        db.resolve(
            &FontAttributes {
                family: family.into(),
                stretch: Default::default(),
                weight: Default::default(),
                is_fallback: true,
                is_synthetic: false,
                style: Default::default(),
                freetype_load_flags: None,
                freetype_load_target: None,
                freetype_render_target: None,
                harfbuzz_features: None,
                scale: None,
                assume_emoji_presentation: None,
            },
            14,
        )
        .unwrap()
        .clone()
    }

    // Exact real default order: primary, JetBrains fallback, Noto Color
    // Emoji, Cascadia Mono (Hebrew), Symbols Nerd Font Mono (see
    // `TextStyle::font_with_fallback`).
    vec![
        lucida,
        jetbrains_handle(),
        built_in("Noto Color Emoji"),
        hebrew_fallback_handle(),
        built_in("Symbols Nerd Font Mono"),
    ]
}

/// Regression test for a real bug: mixed Hebrew/Latin/punctuation text
/// on one line (eg: "shalom, world" style output with an embedded
/// dash/quote) rendered with duplicated punctuation and cells drawn in
/// the wrong place. Root cause: `CellCluster::make_cluster_with_bidi`
/// used `ReorderedRun::range` (a `min..max+1` numeric envelope) to walk
/// a run's codepoints, but that envelope isn't guaranteed to contain
/// *only* this run's codepoints when multiple runs are interleaved on
/// the same line -- it can overlap with a neighboring run, visiting
/// (and rendering) the same character twice. Fixed by using
/// `ReorderedRun::indices` (the exact, deduplicated set of codepoints
/// for this run) instead, sorted ascending to recover logical order.
///
/// This asserts the fundamental invariant that must hold no matter how
/// bidi resolution splits a line into clusters: every original cell is
/// covered by exactly one resolved cluster, never zero and never two.
/// Exact reproduction captured from a live warning log: the defensive
/// `clamp_to_char_boundaries` guard fired for real, with
/// `text=",םלועל "` (a `CellCluster::text`, part of a longer Hebrew
/// phrase), reporting "adjusted 0..2 -> 0..3" -- meaning
/// `ClusterResolver` computed a byte range that cut the Hebrew letter
/// 'ם' (a 2-byte UTF-8 character occupying bytes 1..3) in half. This
/// pins down the *exact* input/font-stack combination that triggers
/// the underlying byte-range miscalculation, for use as a base to find
/// the true root cause (this only proves the clamp saves us from a
/// crash, not that the resulting glyphs/positions are actually
/// correct).
#[test]
fn reproduces_the_captured_clamp_warning_input() {
    let _ = env_logger::Builder::new()
        .is_test(true)
        .filter_level(log::LevelFilter::Debug)
        .try_init();

    let config = config::configuration();
    let shaper =
        RustybuzzShaper::new(&config, &primary_then_hebrew_fallback_handles()).unwrap();

    let mut no_glyphs = vec![];
    shaper
        .shape(
            ",םלועל ",
            14.,
            72,
            &mut no_glyphs,
            None,
            Direction::RightToLeft,
            None,
            None,
        )
        .unwrap();
}

#[test]
fn bidi_clusters_do_not_duplicate_or_drop_cells() {
    use termwiz::cell::CellAttributes;
    use termwiz::surface::Line;
    use wezterm_bidi::ParagraphDirectionHint;

    for text in [
        "שלום, עולם! Hello, world",
        "ברוך ה' — Благословен вовеки",
        "На иврите: אמן ואמן, לעולם — Благословен",
        "י ואת נ ודבלמ",
    ] {
        let line = Line::from_text(text, &CellAttributes::default(), 0, None);
        let total_cells = line.len();
        let clusters = line.cluster(Some(ParagraphDirectionHint::AutoLeftToRight));

        // A cluster's cells no longer need to be a contiguous
        // `first_cell_idx..first_cell_idx+width` range now that a
        // Hebrew phrase can be reordered within its cluster (only the
        // *set* of covered cells, via `byte_to_cell_idx`, needs to
        // partition the line exactly). `byte_to_cell_idx` is the
        // authoritative per-byte mapping actually used to position
        // glyphs at render time.
        let mut coverage = vec![0u32; total_cells];
        for cluster in &clusters {
            // Dedup within the cluster first: a niqqud/base pair is
            // two chars sharing one cell, which must count once, not
            // once per char.
            let mut cluster_cells: Vec<usize> = cluster
                .text
                .char_indices()
                .map(|(byte_idx, _)| cluster.byte_to_cell_idx(byte_idx))
                .collect();
            cluster_cells.sort_unstable();
            cluster_cells.dedup();
            for cell_idx in cluster_cells {
                assert!(
                    cell_idx < total_cells,
                    "text={text:?}: cluster {cluster:?} covers out-of-range cell {cell_idx}"
                );
                coverage[cell_idx] += 1;
            }
        }
        for (cell_idx, count) in coverage.iter().enumerate() {
            assert_eq!(
                *count, 1,
                "text={text:?}: cell {cell_idx} covered {count} times (want exactly 1); clusters={clusters:#?}"
            );
        }
    }
}

/// Regression reproduction for a real crash: rendering Hebrew text with
/// niqqud (vowel points, which combine into the same terminal cell as
/// their base letter) through the real `Line` -> `CellCluster` -> shaper
/// pipeline, with bidi enabled (as it now is by default), panicked with
/// "byte index N is not a char boundary" inside `ClusterResolver`
/// (`do_shape`, around the `let substr = &s[sub_range.clone()];` line).
#[test]
fn bidi_multi_word_hebrew_phrase_cluster_order() {
    // Diagnostic: for a multi-word, uniform-attrs Hebrew phrase, how
    // many clusters does `Line::cluster()` produce and in what order?
    // If it stays as ONE cluster, the shaper reorders inter-word RTL
    // layout correctly on its own. If it gets split into several
    // clusters (eg: by the whitespace force-break heuristic), the
    // clusters themselves need to be in VISUAL (reversed) order for
    // RTL, since crossing a cluster boundary means the shaper can't
    // reorder across it.
    use termwiz::cell::CellAttributes;
    use termwiz::surface::Line;
    use wezterm_bidi::ParagraphDirectionHint;

    let text = "שלום עליכם עליכם שלום";
    let line = Line::from_text(text, &CellAttributes::default(), 0, None);
    let clusters = line.cluster(Some(ParagraphDirectionHint::AutoLeftToRight));
    eprintln!("{} cluster(s) for {:?}", clusters.len(), text);
    for c in &clusters {
        eprintln!(
            "  text={:?} width={} first_cell_idx={} direction={:?}",
            c.text, c.width, c.first_cell_idx, c.direction
        );
    }
}

#[test]
fn unresolved_mark_does_not_discard_its_base_letter() {
    // Regression test: hiriq (U+05B4) is not covered by Cascadia Mono,
    // and there's no secondary Hebrew fallback font behind it. Before
    // the fix, a grapheme where the base letter resolved but its
    // combining mark didn't (both share one rustybuzz "cluster" under
    // `MonotoneGraphemes`) was entirely discarded and re-shaped as two
    // separate notdef glyphs once fallback fonts were exhausted --
    // losing the base letter's real glyph and injecting an extra
    // full-width blank cell for the mark. Now the base letter's glyph
    // must survive and the unresolved mark must claim zero cells.
    let _ = env_logger::Builder::new()
        .is_test(true)
        .filter_level(log::LevelFilter::Warn)
        .try_init();

    let text = "\u{5d4}\u{5b4}\u{5d9}\u{5d0}"; // he + hiriq + yod + alef ("הִיא")
    let config = config::configuration();
    let shaper =
        RustybuzzShaper::new(&config, &primary_then_hebrew_fallback_handles()).unwrap();
    let mut no_glyphs = vec![];
    let info = shaper
        .shape(
            text,
            14.,
            72,
            &mut no_glyphs,
            None,
            Direction::RightToLeft,
            None,
            None,
        )
        .unwrap();

    let total_cells: usize = info.iter().map(|i| i.num_cells as usize).sum();
    assert_eq!(
        total_cells, 3,
        "he+yod+alef should claim 3 cells total (hiriq is unresolved and \
         must claim 0), got {total_cells}: {info:#?}"
    );

    let he_resolved = info
        .iter()
        .any(|i| i.font_idx == 1 && i.glyph_pos != 0 && i.num_cells == 1);
    assert!(
        he_resolved,
        "the base letter he (U+05D4) should keep its real, resolved \
         Cascadia Mono glyph even though the hiriq mark attached to \
         the same grapheme has no glyph in that font: {info:#?}"
    );
}

#[test]
fn bidi_multi_word_hebrew_phrase_shapes_with_correct_cell_widths() {
    // Reproduction attempt using the REAL current default font stack
    // (JetBrains Mono primary -- has zero Hebrew coverage -- falling
    // back to the bundled Cascadia Mono) for a full multi-word
    // Hebrew phrase, checking that every shaped glyph's num_cells adds
    // up to exactly the cluster's width (no glyph should claim 0 cells
    // or more cells than are left, which would show up as glued-together
    // or overly-wide gaps on screen).
    let _ = env_logger::Builder::new()
        .is_test(true)
        .filter_level(log::LevelFilter::Warn)
        .try_init();

    use termwiz::cell::CellAttributes;
    use termwiz::surface::Line;
    use wezterm_bidi::ParagraphDirectionHint;

    let text = "שלום עליכם עליכם שלום";
    let line = Line::from_text(text, &CellAttributes::default(), 0, None);
    let clusters = line.cluster(Some(ParagraphDirectionHint::AutoLeftToRight));

    let config = config::configuration();
    let shaper =
        RustybuzzShaper::new(&config, &primary_then_hebrew_fallback_handles()).unwrap();

    for cluster in &clusters {
        let presentation_width = PresentationWidth::with_cluster(cluster);
        let mut no_glyphs = vec![];
        let info = shaper
            .shape(
                &cluster.text,
                14.,
                72,
                &mut no_glyphs,
                Some(cluster.presentation),
                cluster.direction,
                None,
                Some(&presentation_width),
            )
            .unwrap();
        let total_cells: usize = info.iter().map(|i| i.num_cells as usize).sum();
        eprintln!(
            "cluster width={} total_shaped_cells={} no_glyphs={:?}",
            cluster.width, total_cells, no_glyphs
        );
        for i in &info {
            eprintln!(
                "  glyph_pos={} num_cells={} x_advance={:.2} cluster={} only_char={:?}",
                i.glyph_pos,
                i.num_cells,
                i.x_advance.get(),
                i.cluster,
                i.only_char
            );
        }
        assert_eq!(
            total_cells, cluster.width,
            "shaped glyphs' num_cells sum ({total_cells}) doesn't match cluster width ({}) for {:?}",
            cluster.width, cluster.text
        );
    }
}

#[test]
fn bidi_cluster_widths_per_char_attrs() {
    // Diagnostic (not a hard assertion yet): does giving each Hebrew
    // character DIFFERENT cell attributes -- as a chatty/streaming CLI
    // like Claude Code plausibly does per-token/per-color-span -- cause
    // `Line::cluster()` to split what should be one contiguous Hebrew
    // word into several tiny independent bidi "paragraphs", each
    // auto-detecting its own direction independently and losing the
    // surrounding context? This inspects cluster count/width/
    // first_cell_idx directly, without going through the shaper at all.
    use termwiz::cell::{Cell, CellAttributes};
    use termwiz::surface::Line;
    use wezterm_bidi::ParagraphDirectionHint;

    let text = "שלום";

    let uniform = Line::from_text(text, &CellAttributes::default(), 0, None);
    let uniform_clusters = uniform.cluster(Some(ParagraphDirectionHint::AutoLeftToRight));
    eprintln!("uniform attrs: {} cluster(s)", uniform_clusters.len());
    for c in &uniform_clusters {
        eprintln!(
            "  text={:?} width={} first_cell_idx={} direction={:?}",
            c.text, c.width, c.first_cell_idx, c.direction
        );
    }

    let mut varied = Line::new(0);
    for (idx, c) in text.chars().enumerate() {
        let mut attrs = CellAttributes::default();
        // Alternate foreground color per character, mimicking
        // per-character/per-token styling.
        attrs.set_foreground(termwiz::color::ColorAttribute::PaletteIndex(
            (idx % 2) as u8,
        ));
        varied.set_cell(idx, Cell::new(c, attrs), 0);
    }
    let varied_clusters = varied.cluster(Some(ParagraphDirectionHint::AutoLeftToRight));
    eprintln!("varied attrs: {} cluster(s)", varied_clusters.len());
    for c in &varied_clusters {
        eprintln!(
            "  text={:?} width={} first_cell_idx={} direction={:?}",
            c.text, c.width, c.first_cell_idx, c.direction
        );
    }
}

#[test]
fn hebrew_phrase_reverses_in_place_without_touching_neighbors() {
    // Regression test for the simplified (non-UAX#9) rendering
    // model: a terminal ties cursor movement, selection and shell
    // line-editing to each character's typed/logical column, so
    // instead of running the full Bidi Algorithm (which
    // right-justifies RTL-based paragraphs and can sweep a stray
    // dash or number into the wrong end of the line), only the
    // Hebrew letters themselves get reversed relative to each other,
    // exactly where they were typed. Brackets/digits/Latin text
    // never move and are never mirrored, since they never change
    // position relative to the rest of the line.
    use termwiz::cell::CellAttributes;
    use termwiz::surface::Line;
    use wezterm_bidi::ParagraphDirectionHint;

    for (text, want) in [
        ("(שלום)", "(םולש)"),
        ("שלום עולם", "םלוע םולש"),
        // The geresh stays bonded to its letter (moves with it) but
        // the pair itself still reverses along with the rest of the
        // phrase, same as any other letter -- reading the resulting
        // "'א קרפ" span right-to-left recovers "פרק א'" exactly.
        ("פרק א' — Chapter", "'א קרפ — Chapter"),
    ] {
        let line = Line::from_text(text, &CellAttributes::default(), 0, None);
        let clusters = line.cluster(Some(ParagraphDirectionHint::AutoLeftToRight));
        let joined: String = clusters.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(joined, want, "input {text:?}");
    }
}

#[test]
fn punctuation_inside_hebrew_phrase_moves_with_the_phrase() {
    // A comma/question mark *between* two Hebrew words punctuates the
    // Hebrew, so it has to travel with it when the phrase is reversed
    // (this is Unicode rule UAX #9 N1: a neutral run surrounded by
    // right-to-left text becomes right-to-left too). Quotes/brackets
    // wrapping the whole phrase have non-Hebrew on their far side, so
    // they are *not* part of the phrase and must stay put -- which is
    // what keeps the line growing left-to-right from column 0 with
    // the Hebrew half still ahead of its Russian translation.
    //
    // Each case is written as (before, phrase, after) and the
    // expectation is built as `before + reverse(phrase) + after`:
    // reversing is by definition what "reads right-to-left" means, so
    // this states the intent without restating the algorithm.
    use termwiz::cell::CellAttributes;
    use termwiz::surface::Line;
    use wezterm_bidi::ParagraphDirectionHint;

    for (before, phrase, after) in [
        // The reported case: quoted Hebrew, then its quoted Russian
        // translation. The comma is inside the phrase and moves; the
        // quotes and the ` / ` separator do not.
        (
            "\"",
            "אם אין אני לי, מי לי",
            "\" / \"Если не я за себя, то кто за меня\"",
        ),
        ("«", "כל ישראל ערבים זה בזה", "» / «Весь Израиль в ответе»"),
        ("(", "איזהו עשיר", ") (кто богат?)"),
        // A closing ASCII apostrophe is a quote, not a geresh: it
        // must stay outside the phrase it closes rather than being
        // dragged to the far side of it.
        (
            "'",
            "דע לפני מי אתה עומד",
            "' / 'знай, перед кем ты стоишь'",
        ),
    ] {
        let text = format!("{before}{phrase}{after}");
        let want = format!(
            "{before}{}{after}",
            phrase.chars().rev().collect::<String>()
        );
        let line = Line::from_text(&text, &CellAttributes::default(), 0, None);
        let clusters = line.cluster(Some(ParagraphDirectionHint::AutoLeftToRight));
        let joined: String = clusters.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(joined, want, "input {text:?}");
    }
}

#[test]
fn multiword_hebrew_phrase_reverses_as_one_block() {
    // Regression test for a reported bug: within a multi-word Hebrew
    // phrase, letters inside each word read right-to-left correctly,
    // but the words themselves stayed in typed (left-to-right) order.
    // Consecutive Hebrew words glued by spaces/punctuation must
    // reverse together as a single block, exactly like a single word.
    use termwiz::cell::CellAttributes;
    use termwiz::surface::Line;
    use wezterm_bidi::ParagraphDirectionHint;

    let text = "равные, без !חמש באב ו״ט";
    let line = Line::from_text(text, &CellAttributes::default(), 0, None);
    let hint = Some(ParagraphDirectionHint::AutoLeftToRight);
    let joined: String = line.cluster(hint).iter().map(|c| c.text.as_str()).collect();
    assert_eq!(joined, "равные, без !ט״ו באב שמח");
}

#[test]
fn multiword_hebrew_phrase_split_by_wrap_still_reverses_each_row() {
    // The reported bug above turned out to be exactly this: the wrap
    // happened to split the multi-word phrase between two of its
    // words, and the old "leave an edge-touching run unreversed"
    // wrap-boundary precaution then left BOTH rows completely
    // untouched (typed order), not just the seam between them. Each
    // row must still reverse its own Hebrew content regardless of
    // wrap topology.
    use termwiz::cell::CellAttributes;
    use termwiz::surface::Line;
    use wezterm_bidi::ParagraphDirectionHint;

    let hint = Some(ParagraphDirectionHint::AutoLeftToRight);

    let mut row1 = Line::from_text("равные, без !חמש", &CellAttributes::default(), 0, None);
    row1.set_last_cell_was_wrapped(true, 1);
    let row1_out: String = row1
        .cluster_with_wrap_context(hint, false)
        .iter()
        .map(|c| c.text.as_str())
        .collect();
    assert_eq!(row1_out, "равные, без !שמח");

    let row2 = Line::from_text("באב ו״ט", &CellAttributes::default(), 0, None);
    let row2_out: String = row2
        .cluster_with_wrap_context(hint, true)
        .iter()
        .map(|c| c.text.as_str())
        .collect();
    assert_eq!(row2_out, "ט״ו באב");
}

#[test]
fn hebrew_phrase_touching_wrap_boundary_still_reverses() {
    // Regression test: a physical row only ever sees its own cells,
    // so a Hebrew phrase touching the first/last cell might actually
    // be a fragment of a longer phrase continuing on the row before/
    // after it (the line wrapped there). `cluster_with_wrap_context`
    // used to leave such an edge-touching phrase completely
    // unreversed as a precaution -- but that left a multi-word
    // phrase wrapped between two of its words with BOTH halves in
    // typed (wrong) order, not just at the seam. Standard bidi text
    // layout reverses each visual line independently regardless of
    // wrap topology, which is also the closest a terminal's fixed
    // per-cell grid can get (it can't move a word across a row
    // boundary either way) -- so wrap context must NOT change how
    // this phrase reverses.
    use termwiz::cell::CellAttributes;
    use termwiz::surface::Line;
    use wezterm_bidi::ParagraphDirectionHint;

    let text = "שלום עולם";
    let line = Line::from_text(text, &CellAttributes::default(), 0, None);
    let hint = Some(ParagraphDirectionHint::AutoLeftToRight);

    let normal: String = line
        .cluster(hint)
        .iter()
        .map(|c| c.text.as_str())
        .collect::<Vec<_>>()
        .join("");
    assert_eq!(normal, "םלוע םולש");

    // This row is the tail of a wrapped phrase (its first cell might
    // continue a run from the row above) -- it must still reverse
    // exactly the same as the no-wrap-context baseline above.
    let as_continuation: String = line
        .cluster_with_wrap_context(hint, true)
        .iter()
        .map(|c| c.text.as_str())
        .collect::<Vec<_>>()
        .join("");
    assert_eq!(as_continuation, normal);
}

#[test]
fn diag_quoted_hebrew_then_russian_char_by_char() {
    // Diagnostic: build the same line two ways -- via `Line::from_text`
    // (grapheme-aware, used by `render_line`/most tests) and via
    // per-character `set_cell` (mimicking how the real terminal builds
    // a line one printed character at a time from PTY bytes) -- and
    // compare the resulting cluster order, to check whether the two
    // construction paths actually produce the same `CellCluster`s for
    // a line reported to render differently in the two contexts.
    use termwiz::cell::{Cell, CellAttributes};
    use termwiz::surface::Line;
    use wezterm_bidi::ParagraphDirectionHint;

    let text = "\"אם אין אני לי, מי לי\" / \"Если не я за себя, то кто за меня\"";
    let hint = Some(ParagraphDirectionHint::AutoLeftToRight);

    let from_text = Line::from_text(text, &CellAttributes::default(), 0, None);
    let joined_from_text: String = from_text
        .cluster(hint)
        .iter()
        .map(|c| c.text.as_str())
        .collect::<Vec<_>>()
        .join("");

    let mut char_by_char = Line::new(0);
    for (idx, c) in text.chars().enumerate() {
        char_by_char.set_cell(idx, Cell::new(c, CellAttributes::default()), 0);
    }
    let joined_char_by_char: String = char_by_char
        .cluster(hint)
        .iter()
        .map(|c| c.text.as_str())
        .collect::<Vec<_>>()
        .join("");

    eprintln!("from_text:     {joined_from_text:?}");
    eprintln!("char_by_char:  {joined_char_by_char:?}");
    assert_eq!(joined_from_text, joined_char_by_char);
}

#[test]
fn diag_mixed_lang_quote_boundary() {
    // Diagnostic: a Russian translation wrapped in guillemets, an em
    // dash, and the Hebrew original -- with the Russian+punctuation
    // portion given one set of attrs and the Hebrew portion another
    // (mimicking a chatty CLI's per-language color styling), the way
    // a real user reported broken quote mirroring/positioning.
    use termwiz::cell::{Cell, CellAttributes};
    use termwiz::surface::Line;
    use wezterm_bidi::ParagraphDirectionHint;

    let ru = "«Если не я за себя, то кто?» — ";
    let he = "אם אין אני לי";
    let mut line = Line::new(0);
    let mut idx = 0;
    let mut ru_attrs = CellAttributes::default();
    ru_attrs.set_foreground(termwiz::color::ColorAttribute::PaletteIndex(1));
    for c in ru.chars() {
        line.set_cell(idx, Cell::new(c, ru_attrs.clone()), 0);
        idx += 1;
    }
    let mut he_attrs = CellAttributes::default();
    he_attrs.set_foreground(termwiz::color::ColorAttribute::PaletteIndex(2));
    for c in he.chars() {
        line.set_cell(idx, Cell::new(c, he_attrs.clone()), 0);
        idx += 1;
    }

    let clusters = line.cluster(Some(ParagraphDirectionHint::AutoLeftToRight));
    eprintln!("{} cluster(s):", clusters.len());
    for c in &clusters {
        eprintln!(
            "  text={:?} width={} first_cell_idx={} direction={:?}",
            c.text, c.width, c.first_cell_idx, c.direction
        );
    }
}

#[test]
fn shapes_hebrew_text_with_niqqud_under_bidi() {
    let _ = env_logger::Builder::new()
        .is_test(true)
        .filter_level(log::LevelFilter::Debug)
        .try_init();

    use termwiz::cell::CellAttributes;
    use termwiz::surface::Line;
    use wezterm_bidi::ParagraphDirectionHint;

    // "shalom" with niqqud: each vowel point combines into the same
    // grapheme cluster (and thus the same terminal cell) as the
    // preceding consonant.
    let combined = Line::from_text("שָׁלוֹם", &CellAttributes::default(), 0, None);

    // Same text, but with every niqqud mark placed in its OWN cell
    // instead of being grouped into the preceding consonant's grapheme
    // cluster -- simulating what happens if the base letter and its
    // combining mark get printed via separate `print()`/flush cycles
    // (eg: an SGR/color escape between them, as a chatty program like
    // Claude Code emits per-character/per-word highlighting) instead of
    // arriving as one already-composed string handed to
    // `Line::from_text`.
    let mut split = Line::new(0);
    for (idx, c) in "שָׁלוֹם".chars().enumerate() {
        split.set_cell(
            idx,
            termwiz::cell::Cell::new(c, CellAttributes::default()),
            0,
        );
    }

    // Neither JetBrains Mono nor Lucida Console has ANY Hebrew coverage
    // (confirmed separately), so a Latin prefix ahead of the Hebrew word
    // forces the Hebrew span to resolve via recursive fallback
    // (`do_shape(font_idx + 1, ...)`) starting at a NON-ZERO byte offset
    // -- exercising the "incomplete cluster" recursion path with
    // `range.start != 0`, which combined/split (pure Hebrew, always
    // starting at byte 0) never did.
    let prefixed = Line::from_text("echo שָׁלוֹם", &CellAttributes::default(), 0, None);

    for (label, line) in [
        ("combined", &combined),
        ("split", &split),
        ("prefixed", &prefixed),
    ] {
        let clusters = line.cluster(Some(ParagraphDirectionHint::AutoLeftToRight));

        let config = config::configuration();
        let shaper =
            RustybuzzShaper::new(&config, &primary_then_hebrew_fallback_handles()).unwrap();

        for cluster in &clusters {
            let presentation_width = PresentationWidth::with_cluster(cluster);
            let mut no_glyphs = vec![];
            shaper
                .shape(
                    &cluster.text,
                    14.,
                    72,
                    &mut no_glyphs,
                    Some(cluster.presentation),
                    cluster.direction,
                    None,
                    Some(&presentation_width),
                )
                .unwrap_or_else(|e| panic!("label={label:?} cluster={cluster:?}: {e:?}"));
        }

        #[cfg(windows)]
        {
            let shaper =
                RustybuzzShaper::new(&config, &lucida_then_hebrew_fallback_handles()).unwrap();
            for cluster in &clusters {
                let presentation_width = PresentationWidth::with_cluster(cluster);
                let mut no_glyphs = vec![];
                shaper
                    .shape(
                        &cluster.text,
                        14.,
                        72,
                        &mut no_glyphs,
                        Some(cluster.presentation),
                        cluster.direction,
                        None,
                        Some(&presentation_width),
                    )
                    .unwrap_or_else(|e| {
                        panic!("[lucida] label={label:?} cluster={cluster:?}: {e:?}")
                    });
            }
        }
    }
}

fn jetbrains_handle() -> ParsedFont {
    let db = FontDatabase::with_built_in().unwrap();
    db.resolve(
        &FontAttributes {
            family: "JetBrains Mono".into(),
            stretch: Default::default(),
            weight: Default::default(),
            is_fallback: false,
            is_synthetic: false,
            style: Default::default(),
            freetype_load_flags: None,
            freetype_load_target: None,
            freetype_render_target: None,
            harfbuzz_features: None,
            scale: None,
            assume_emoji_presentation: None,
        },
        14,
    )
    .unwrap()
    .clone()
}

/// One shaped glyph's regression-relevant fields, used by
/// `assert_shape_matches_baseline` below.
#[derive(Debug, Clone, Copy, PartialEq)]
struct GlyphBaseline {
    glyph_pos: u32,
    cluster: u32,
    x_advance: f64,
}

/// Shapes `text` with `RustybuzzShaper` (size=10, dpi=72, JetBrains
/// Mono) and asserts the result matches a hardcoded baseline exactly
/// for `glyph_pos`/`cluster`, and within `eps` pixels for `x_advance`
/// (not bit-exact against the baseline capture, to tolerate
/// float-rounding jitter across platforms/toolchains rather than
/// requiring the environment that captured the baseline).
///
/// This replaces a former harfbuzz-vs-rustybuzz parity comparison
/// (see the module doc comment on the H0-established guarantee): now
/// that the `harfbuzz` crate/`HarfbuzzShaper` have been removed
/// (phase H4), there is no live oracle to compare against, so this
/// instead pins down the current `RustybuzzShaper` output as a
/// regression baseline (captured by actually running the shaper, not
/// guessed) -- it will still catch a shaping regression from a
/// rustybuzz/ttf-parser upgrade or a refactor of `do_shape`, just not
/// a *divergence from harfbuzz* (which H0/H1 already established was
/// zero for glyph_id/cluster, and small/tolerance-bounded for
/// x_advance, before this crate was removed).
fn assert_shape_matches_baseline(text: &str, eps: f64, expected: &[GlyphBaseline]) {
    let config = config::configuration();
    let handle = jetbrains_handle();
    let rb_shaper = RustybuzzShaper::new(&config, &[handle]).unwrap();

    let mut no_glyphs = vec![];
    let info = rb_shaper
        .shape(
            text,
            10.,
            72,
            &mut no_glyphs,
            None,
            Direction::LeftToRight,
            None,
            None,
        )
        .unwrap();
    assert!(no_glyphs.is_empty(), "{:?}", no_glyphs);

    assert_eq!(
        expected.len(),
        info.len(),
        "glyph count mismatch for {text:?}: expected={expected:#?} actual={info:#?}"
    );

    for (want, got) in expected.iter().zip(info.iter()) {
        assert_eq!(
            want.glyph_pos, got.glyph_pos,
            "glyph_id mismatch for {text:?}: want={want:?} got={got:?}"
        );
        assert_eq!(
            want.cluster, got.cluster,
            "cluster mismatch for {text:?}: want={want:?} got={got:?}"
        );
        assert!(
            (want.x_advance - got.x_advance.get()).abs() <= eps,
            "x_advance mismatch beyond eps={eps} for {text:?}: want={want:?} got={got:?}"
        );
    }
}

#[test]
fn parity_simple_latin() {
    let _ = env_logger::Builder::new()
        .is_test(true)
        .filter_level(log::LevelFilter::Trace)
        .try_init();
    // Baselines captured from a real `RustybuzzShaper::shape` run
    // against JetBrainsMono-Regular.ttf at size=10, dpi=72 (see
    // `assert_shape_matches_baseline`'s doc comment for why these are
    // hardcoded rather than compared live against harfbuzz).
    assert_shape_matches_baseline(
        "abc",
        1.0,
        &[
            GlyphBaseline {
                glyph_pos: 189,
                cluster: 0,
                x_advance: 6.0,
            },
            GlyphBaseline {
                glyph_pos: 214,
                cluster: 1,
                x_advance: 6.0,
            },
            GlyphBaseline {
                glyph_pos: 215,
                cluster: 2,
                x_advance: 6.0,
            },
        ],
    );
    assert_shape_matches_baseline(
        "x x",
        1.0,
        &[
            GlyphBaseline {
                glyph_pos: 367,
                cluster: 0,
                x_advance: 6.0,
            },
            GlyphBaseline {
                glyph_pos: 958,
                cluster: 1,
                x_advance: 6.0,
            },
            GlyphBaseline {
                glyph_pos: 367,
                cluster: 2,
                x_advance: 6.0,
            },
        ],
    );
    assert_shape_matches_baseline(
        "x\u{3000}x",
        1.0,
        &[
            GlyphBaseline {
                glyph_pos: 367,
                cluster: 0,
                x_advance: 6.0,
            },
            GlyphBaseline {
                glyph_pos: 958,
                cluster: 1,
                x_advance: 10.0,
            },
            GlyphBaseline {
                glyph_pos: 367,
                cluster: 4,
                x_advance: 6.0,
            },
        ],
    );
}

#[test]
fn parity_ligatures() {
    let _ = env_logger::Builder::new()
        .is_test(true)
        .filter_level(log::LevelFilter::Trace)
        .try_init();
    // JetBrains Mono applies contextual (`calt`) substitution to
    // `<-`/`<--` (each character gets a different glyph id than its
    // standalone form, e.g. `<`'s glyph_pos changes from 1052 to
    // 1742 once followed by `-`), exercising the same
    // feature-driven substitution path a former `HarfbuzzShaper`
    // comparison test covered (see `assert_shape_matches_baseline`'s
    // doc comment) -- note this does not collapse into a single
    // merged glyph per sequence at this size/config (each character
    // keeps its own glyph and cluster), so the baselines below have
    // one entry per input character, not one per ligated sequence.
    // Baselines captured from a real shaper run, same as
    // `parity_simple_latin`.
    assert_shape_matches_baseline(
        "<",
        1.0,
        &[GlyphBaseline {
            glyph_pos: 1052,
            cluster: 0,
            x_advance: 6.0,
        }],
    );
    assert_shape_matches_baseline(
        "<-",
        1.0,
        &[
            GlyphBaseline {
                glyph_pos: 1742,
                cluster: 0,
                x_advance: 6.0,
            },
            GlyphBaseline {
                glyph_pos: 1588,
                cluster: 1,
                x_advance: 6.0,
            },
        ],
    );
    assert_shape_matches_baseline(
        "<--",
        1.0,
        &[
            GlyphBaseline {
                glyph_pos: 1742,
                cluster: 0,
                x_advance: 6.0,
            },
            GlyphBaseline {
                glyph_pos: 1742,
                cluster: 1,
                x_advance: 6.0,
            },
            GlyphBaseline {
                glyph_pos: 1589,
                cluster: 2,
                x_advance: 6.0,
            },
        ],
    );
}

#[test]
fn shape_basic() {
    let _ = env_logger::Builder::new()
        .is_test(true)
        .filter_level(log::LevelFilter::Trace)
        .try_init();

    let config = config::configuration();
    let shaper = RustybuzzShaper::new(&config, &[jetbrains_handle()]).unwrap();
    let mut no_glyphs = vec![];
    let info = shaper
        .shape(
            "abc",
            10.,
            72,
            &mut no_glyphs,
            None,
            Direction::LeftToRight,
            None,
            None,
        )
        .unwrap();
    assert!(no_glyphs.is_empty(), "{:?}", no_glyphs);
    assert_eq!(info.len(), 3);
    assert_eq!(info[0].only_char, Some('a'));
    assert_eq!(info[1].only_char, Some('b'));
    assert_eq!(info[2].only_char, Some('c'));
    assert_eq!(info[0].cluster, 0);
    assert_eq!(info[1].cluster, 1);
    assert_eq!(info[2].cluster, 2);
}

/// Regression coverage for
/// <https://github.com/wezterm/wezterm/issues/7963>: a fallback font
/// candidate whose backing file cannot be opened (originally reported
/// as a Windows Store / MSIX font living under an ACL-protected
/// `C:\Program Files\WindowsApps\...` path, denying access with "Access
/// is denied. (os error 5)") must not abort shaping for the whole text
/// run. The old `HarfbuzzShaper::load_fallback` (removed along with the
/// rest of the harfbuzz shaper in the freetype/harfbuzz -> rustybuzz/
/// swash migration) panicked in this situation; that panic could
/// escalate to a fatal crash (STATUS_FATAL_APP_EXIT) if a caught panic
/// unwind triggered a second panic, e.g. from a CLI spinner animation
/// re-triggering fallback resolution on every tick.
///
/// We don't attempt to reproduce real Windows ACL denial here (fragile
/// and platform-specific); instead we point a fallback candidate's
/// `FontDataSource::OnDisk` at a path that does not exist at all. From
/// `RustybuzzShaper::load_fallback`'s point of view this produces the
/// same shape of failure as an ACL-Denied open: `std::fs::read` (inside
/// `FontDataSource::load_data`, called by
/// `SwashFontInfo::from_locator`) returns an `Err`, and any IO error
/// there must be handled identically regardless of its underlying
/// `io::ErrorKind` (`NotFound`, `PermissionDenied`, etc.) -- the
/// resolver has no business special-casing one IO error kind over
/// another; all of them mean "this candidate is unusable, move on".
///
/// The fallback list here has the broken candidate at index 0 and a
/// real, working font (JetBrains Mono) at index 1. If the resolver
/// still worked the old (buggy) way -- propagating the open/parse
/// error out of `do_shape` via `?` -- this test would fail with
/// `shape(..).unwrap()` panicking on the propagated `Err`. With the
/// fix, `shape` logs a warning for the broken candidate and moves on
/// to shape successfully against font_idx=1.
#[test]
fn fallback_skips_unreadable_candidate() {
    let _ = env_logger::Builder::new()
        .is_test(true)
        .filter_level(log::LevelFilter::Trace)
        .try_init();

    let config = config::configuration();

    let unreadable_handle = ParsedFont::from_locator(&FontDataHandle {
        source: FontDataSource::OnDisk(std::path::PathBuf::from(
            "/this/path/does/not/exist/wezterm-issue-7963-fallback-test.ttf",
        )),
        index: 0,
        variation: 0,
        origin: crate::locator::FontOrigin::FontDirs,
        coverage: None,
    });
    // `ParsedFont::from_locator` itself may already fail to build a
    // `ParsedFont` for a nonexistent path (it needs to peek at the file
    // to extract names/metrics) -- either way we want a `ParsedFont`
    // value to put in the handles list, because the real-world bug is
    // about a *resolved* fallback candidate (one that made it into the
    // handles list, e.g. because font enumeration read it from a
    // directory listing without opening it) whose file later can't be
    // opened when the shaper actually tries to load it. So if
    // constructing it from a bogus path fails up front, fall back to
    // building one from the real JetBrains Mono font and then
    // rewriting its `handle.source` to the bogus path -- this forges
    // exactly the "resolved candidate, unreadable file" scenario
    // `load_fallback` must tolerate.
    let mut broken = unreadable_handle.unwrap_or_else(|_| jetbrains_handle());
    broken.handle.source = FontDataSource::OnDisk(std::path::PathBuf::from(
        "/this/path/does/not/exist/wezterm-issue-7963-fallback-test.ttf",
    ));

    let working = jetbrains_handle();

    let shaper = RustybuzzShaper::new(&config, &[broken, working]).unwrap();

    let mut no_glyphs = vec![];
    let info = shaper
        .shape(
            "abc",
            10.,
            72,
            &mut no_glyphs,
            None,
            Direction::LeftToRight,
            None,
            None,
        )
        .expect(
            "shape() must gracefully skip an unreadable fallback candidate \
             instead of propagating its IO/parse error (see #7963)",
        );

    assert!(no_glyphs.is_empty(), "{:?}", no_glyphs);
    assert_eq!(info.len(), 3);
    assert_eq!(info[0].only_char, Some('a'));
    assert_eq!(info[1].only_char, Some('b'));
    assert_eq!(info[2].only_char, Some('c'));
    for glyph in &info {
        assert_eq!(
            glyph.font_idx, 1,
            "expected glyphs to be shaped from the working fallback \
             candidate (font_idx=1), not the unreadable one: {:?}",
            info
        );
    }
}
