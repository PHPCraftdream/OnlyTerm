#![cfg(test)]
//! Differential tests for the O(1) clustered-storage fast paths in
//! `Line::last_cell_was_wrapped` and `Line::is_whitespace`: the new
//! `ClusteredLine::last_cell_attrs`/`is_all_spaces` reads must agree with
//! the storage-agnostic reference (`visible_cells().last()` /
//! `visible_cells().all(...)`) across every mutation that can change
//! which cell is last or whether the line is blank.
//!
//! Lives as a child of `crate::line::line` (registered from
//! `line/line/mod.rs`, like `ascii_run_test.rs`) so it can match on
//! `Line`'s `pub(crate)` `cells` field to assert which storage a given
//! case is actually exercising.

use super::*;
use crate::alloc::string::ToString;
use k9::assert_equal as assert_eq;
use onlyterm_cell::color::AnsiColor;

/// Reference for `last_cell_was_wrapped`: the pre-optimization behavior,
/// bypassing both the cache and the clustered fast path.
fn reference_last_cell_wrapped(line: &Line) -> bool {
    line.visible_cells()
        .last()
        .map(|c| c.attrs().wrapped())
        .unwrap_or(false)
}

/// Reference for `is_whitespace`: the pre-optimization behavior.
fn reference_is_whitespace(line: &Line) -> bool {
    line.visible_cells().all(|c| c.str() == " ")
}

fn assert_matches_reference(line: &Line, label: &str) {
    assert_eq!(
        line.last_cell_was_wrapped(),
        reference_last_cell_wrapped(line),
        "last_cell_was_wrapped mismatch: {}",
        label
    );
    assert_eq!(
        line.is_whitespace(),
        reference_is_whitespace(line),
        "is_whitespace mismatch: {}",
        label
    );
}

#[test]
fn empty_line() {
    let v = Line::with_width(0, SEQ_ZERO);
    assert!(matches!(v.cells, CellStorage::V(_)));
    assert_matches_reference(&v, "empty V");

    let c = Line::new(SEQ_ZERO);
    assert!(matches!(c.cells, CellStorage::C(_)));
    assert_matches_reference(&c, "empty C");
}

#[test]
fn set_last_cell_was_wrapped_true_and_false() {
    for wrapped in [true, false] {
        let mut v: Line = "hello".into();
        v.set_last_cell_was_wrapped(wrapped, 1);
        assert_matches_reference(&v, "V set wrapped");

        let mut c: Line = "hello".into();
        c.compress_for_scrollback();
        assert!(matches!(c.cells, CellStorage::C(_)));
        c.set_last_cell_was_wrapped(wrapped, 1);
        assert_matches_reference(&c, "C set wrapped");
    }
}

#[test]
fn set_last_cell_was_wrapped_wide_last_cell() {
    for wrapped in [true, false] {
        let mut v = Line::from_text("a日", &CellAttributes::default(), SEQ_ZERO, None);
        v.set_last_cell_was_wrapped(wrapped, 1);
        assert_matches_reference(&v, "V wide last cell");

        let mut c = Line::from_text("a日", &CellAttributes::default(), SEQ_ZERO, None);
        c.compress_for_scrollback();
        assert!(matches!(c.cells, CellStorage::C(_)));
        c.set_last_cell_was_wrapped(wrapped, 1);
        assert_matches_reference(&c, "C wide last cell");
    }
}

/// A line marked wrapped, then more content appended past the end: the
/// new last cell is fresh (unwrapped) content, not the old wrapped one.
#[test]
fn append_after_wrap_clears_wrapped() {
    let mut c = Line::new(SEQ_ZERO);
    c.set_cell_grapheme(0, "a", 1, CellAttributes::default(), SEQ_ZERO);
    c.set_cell_grapheme(1, "b", 1, CellAttributes::default(), SEQ_ZERO);
    c.set_last_cell_was_wrapped(true, 1);
    assert_matches_reference(&c, "C wrapped before append");
    assert_eq!(c.last_cell_was_wrapped(), true);

    c.set_cell_grapheme(c.len(), "c", 1, CellAttributes::default(), 2);
    assert_matches_reference(&c, "C after append past wrap");
    assert_eq!(
        c.last_cell_was_wrapped(),
        false,
        "appending new content must make the new last cell unwrapped"
    );
}

#[test]
fn prune_trailing_blanks_updates_last_cell() {
    let mut c = Line::new(SEQ_ZERO);
    c.set_cell_grapheme(0, "a", 1, CellAttributes::default(), SEQ_ZERO);
    c.set_cell_grapheme(1, "b", 1, CellAttributes::default(), SEQ_ZERO);
    c.set_cell_grapheme(2, " ", 1, CellAttributes::default(), SEQ_ZERO);
    c.set_cell_grapheme(3, " ", 1, CellAttributes::default(), SEQ_ZERO);
    assert!(matches!(c.cells, CellStorage::C(_)));
    assert_matches_reference(&c, "C before prune");

    c.prune_trailing_blanks(1);
    assert_matches_reference(&c, "C after prune");
    assert_eq!(c.as_str(), "ab");
}

/// `fill_range` reaching the end of clustered storage takes the
/// truncate-then-append fast path (`fill_cluster_to_end`); the new last
/// cell after that truncation must still read correctly.
#[test]
fn truncation_via_fill_range_to_end() {
    let mut c = Line::new(SEQ_ZERO);
    for (i, ch) in "abcdef".chars().enumerate() {
        c.set_cell_grapheme(i, &ch.to_string(), 1, CellAttributes::default(), SEQ_ZERO);
    }
    assert!(matches!(c.cells, CellStorage::C(_)));
    c.set_last_cell_was_wrapped(true, 1);
    assert_matches_reference(&c, "C before fill_range truncation");

    c.fill_range(3..6, &Cell::blank(), 2);
    assert_matches_reference(&c, "C after fill_range truncation to end");
}

#[test]
fn is_whitespace_all_blank_and_colored_blank() {
    for (label, attr) in [
        ("default blank", CellAttributes::default()),
        (
            "colored blank",
            CellAttributes::default()
                .set_background(AnsiColor::Red)
                .clone(),
        ),
    ] {
        let v = Line::with_width_and_cell(4, Cell::new(' ', attr.clone()), SEQ_ZERO);
        assert_matches_reference(&v, &format!("V {}", label));

        let mut c = Line::new(SEQ_ZERO);
        for i in 0..4 {
            c.set_cell_grapheme(i, " ", 1, attr.clone(), SEQ_ZERO);
        }
        assert!(matches!(c.cells, CellStorage::C(_)));
        assert_matches_reference(&c, &format!("C {}", label));
    }
}

#[test]
fn is_whitespace_non_blank_content() {
    let v: Line = "ab  ".into();
    assert_matches_reference(&v, "V non-blank");

    let mut c: Line = "  ab".into();
    c.compress_for_scrollback();
    assert!(matches!(c.cells, CellStorage::C(_)));
    assert_matches_reference(&c, "C non-blank");
}

#[test]
fn is_whitespace_empty_line() {
    let v = Line::with_width(0, SEQ_ZERO);
    assert_matches_reference(&v, "empty V is whitespace");

    let c = Line::new(SEQ_ZERO);
    assert_matches_reference(&c, "empty C is whitespace");
}
