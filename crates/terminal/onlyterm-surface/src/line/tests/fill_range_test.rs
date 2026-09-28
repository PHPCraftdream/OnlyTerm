#![cfg(test)]
//! Differential test for the bulk `Line::fill_range` rewrite: compares the
//! production implementation against `reference_fill_range`, a direct
//! transliteration of the pre-optimization per-cell loop, built only from
//! `Line`'s public API. Exercises random content (ASCII, CJK wide chars,
//! combining marks, several attributes/backgrounds, explicit hyperlinks,
//! a wrapped last cell) across both V and C storage, with ranges that are
//! empty, interior, reaching/extending past the end, and straddling wide
//! characters on either edge.
//!
//! This file lives as a child of `crate::line::line` (registered from
//! `line/line/mod.rs`, like `cow_test.rs`) so it can read `Line`'s private
//! `bits` field directly, the same way `cow_test.rs` reads `cells`/`zones`.

use super::*;
use crate::hyperlink::Hyperlink;
use alloc::string::{String, ToString};
use core::ops::Range;
use k9::assert_equal as assert_eq;
use onlyterm_cell::color::AnsiColor;
use onlyterm_cell::Intensity;

/// Small deterministic PRNG so failures are reproducible without pulling in
/// an external `rand` dependency.
struct Lcg(u64);

impl Lcg {
    fn new(seed: u64) -> Self {
        // Avoid an all-zero state, which would make the LCG degenerate.
        Lcg(seed ^ 0x9E37_79B9_7F4A_7C15)
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }

    fn below(&mut self, bound: usize) -> usize {
        if bound == 0 {
            0
        } else {
            (self.next_u64() % bound as u64) as usize
        }
    }

    fn chance_one_in(&mut self, n: usize) -> bool {
        self.below(n) == 0
    }
}

