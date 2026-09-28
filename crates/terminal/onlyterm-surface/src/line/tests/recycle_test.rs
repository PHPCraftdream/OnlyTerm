#![cfg(test)]
//! Unit tests for `Line::recycle_as_blank`, which `Screen::scroll_up`
//! (onlyterm-term) uses to reuse an evicted line's storage for the blank
//! row it needs at the bottom, instead of allocating `Line::new()` fresh.
//!
//! Lives as a child of `crate::line::line` (registered from
//! `line/line/mod.rs`, like `cow_test.rs`/`fill_range_test.rs`) so it can
//! read `Line`'s private fields (`bits`, `seqno`, `zones`, the wrap-cache
//! atomics) directly to prove full field-by-field equivalence with a fresh
//! `Line::new()`, not just what's reachable through the public API.

use super::*;
use alloc::string::ToString;
use core::sync::atomic::Ordering::Relaxed;
use k9::assert_equal as assert_eq;
use onlyterm_cell::CellAttributes;

/// Builds a non-trivial clustered line by appending cell-by-cell (keeps
/// `CellStorage::C`, the storage kind `recycle_as_blank` can reuse).
fn build_line(seqno: SequenceNo, text: &str) -> Line {
    let mut line = Line::new(seqno);
    for (i, ch) in text.chars().enumerate() {
        line.set_cell_grapheme(i, &ch.to_string(), 1, CellAttributes::default(), seqno);
    }
    line
}

fn cluster_arc_ptr_and_cap(line: &Line) -> (usize, usize) {
    match &line.cells {
        CellStorage::C(arc) => (Arc::as_ptr(arc) as usize, arc.text.capacity()),
        CellStorage::V(_) => panic!("expected cluster storage"),
    }
}

/// Storage is reused (same `Arc` allocation, capacity never shrinks) when
/// `self` is the sole owner of it.
#[test]
fn recycle_as_blank_reuses_storage_when_unique() {
    let line = build_line(SEQ_ZERO, "hello world");
    let (before_ptr, before_cap) = cluster_arc_ptr_and_cap(&line);
    assert!(before_cap >= "hello world".len());

    let recycled = line.recycle_as_blank(SEQ_ZERO + 5);

    let (after_ptr, after_cap) = cluster_arc_ptr_and_cap(&recycled);
    assert_eq!(
        after_ptr, before_ptr,
        "the ClusteredLine allocation must be reused when uniquely owned"
    );
    assert!(
        after_cap >= before_cap,
        "recycling must not shrink the retained capacity"
    );
    assert_eq!(recycled.as_str(), "");
}

/// After recycling, every field `Line::new(seqno)` would set must match --
/// not just what's visible through `as_str()`/`len()`.
#[test]
fn recycle_as_blank_matches_fresh_line_new_field_by_field() {
    let mut line = build_line(SEQ_ZERO, "hello world");
    line.set_double_width(SEQ_ZERO);
    line.set_last_cell_was_wrapped(true, SEQ_ZERO);
    assert!(
        !line.semantic_zone_ranges().is_empty(),
        "precondition: zones must be populated before recycling"
    );
    assert!(
        line.last_cell_was_wrapped(),
        "precondition: wrap cache must be populated before recycling"
    );

    let seqno = SEQ_ZERO + 42;
    let recycled = line.recycle_as_blank(seqno);
    let fresh = Line::new(seqno);

    assert_eq!(recycled.bits, fresh.bits, "bits");
    assert_eq!(recycled.seqno, fresh.seqno, "seqno");
    assert_eq!(recycled.current_seqno(), seqno);
    assert!(recycled.zones.is_empty(), "zones must reset to empty");
    assert_eq!(recycled.cells, fresh.cells, "cells (text/clusters/len/etc)");
    assert_eq!(
        recycled.cached_last_cell_was_wrapped.load(Relaxed),
        fresh.cached_last_cell_was_wrapped.load(Relaxed),
        "cached_last_cell_was_wrapped"
    );
    assert_eq!(
        recycled.cached_last_cell_wrapped_seqno.load(Relaxed),
        fresh.cached_last_cell_wrapped_seqno.load(Relaxed),
        "cached_last_cell_wrapped_seqno"
    );
    assert_eq!(recycled.is_single_width(), fresh.is_single_width());
    assert_eq!(recycled.bidi_info(), fresh.bidi_info());
    assert_eq!(
        recycled.last_cell_was_wrapped(),
        fresh.last_cell_was_wrapped()
    );
    assert_eq!(recycled.as_str(), fresh.as_str());
    assert_eq!(recycled.len(), fresh.len());
}

/// A render snapshot (an `Arc` clone of the line, the way the GUI keeps
/// per-frame snapshots) must never observe a mutation: when storage is
/// still shared, recycling must fall back to a fresh allocation rather
/// than clearing the shared one in place.
#[test]
fn recycle_as_blank_falls_back_and_leaves_shared_clone_untouched() {
    let line = build_line(SEQ_ZERO, "clone-me");
    let snapshot = line.clone();
    let (snapshot_ptr, _) = cluster_arc_ptr_and_cap(&snapshot);

    let recycled = line.recycle_as_blank(SEQ_ZERO + 1);

    // The snapshot is completely unaffected: same allocation, same content.
    let (snapshot_ptr_after, _) = cluster_arc_ptr_and_cap(&snapshot);
    assert_eq!(snapshot_ptr_after, snapshot_ptr);
    assert_eq!(snapshot.as_str(), "clone-me");

    // The recycled line must be a genuinely fresh allocation -- reusing the
    // still-shared one in place would have corrupted the snapshot above.
    let (recycled_ptr, recycled_cap) = cluster_arc_ptr_and_cap(&recycled);
    assert_ne!(
        recycled_ptr, snapshot_ptr,
        "must not reuse storage that a live clone still references"
    );
    assert_eq!(
        recycled_cap, 0,
        "fresh ClusteredLine::new() reserves no text capacity"
    );
    assert_eq!(recycled.as_str(), "");
    assert_eq!(recycled.len(), 0);
}
