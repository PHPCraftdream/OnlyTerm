//! Regression tests for task B2 (per-frame hyperlink pass): a per-frame
//! render must not call `Screen::for_each_logical_line_in_stable_range_mut`
//! (and pay for its per-logical-line `Vec<&mut Line>` allocation via
//! `Screen::with_phys_lines_mut`) when nothing in the requested range
//! actually needs a hyperlink scan.
//!
//! `Screen::hyperlink_scan_needed_in_stable_range` is the gate
//! `LocalPane::get_render_snapshot` uses to decide whether to call that
//! walk at all. These tests exercise it directly against a real `Screen`,
//! mirroring `LocalPane`'s own gating pattern (`if needs_scan { call the
//! walk }`) so the count of walk invocations is asserted directly rather
//! than inferred.

use super::*;
use onlyterm_surface::hyperlink::Rule;

fn hyperlink_rules() -> Vec<Rule> {
    vec![Rule::new(r"\b\w+://(?:[\w.-]+)\.[a-z]{2,15}\S*\b", "$0").unwrap()]
}

/// Runs the walk under the same gate `LocalPane::get_render_snapshot` uses,
/// returning how many logical lines were actually visited.
fn gated_scan(
    term: &mut TestTerm,
    rules: &[Rule],
    range: std::ops::Range<StableRowIndex>,
) -> usize {
    let mut visited = 0usize;
    if term
        .screen()
        .hyperlink_scan_needed_in_stable_range(range.clone())
    {
        term.screen_mut().for_each_logical_line_in_stable_range_mut(
            range,
            |_stable_range, lines| {
                visited += 1;
                Line::apply_hyperlink_rules(rules, lines);
                true
            },
        );
    }
    visited
}

#[test]
fn hyperlink_scan_extends_across_wrap_and_is_skipped_once_scanned() {
    // width=10: "http://example.com" (18 chars) auto-wraps as
    // "http://exa" (row 0) + "mple.com" (row 1).
    let mut term = TestTerm::new(6, 10, 10);
    term.print("http://example.com");
    let rules = hyperlink_rules();

    let row0_wrapped = {
        let mut wrapped = false;
        term.screen()
            .with_phys_lines(0..1, |lines| wrapped = lines[0].last_cell_was_wrapped());
        wrapped
    };
    assert!(
        row0_wrapped,
        "row 0 must wrap into row 1 for this test to be meaningful"
    );

    // The caller (mirroring the GUI's viewport) only asks about row 1 --
    // the tail of the wrapped logical line.
    let range: std::ops::Range<StableRowIndex> = 1..2;

    assert!(
        term.screen()
            .hyperlink_scan_needed_in_stable_range(range.clone()),
        "neither physical row has been scanned yet"
    );

    let visited_first_pass = gated_scan(&mut term, &rules, range.clone());
    std::assert_eq!(
        visited_first_pass,
        1,
        "the wrapped logical line (rows 0..2) is one logical line, visited once, even \
         though the caller only asked about row 1"
    );

    let (row0_scanned, row1_scanned, row0_has_link, row1_has_link, seqno0, seqno1) = {
        let mut r = (false, false, false, false, 0, 0);
        term.screen().with_phys_lines(0..2, |lines| {
            r.0 = lines[0].implicit_hyperlinks_scanned();
            r.1 = lines[1].implicit_hyperlinks_scanned();
            r.2 = lines[0].has_hyperlink();
            r.3 = lines[1].has_hyperlink();
            r.4 = lines[0].current_seqno();
            r.5 = lines[1].current_seqno();
        });
        r
    };
    assert!(
        row0_scanned && row1_scanned,
        "both physical rows of the logical line must end up scanned"
    );
    assert!(
        row0_has_link && row1_has_link,
        "the hyperlink must be applied to both physical rows of the wrapped logical line, \
         even though only the second row was in the requested viewport"
    );

    // Second pass: same range, nothing changed since. The gate must report
    // "no work needed" and the walk must not be invoked at all -- this is
    // the actual allocation this task removes, not just a per-line no-op.
    assert!(
        !term
            .screen()
            .hyperlink_scan_needed_in_stable_range(range.clone()),
        "already-scanned rows must report no work needed"
    );
    let visited_second_pass = gated_scan(&mut term, &rules, range.clone());
    std::assert_eq!(
        visited_second_pass,
        0,
        "the mutating walk must never run once every line in range is already scanned"
    );

    // Untouched: the walk never ran, so nothing could have bumped seqno.
    let (seqno0_after, seqno1_after) = {
        let mut s = (0, 0);
        term.screen().with_phys_lines(0..2, |lines| {
            s.0 = lines[0].current_seqno();
            s.1 = lines[1].current_seqno();
        });
        s
    };
    std::assert_eq!(
        seqno0,
        seqno0_after,
        "row 0 must not be mutated by a skipped scan"
    );
    std::assert_eq!(
        seqno1,
        seqno1_after,
        "row 1 must not be mutated by a skipped scan"
    );
}

#[test]
fn hyperlink_scan_needed_is_false_when_rules_are_empty_is_caller_responsibility() {
    // `hyperlink_scan_needed_in_stable_range` itself only answers "is there
    // an unscanned line here" -- it doesn't know about `rules` at all.
    // Callers (both `LocalPane::get_render_snapshot` and the default
    // `Pane::apply_hyperlinks`) are responsible for skipping on an empty
    // rule list before even asking. This test documents that split: with
    // unscanned content and an empty rule slice, the gate still reports
    // "needs scan" (there IS an unscanned line), but applying zero rules to
    // it is a correctly-behaved no-op.
    let mut term = TestTerm::new(6, 10, 10);
    term.print("plain text, no links here");

    let range: std::ops::Range<StableRowIndex> = 0..1;
    assert!(term
        .screen()
        .hyperlink_scan_needed_in_stable_range(range.clone()));

    let visited = gated_scan(&mut term, &[], range);
    std::assert_eq!(visited, 1);
}
