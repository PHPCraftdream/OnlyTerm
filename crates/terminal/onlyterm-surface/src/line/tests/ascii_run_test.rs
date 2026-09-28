#![cfg(test)]
//! Unit tests for `Line::set_ascii_run`, the bulk API backing the `term`
//! crate's bulk-ASCII print fast path (see `Performer::flush_print`).
//! Compares the single bulk call against calling `set_cell_grapheme` once
//! per byte -- the pre-optimization way to write the same run -- across
//! both V and clustered storage, including writes that land past the
//! current end (padded with blanks), writes that append exactly at the
//! end of clustered storage (the fast path `append_ascii_run` takes), and
//! writes that overwrite existing content straddling a wide character at
//! either edge.
//!
//! Lives as a child of `crate::line::line` (registered from
//! `line/line/mod.rs`, like `fill_range_test.rs`) so it can read `Line`'s
//! private `bits` field directly.

use super::*;
use crate::hyperlink::Hyperlink;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;
use k9::assert_equal as assert_eq;
use onlyterm_cell::color::AnsiColor;
use onlyterm_cell::Intensity;

type Piece = (&'static str, usize, CellAttributes);

/// a=0 日=1-2 本=3-4 b=5 c=6 d=7; len=8. Deliberately includes two
/// adjacent wide graphemes so idx values 1..5 straddle a wide char at
/// either the left or right edge of a write.
fn pieces() -> Vec<Piece> {
    let hyperlink = Arc::new(Hyperlink::new("http://example.com/ascii-run-base"));
    let link_attr = CellAttributes::default()
        .set_hyperlink(Some(hyperlink))
        .clone();
    vec![
        ("a", 1, CellAttributes::default()),
        (
            "日",
            2,
            CellAttributes::default()
                .set_background(AnsiColor::Blue)
                .clone(),
        ),
        ("本", 2, CellAttributes::default()),
        ("b", 1, link_attr),
        ("c", 1, CellAttributes::default()),
        ("d", 1, CellAttributes::default()),
    ]
}

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

/// Builds clustered storage via incremental grapheme appends, matching
/// how a real terminal accumulates a line.
fn build_c_line(pieces: &[Piece], seqno: SequenceNo) -> Line {
    let mut line = Line::new(seqno);
    for (text, width, attrs) in pieces {
        let idx = line.len();
        line.set_cell_grapheme(idx, text, *width, attrs.clone(), seqno);
    }
    line
}

#[derive(Debug, PartialEq)]
struct Observed {
    text: String,
    cells: Vec<(usize, usize, CellAttributes)>,
    len: usize,
    has_hyperlink: bool,
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
        has_hyperlink: line.has_hyperlink(),
        bits: line.bits.bits(),
    }
}

/// Reference: write `text` one byte at a time via `set_cell_grapheme`,
/// the way the pre-bulk per-grapheme print loop would.
fn reference_ascii_run(
    line: &mut Line,
    idx: usize,
    text: &str,
    attr: &CellAttributes,
    seqno: SequenceNo,
) {
    for (i, byte) in text.bytes().enumerate() {
        let ch = (byte as char).to_string();
        line.set_cell_grapheme(idx + i, &ch, 1, attr.clone(), seqno);
    }
}

