#![cfg(test)]
//! BUG-33: `ClusteredLine::set_last_cell_was_wrapped` underflowed when the
//! line's last cell is zero-width (cluster `cell_width == 0`, while the
//! recorded/computed `last_cell_width` is 1): the split branch computed
//! `0 - 1`. All tests here go through the public `Line` API only.
//!
//! NOTE: `Line::new` + `set_cell_grapheme` builds *clustered* storage, so the
//! "v" lines below are clustered too (a width-0 cluster only arises from
//! clustered appends; `Line::from_cells` + `compress_for_scrollback` reports
//! such a cell with width 1). The "Vec vs C" names are historical; the
//! decisive assertions are absolute (flag location, cell count).
//!
//! NOTE on the oracle: for a zero-width cell, Vec and C storage differ in
//! *pre-existing* representational details that this fix does not touch:
//! Vec counts the cell in `len()` (cell count) and reports its width as 0,
//! while `from_cell_vec`/`iter` account widths (0 columns) and the
//! iterator reports width 1 for every non-wide grapheme. Equivalence is
//! therefore asserted on the wrapped flag, the last cell's attributes and
//! the cell *texts* -- the observables `set_last_cell_was_wrapped`
//! actually owns. U+2060 WORD JOINER is used where cell identity must
//! survive storage round-trips: unlike U+0301 it is not a grapheme
//! Extender, so neighbouring cells never fuse with it in C storage.

use super::Line;
use crate::SEQ_ZERO;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use onlyterm_cell::{CellAttributes, Intensity};

fn attrs_plain() -> CellAttributes {
    CellAttributes::default()
}

fn attrs_bold() -> CellAttributes {
    CellAttributes::default()
        .set_intensity(Intensity::Bold)
        .clone()
}

/// Cell texts + attrs of the visible cells, projected per storage.
fn projection(line: &Line) -> (Vec<(String, CellAttributes)>, bool) {
    let cells = line
        .visible_cells()
        .map(|c| (c.str().to_string(), c.attrs().clone()))
        .collect();
    (cells, line.last_cell_was_wrapped())
}

/// Minimal reproduction of the underflow (before the fix this panicked
/// with "attempt to subtract with overflow" at the cluster split).
#[test]
fn set_wrapped_zero_width_last_cell_compressed() {
    let mut line = Line::new(SEQ_ZERO);
    line.set_cell_grapheme(0, "a", 1, attrs_plain(), 1);
    // Distinct attrs so `from_cell_vec` cannot merge the zero-width cell
    // into the preceding cluster: it becomes a width-0 cluster of its own.
    line.set_cell_grapheme(1, "\u{301}", 0, attrs_bold(), 1);
    line.compress_for_scrollback();
    line.set_last_cell_was_wrapped(true, 2);
    assert!(
        line.last_cell_was_wrapped(),
        "wrapped flag must be observable on a zero-width last cell"
    );
}

#[test]
fn set_wrapped_zero_width_only_line_compressed() {
    // A line whose ONLY cell is zero-width: `len() == 0` but the cell is
    // real content -- the C path must not prepend an implicit blank (which
    // would diverge from Vec storage and swallow the flag's location).
    let mut v = Line::new(SEQ_ZERO);
    v.set_cell_grapheme(0, "\u{2060}", 0, attrs_bold(), 1);
    let mut c = v.clone();
    c.compress_for_scrollback();

    v.set_last_cell_was_wrapped(true, 2);
    c.set_last_cell_was_wrapped(true, 2);
    let (vc, vw) = projection(&v);
    let (cc, cw) = projection(&c);
    assert_eq!(vc, cc, "cell texts/attrs must match Vec storage");
    // Absolute check (both sides above are clustered, since `Line::new`
    // builds C storage): exactly the zero-width cell, flag on it, and no
    // implicit blank in front of or behind it.
    assert_eq!(cc.len(), 1, "no implicit blank may be appended: {:?}", cc);
    assert_eq!(cc[0].0, "\u{2060}");
    assert!(cc[0].1.wrapped());
    assert!(vw);
    assert!(cw);
    assert_eq!(
        v.last_cell_was_wrapped(),
        c.last_cell_was_wrapped(),
        "wrapped flag must agree across storages"
    );
}

