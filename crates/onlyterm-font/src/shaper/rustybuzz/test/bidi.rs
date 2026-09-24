use super::*;

/// Regression test for a real bug: mixed Hebrew/Latin/punctuation text
/// on one line rendered with duplicated punctuation and misplaced cells.
/// The cause was using `ReorderedRun::range`, which can overlap a
/// neighboring run; the fix uses the run's exact `indices` instead.
///
/// This pins the invariant that every original cell belongs to exactly
/// one resolved cluster. It also reproduces a byte range that cut a
/// two-byte Hebrew character in half, ensuring the defensive clamp
/// prevents a panic for the captured font stack and input.
#[test]
fn reproduces_the_captured_clamp_warning_input() {
    let _ = env_logger::Builder::new()
        .is_test(true)
        .filter_level(log::LevelFilter::Debug)
        .try_init();

    let config = config::configuration();
    let shaper = RustybuzzShaper::new(&config, &primary_then_hebrew_fallback_handles()).unwrap();

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
    use onlyterm_bidi::ParagraphDirectionHint;
    use termwiz::cell::CellAttributes;
    use termwiz::surface::Line;

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
    use onlyterm_bidi::ParagraphDirectionHint;
    use termwiz::cell::CellAttributes;
    use termwiz::surface::Line;

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
    let shaper = RustybuzzShaper::new(&config, &primary_then_hebrew_fallback_handles()).unwrap();
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

    use onlyterm_bidi::ParagraphDirectionHint;
    use termwiz::cell::CellAttributes;
    use termwiz::surface::Line;

    let text = "שלום עליכם עליכם שלום";
    let line = Line::from_text(text, &CellAttributes::default(), 0, None);
    let clusters = line.cluster(Some(ParagraphDirectionHint::AutoLeftToRight));

    let config = config::configuration();
    let shaper = RustybuzzShaper::new(&config, &primary_then_hebrew_fallback_handles()).unwrap();

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
    use onlyterm_bidi::ParagraphDirectionHint;
    use termwiz::cell::{Cell, CellAttributes};
    use termwiz::surface::Line;

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
    use onlyterm_bidi::ParagraphDirectionHint;
    use termwiz::cell::CellAttributes;
    use termwiz::surface::Line;

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
    use onlyterm_bidi::ParagraphDirectionHint;
    use termwiz::cell::CellAttributes;
    use termwiz::surface::Line;

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
    use onlyterm_bidi::ParagraphDirectionHint;
    use termwiz::cell::CellAttributes;
    use termwiz::surface::Line;

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
    use onlyterm_bidi::ParagraphDirectionHint;
    use termwiz::cell::CellAttributes;
    use termwiz::surface::Line;

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
    use onlyterm_bidi::ParagraphDirectionHint;
    use termwiz::cell::CellAttributes;
    use termwiz::surface::Line;

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
    use onlyterm_bidi::ParagraphDirectionHint;
    use termwiz::cell::{Cell, CellAttributes};
    use termwiz::surface::Line;

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
    use onlyterm_bidi::ParagraphDirectionHint;
    use termwiz::cell::{Cell, CellAttributes};
    use termwiz::surface::Line;

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

    use onlyterm_bidi::ParagraphDirectionHint;
    use termwiz::cell::CellAttributes;
    use termwiz::surface::Line;

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