#[test]
fn set_ascii_run_matches_per_cell_loop() {
    let base_pieces = pieces();
    let base_v = build_v_line(&base_pieces, SEQ_ZERO);
    let base_c = build_c_line(&base_pieces, SEQ_ZERO);
    assert_eq!(base_v.len(), 8);
    assert_eq!(base_c.len(), 8);

    let hyperlink = Arc::new(Hyperlink::new("http://example.com/ascii-run-write"));
    let run_attrs = [
        CellAttributes::default(),
        CellAttributes::default()
            .set_intensity(Intensity::Bold)
            .clone(),
        CellAttributes::default()
            .set_hyperlink(Some(Arc::clone(&hyperlink)))
            .clone(),
    ];

    // Covers: left edge (0, no wide char before), straddling 日's
    // continuation column from the left (1), straddling 日's leading col
    // touched only (0..1 handled via run len), landing exactly on 本's
    // leading column after overwriting 日 fully (1..3 range via idx=1
    // len=2), interior aligned boundary (5), append exactly at end (8),
    // and past-the-end (needs blank padding) (11).
    let idxs = [0usize, 1, 2, 3, 4, 5, 8, 11];
    // Leading and all-space runs hit the implicit-blank skip past the end.
    let run_texts = ["X", "XY", "XYZ!", "12345", " ", "   ", "  a", " a b "];

    for &idx in &idxs {
        for &run in &run_texts {
            for attr in &run_attrs {
                for (label, base) in [("v", &base_v), ("c", &base_c)] {
                    let seqno = SEQ_ZERO + 1;
                    let mut under_test = base.clone();
                    let mut reference = base.clone();

                    under_test.set_ascii_run(idx, run, attr, seqno);
                    reference_ascii_run(&mut reference, idx, run, attr, seqno);

                    assert_eq!(
                        observe(&under_test),
                        observe(&reference),
                        "storage={} idx={} run={:?} attr={:?}",
                        label,
                        idx,
                        run,
                        attr
                    );
                }
            }
        }
    }
}

/// Writing exactly at the current end of clustered storage must take the
/// `append_ascii_run` fast path and never coerce to Vec storage.
#[test]
fn set_ascii_run_append_at_end_stays_clustered() {
    let base_pieces = pieces();
    let mut line = build_c_line(&base_pieces, SEQ_ZERO);
    assert!(matches!(line.cells, CellStorage::C(_)));

    let len = line.len();
    line.set_ascii_run(len, "tail", &CellAttributes::default(), SEQ_ZERO + 1);

    assert!(
        matches!(line.cells, CellStorage::C(_)),
        "appending exactly at the end of clustered storage must stay clustered"
    );
    assert_eq!(line.as_str(), "a日本bcdtail");
}

/// Writing past the end of clustered storage must pad with blanks but
/// still stay clustered (no per-cell Vec conversion needed).
#[test]
fn set_ascii_run_past_end_pads_with_blanks_and_stays_clustered() {
    let mut line = build_c_line(&pieces(), SEQ_ZERO);
    let len = line.len();

    line.set_ascii_run(len + 3, "Z", &CellAttributes::default(), SEQ_ZERO + 1);

    assert!(matches!(line.cells, CellStorage::C(_)));
    assert_eq!(line.as_str(), "a日本bcd   Z");
}

/// Writing into the interior of clustered storage must coerce to Vec
/// storage, exactly like the per-cell loop eventually would.
#[test]
fn set_ascii_run_interior_write_coerces_clustered_to_vec() {
    let mut line = build_c_line(&pieces(), SEQ_ZERO);
    assert!(matches!(line.cells, CellStorage::C(_)));

    line.set_ascii_run(5, "XY", &CellAttributes::default(), SEQ_ZERO + 1);

    assert!(matches!(line.cells, CellStorage::V(_)));
    assert_eq!(line.as_str(), "a日本XYd");
}

/// A run whose attributes carry a hyperlink must set `HAS_HYPERLINK` on
/// the line, for both storage kinds.
#[test]
fn set_ascii_run_sets_has_hyperlink_bit() {
    // A base with no hyperlink of its own, so the bit starts clear.
    let plain_pieces: Vec<Piece> = vec![
        ("a", 1, CellAttributes::default()),
        ("日", 2, CellAttributes::default()),
        ("b", 1, CellAttributes::default()),
    ];
    let hyperlink = Arc::new(Hyperlink::new("http://example.com/ascii-run-bit"));
    let attr = CellAttributes::default()
        .set_hyperlink(Some(hyperlink))
        .clone();

    for mut line in [
        build_v_line(&plain_pieces, SEQ_ZERO),
        build_c_line(&plain_pieces, SEQ_ZERO),
    ] {
        assert!(!line.has_hyperlink());
        let idx = line.len();
        line.set_ascii_run(idx, "link", &attr, SEQ_ZERO + 1);
        assert!(line.has_hyperlink());
    }
}
