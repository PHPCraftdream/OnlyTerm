//! `Screen::scroll_up`'s "recycle the evicted line" path: when the
//! scrollback is full and the blank attribute is the default one, the
//! comment above that branch claims lines are moved "to avoid thrashing
//! the heap" -- but until this fix the default-attribute branch dropped
//! the evicted line and allocated `Line::new()` fresh instead. These tests
//! drive a real `Terminal` to force that eviction and check, through
//! `Line::debug_cluster_text_storage` (a test/diagnostic-only accessor;
//! `onlyterm-surface`'s own crate-internal tests cover
//! `Line::recycle_as_blank` more thoroughly via private-field access),
//! that the evicted line's storage is actually reused when it's safe to,
//! and never touched when a render snapshot still holds a clone of it.
use super::*;

/// physical_rows=2, scrollback=1 => max_allowed = 3 total lines. Prints
/// "row\r\n" until the ring buffer holds exactly `max_allowed` lines, i.e.
/// the point where the NEXT scroll evicts exactly one line.
fn fill_to_capacity(term: &mut TestTerm, max_allowed: usize) {
    while term.screen().scrollback_rows() < max_allowed {
        term.print("row\r\n");
    }
    std::assert_eq!(term.screen().scrollback_rows(), max_allowed);
}

#[test]
fn scroll_up_recycles_evicted_line_storage_when_unique() {
    let mut term = TestTerm::new(2, 8, 1);
    fill_to_capacity(&mut term, 3);

    let (before_ptr, before_cap) = term
        .screen_mut()
        .line_mut(0)
        .debug_cluster_text_storage()
        .expect("topmost line should use cluster storage");
    assert!(
        before_cap > 0,
        "precondition: the line about to be evicted has content"
    );

    // Write to the current bottom row, then scroll: this evicts phys index
    // 0 (the line whose identity was just captured) and, on the recycle
    // path, should push it back as the new blank bottom row.
    term.print("next\r\n");

    std::assert_eq!(
        term.screen().scrollback_rows(),
        3,
        "eviction keeps the ring at capacity"
    );
    let bottom_idx = term.screen().scrollback_rows() - 1;
    let (after_ptr, after_cap) = term
        .screen_mut()
        .line_mut(bottom_idx)
        .debug_cluster_text_storage()
        .expect("recycled line should still use cluster storage");

    std::assert_eq!(
        after_ptr,
        before_ptr,
        "scroll_up must reuse the evicted line's storage"
    );
    assert!(
        after_cap >= before_cap,
        "recycling must not shrink capacity"
    );
    std::assert_eq!(
        term.screen_mut().line_mut(bottom_idx).as_str(),
        "",
        "recycled line must be blank"
    );
}

/// If a render snapshot (an `Arc` clone of the evicted line, mirroring what
/// the GUI keeps for per-frame rendering) is still alive, `scroll_up` must
/// not mutate the storage the snapshot references.
#[test]
fn scroll_up_recycle_does_not_disturb_a_live_snapshot() {
    let mut term = TestTerm::new(2, 8, 1);
    fill_to_capacity(&mut term, 3);

    let snapshot = term.screen_mut().line_mut(0).clone();
    let snapshot_content = snapshot.as_str().to_string();
    let (snapshot_ptr, _) = snapshot
        .debug_cluster_text_storage()
        .expect("cluster storage");

    term.print("next\r\n");

    // The snapshot is completely unaffected.
    std::assert_eq!(snapshot.as_str(), snapshot_content);
    let (snapshot_ptr_after, _) = snapshot
        .debug_cluster_text_storage()
        .expect("cluster storage");
    std::assert_eq!(snapshot_ptr_after, snapshot_ptr);

    // The screen keeps working correctly regardless of which path
    // (reuse vs. fresh allocation) recycling had to take.
    let bottom_idx = term.screen().scrollback_rows() - 1;
    std::assert_eq!(term.screen_mut().line_mut(bottom_idx).as_str(), "");
}