type Piece = (&'static str, usize, CellAttributes);

fn attrs_palette(hyperlink: &Arc<Hyperlink>) -> Vec<CellAttributes> {
    vec![
        CellAttributes::default(),
        CellAttributes::default()
            .set_background(AnsiColor::Red)
            .clone(),
        CellAttributes::default()
            .set_intensity(Intensity::Bold)
            .clone(),
        CellAttributes::default()
            .set_hyperlink(Some(Arc::clone(hyperlink)))
            .clone(),
    ]
}

fn random_pieces(rng: &mut Lcg, attrs_pool: &[CellAttributes], max_cols: usize) -> Vec<Piece> {
    const GRAPHEMES: &[(&str, usize)] = &[
        ("a", 1),
        ("b", 1),
        (" ", 1),
        ("日", 2),
        ("本", 2),
        // Combining marks: forced to width 1 explicitly below, independent
        // of any width-detection logic (that's not what this test covers).
        ("e\u{0301}", 1),
        ("n\u{0303}", 1),
    ];
    let mut pieces = Vec::new();
    let mut cols = 0usize;
    while cols < max_cols {
        let (text, width) = GRAPHEMES[rng.below(GRAPHEMES.len())];
        if cols + width > max_cols {
            break;
        }
        // Wide graphemes are deliberately never given the plain default
        // attrs entry (index 0): a wide grapheme with exactly default
        // attrs exposes a *pre-existing, unrelated* bug in Vec storage's
        // `prune_trailing_blanks` (its `rposition` scan treats the wide
        // character's own continuation cell -- text " ", attrs default --
        // as a prunable blank, and can truncate right after the wide
        // character's leading cell, orphaning the continuation and
        // leaving `len()` inconsistent with the visible cell's width).
        // That bug is in `VecStorage`/`Line::prune_trailing_blanks`, not
        // in `fill_range`, so it's out of scope here; avoid tripping it.
        let attrs = if width > 1 {
            attrs_pool[1 + rng.below(attrs_pool.len() - 1)].clone()
        } else {
            attrs_pool[rng.below(attrs_pool.len())].clone()
        };
        pieces.push((text, width, attrs));
        cols += width;
    }
    pieces
}

/// Builds Vec-backed storage directly from a recipe, mirroring
/// `Line::from_text`'s own wide-char continuation-cell convention.
fn build_v_line(pieces: &[Piece], seqno: SequenceNo) -> Line {
    let mut cells = Vec::new();
    for (text, width, attrs) in pieces {
        cells.push(Cell::new_grapheme_with_width(text, *width, attrs.clone()));
        for _ in 1..*width {
            cells.push(Cell::new(' ', attrs.clone()));
        }
    }
    Line::from_cells(cells, seqno)
}

/// Builds clustered storage via `Line::new` + incremental grapheme
/// appends -- an independent construction path from `compress_for_scrollback`.
fn build_c_line_via_append(pieces: &[Piece], seqno: SequenceNo) -> Line {
    let mut line = Line::new(seqno);
    for (text, width, attrs) in pieces {
        let idx = line.len();
        line.set_cell_grapheme(idx, text, *width, attrs.clone(), seqno);
    }
    line
}

/// Reference implementation: a direct transliteration of `fill_range`'s
/// pre-optimization body, built only from `Line`'s public API.
fn reference_fill_range(line: &mut Line, cols: Range<usize>, cell: &Cell, seqno: SequenceNo) {
    if line.is_empty() && *cell == Cell::blank() {
        return;
    }
    for x in cols {
        line.set_cell_clearing_image_placements(x, cell.clone(), seqno);
    }
    line.prune_trailing_blanks(seqno);
}

#[derive(Debug, PartialEq)]
struct Observed {
    text: String,
    cells: Vec<(usize, usize, CellAttributes)>,
    len: usize,
    last_wrapped: bool,
    has_hyperlink: bool,
    seqno: SequenceNo,
    bits: u16,
}

fn observe(line: &Line) -> Observed {
    Observed {
        text: line.as_str().to_string(),
        cells: line
            .visible_cells()
            .map(|c| (c.cell_index(), c.width(), c.attrs().clone()))
            .collect(),
        len: line.len(),
        last_wrapped: line.last_cell_was_wrapped(),
        has_hyperlink: line.has_hyperlink(),
        seqno: line.current_seqno(),
        bits: line.bits.bits(),
    }
}

fn fill_cells_palette() -> Vec<Cell> {
    vec![
        Cell::blank(),
        Cell::blank_with_attrs(
            CellAttributes::default()
                .set_background(AnsiColor::Blue)
                .clone(),
        ),
        Cell::new('x', CellAttributes::default()),
        Cell::new('E', CellAttributes::default()),
    ]
}

#[test]
fn fill_range_matches_reference_for_random_lines() {
    let hyperlink = Arc::new(Hyperlink::new("http://example.com/differential"));
    let attrs_pool = attrs_palette(&hyperlink);
    let fills = fill_cells_palette();

    for seed in 0..60u64 {
        let mut rng = Lcg::new(seed);
        let max_cols = 2 + rng.below(20);
        let pieces = random_pieces(&mut rng, &attrs_pool, max_cols);

        let base_v = build_v_line(&pieces, SEQ_ZERO);
        let mut base_c_compressed = base_v.clone();
        base_c_compressed.compress_for_scrollback();
        let base_c_appended = build_c_line_via_append(&pieces, SEQ_ZERO);

        let len = base_v.len();

        // Wide-char leading columns, so we can specifically target ranges
        // that straddle them on either edge.
        let wide_starts: Vec<usize> = base_v
            .visible_cells()
            .filter(|c| c.width() > 1)
            .map(|c| c.cell_index())
            .take(2)
            .collect();

        let mut ranges: Vec<Range<usize>> = vec![
            0..0,
            0..len,
            0..(len + 5),
            len..len,
            len..(len + 3),
            (len + 2)..(len + 6),
        ];
        if len > 0 {
            ranges.push(0..1);
            ranges.push((len.saturating_sub(1))..len);
        }
        for &w in &wide_starts {
            ranges.push(w..(w + 1)); // touches only the leading column
            ranges.push((w + 1)..(w + 2).max(len)); // straddles from the right half
            ranges.push((w + 1)..len.max(w + 1)); // straddles, extends to/past end
            if w > 0 {
                ranges.push(w..len); // starts exactly at the wide char, reaches end
            }
        }
        for _ in 0..3 {
            let a = rng.below(len + 6);
            let b = a + rng.below(len + 6);
            ranges.push(a..b);
        }

        for (fill_idx, cell) in fills.iter().enumerate() {
            for (range_idx, cols) in ranges.iter().enumerate() {
                for (label, base) in [
                    ("v", &base_v),
                    ("c-compressed", &base_c_compressed),
                    ("c-appended", &base_c_appended),
                ] {
                    let seqno = SEQ_ZERO + 1;
                    let mut under_test = base.clone();
                    let mut reference = base.clone();
                    if !under_test.is_empty() && rng.chance_one_in(3) {
                        under_test.set_last_cell_was_wrapped(true, seqno);
                        reference.set_last_cell_was_wrapped(true, seqno);
                    }

                    let apply_seqno = seqno + 1;
                    under_test.fill_range(cols.clone(), cell, apply_seqno);
                    reference_fill_range(&mut reference, cols.clone(), cell, apply_seqno);

                    assert_eq!(
                        observe(&under_test),
                        observe(&reference),
                        "seed={} storage={} fill_idx={} range_idx={} cols={:?} max_cols={}",
                        seed,
                        label,
                        fill_idx,
                        range_idx,
                        cols,
                        max_cols
                    );
                }
            }
        }
    }
}

/// A clustered line that is fully erased-to-end with a blank fill cell
/// must never be materialized as Vec storage -- that is the whole point
/// of the bulk rewrite (see `fill_cluster_to_end`).
#[test]
fn fill_range_erase_to_end_stays_clustered() {
    let hyperlink = Arc::new(Hyperlink::new("http://example.com/clustered"));
    let attrs_pool = attrs_palette(&hyperlink);
    let mut rng = Lcg::new(42);
    let pieces = random_pieces(&mut rng, &attrs_pool, 24);
    let mut line = build_c_line_via_append(&pieces, SEQ_ZERO);
    assert!(
        matches!(line.cells, CellStorage::C(_)),
        "test setup must start out clustered"
    );

    let len = line.len();
    let start = len / 3;
    line.fill_range(start..len, &Cell::blank(), SEQ_ZERO + 1);

    assert!(
        matches!(line.cells, CellStorage::C(_)),
        "erasing to end of line with a blank cell must not convert clustered storage to Vec"
    );
    assert_eq!(line.len(), start);
}

/// Same as above, but the range extends past the current end of the line
/// (as ECH/EL commonly do when the cursor is already at the true content
/// boundary): still must not convert.
#[test]
fn fill_range_erase_past_end_stays_clustered() {
    let mut line = build_c_line_via_append(
        &[
            ("h", 1, CellAttributes::default()),
            ("i", 1, CellAttributes::default()),
        ],
        SEQ_ZERO,
    );
    line.fill_range(2..120, &Cell::blank(), SEQ_ZERO + 1);
    assert!(matches!(line.cells, CellStorage::C(_)));
    assert_eq!(line.len(), 2);
    assert_eq!(line.as_str(), "hi");
}

/// Erasing up to a wide last cell must leave the clustered line knowing
/// that cell's width: a later wrap mark would otherwise split the wide
/// cell's cluster and be lost.
#[test]
fn erase_to_wide_tail_then_wrap_keeps_the_mark() {
    let bold = CellAttributes::default()
        .set_intensity(Intensity::Bold)
        .clone();
    let pieces: Vec<Piece> = vec![
        ("a", 1, CellAttributes::default()),
        ("日", 2, bold),
        ("b", 1, CellAttributes::default()),
    ];
    let builders: [fn(&[Piece], SequenceNo) -> Line; 2] = [build_v_line, build_c_line_via_append];
    for build in builders {
        let mut under_test = build(&pieces, SEQ_ZERO);
        let mut reference = build(&pieces, SEQ_ZERO);
        under_test.fill_range(3..10, &Cell::blank(), 2);
        reference_fill_range(&mut reference, 3..10, &Cell::blank(), 2);
        under_test.set_last_cell_was_wrapped(true, 3);
        reference.set_last_cell_was_wrapped(true, 3);

        assert!(under_test.last_cell_was_wrapped());
        assert_eq!(observe(&under_test), observe(&reference));
    }
}

/// The clustered cut must follow the same grapheme segmentation as the
/// cells themselves, for every cell boundary of mixed-script text.
#[test]
fn erase_at_every_boundary_agrees_across_scripts() {
    let bold = CellAttributes::default()
        .set_intensity(Intensity::Bold)
        .clone();
    let text = "क्षa👨\u{200d}👩\u{200d}👧🇺🇸é日b";
    let base_v = Line::from_text(text, &bold, SEQ_ZERO, None);
    let mut base_c = base_v.clone();
    base_c.compress_for_scrollback();
    let len = base_v.len();
    let boundaries: Vec<usize> = base_v.visible_cells().map(|c| c.cell_index()).collect();

    for &start in &boundaries {
        for base in [&base_v, &base_c] {
            let mut under_test = base.clone();
            let mut reference = base.clone();
            under_test.fill_range(start..len + 2, &Cell::blank(), 2);
            reference_fill_range(&mut reference, start..len + 2, &Cell::blank(), 2);
            assert_eq!(observe(&under_test), observe(&reference), "start={}", start);

            under_test.set_last_cell_was_wrapped(true, 3);
            reference.set_last_cell_was_wrapped(true, 3);
            assert_eq!(
                observe(&under_test),
                observe(&reference),
                "wrapped, start={}",
                start
            );
        }
    }
}