/// Vec vs C equivalence for `set_last_cell_was_wrapped(true/false)` on a
/// line ending in a zero-width cell, across repeated toggles.
#[test]
fn vec_c_equivalence_zero_width_tail_toggle_sequence() {
    let build = || {
        let mut line = Line::new(SEQ_ZERO);
        line.set_cell_grapheme(0, "a", 1, attrs_plain(), 1);
        line.set_cell_grapheme(1, "\u{41f}", 1, attrs_bold(), 1);
        line.set_cell_grapheme(2, "\u{2060}", 0, attrs_plain(), 1);
        line
    };
    let mut v = build();
    let mut c = build();
    c.compress_for_scrollback();

    for wrapped in [true, false, true, true, false, true].iter() {
        v.set_last_cell_was_wrapped(*wrapped, 2);
        c.set_last_cell_was_wrapped(*wrapped, 2);
        let (vc, vw) = projection(&v);
        let (cc, cw) = projection(&c);
        assert_eq!(vc, cc, "cells must match after set_wrapped({})", wrapped);
        assert_eq!(vw, *wrapped);
        assert_eq!(cw, *wrapped, "C storage must observe the flag too");
        assert_eq!(vw, cw);
    }
}

/// Same as above but with every cluster carrying different attributes, so
/// the zero-width tail stays an isolated width-0 cluster through
/// `from_cell_vec`.
#[test]
fn vec_c_equivalence_distinct_attrs_per_cluster() {
    let attrs_mid = CellAttributes::default()
        .set_intensity(Intensity::Half)
        .clone();
    let build = || {
        let mut line = Line::new(SEQ_ZERO);
        line.set_cell_grapheme(0, "x", 1, attrs_plain(), 1);
        line.set_cell_grapheme(1, "y", 1, attrs_mid.clone(), 1);
        line.set_cell_grapheme(2, "\u{2060}", 0, attrs_bold(), 1);
        line
    };
    let mut v = build();
    let mut c = build();
    c.compress_for_scrollback();

    v.set_last_cell_was_wrapped(true, 2);
    c.set_last_cell_was_wrapped(true, 2);
    let (vc, vw) = projection(&v);
    let (cc, cw) = projection(&c);
    assert_eq!(vc, cc);
    assert_eq!(vw, cw);
    assert!(cw);

    // And the flag lands on the zero-width cluster itself: the last cell's
    // attributes are wrapped, the preceding ones are not.
    let last = cc.last().unwrap();
    assert!(last.1.wrapped());
    assert!(!cc[0].1.wrapped());
    assert!(!cc[1].1.wrapped());
}

/// A line NOT ending in a zero-width cell must be untouched by the fix
/// (bit-identical behaviour): split/re-purpose decisions are unchanged.
#[test]
fn normal_tail_unchanged() {
    let build = || {
        let mut line = Line::new(SEQ_ZERO);
        for (i, g) in ["a", "b", "c"].iter().enumerate() {
            line.set_cell_grapheme(i, g, 1, attrs_plain(), 1);
        }
        line
    };
    let mut v = build();
    let mut c = build();
    c.compress_for_scrollback();
    v.set_last_cell_was_wrapped(true, 2);
    c.set_last_cell_was_wrapped(true, 2);
    let (vc, vw) = projection(&v);
    let (cc, cw) = projection(&c);
    assert_eq!(vc, cc);
    assert!(vw);
    assert!(cw);
    assert_eq!(v.len(), c.len());

    // Wide tail: the split branch still runs (cluster wider than the last
    // cell) and must keep working.
    let mut wide_v = Line::new(SEQ_ZERO);
    wide_v.set_cell_grapheme(0, "\u{6f22}", 2, attrs_plain(), 1);
    let mut wide_c = wide_v.clone();
    wide_c.compress_for_scrollback();
    wide_v.set_last_cell_was_wrapped(true, 2);
    wide_c.set_last_cell_was_wrapped(true, 2);
    assert!(wide_v.last_cell_was_wrapped());
    assert!(wide_c.last_cell_was_wrapped());
    let (wv, _) = projection(&wide_v);
    let (wc, _) = projection(&wide_c);
    assert_eq!(wv, wc);
}
