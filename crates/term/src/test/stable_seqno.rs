//! Guards the "(StableRowIndex, seqno) identifies a unique line" contract.
//!
//! The GUI's `shape_hash_cache` (task #439) is keyed by
//! `(pane_id, stable_row)` and validated by
//! `entry.seqno == line.current_seqno()`, and `Line::last_cell_was_wrapped`'s
//! own memo (task #462) is seqno-keyed too. Both are only sound if the
//! terminal model guarantees:
//!
//!   for a given pane, the pair (StableRowIndex, Line::current_seqno())
//!   uniquely determines that row's visible content, for all time.
//!
//! Equivalently: any operation that changes *which* content lives at a given
//! StableRowIndex must bump the seqno of every affected line -- whether it
//! rewrote the line's cells, moved the line, or merely shifted
//! `stable_row_index_offset` out from under it.
//!
//! Task #476 was a live user-visible violation of exactly this: a line
//! rendered twice on screen, which no amount of further scrolling cleared,
//! because a poisoned `(stable_row, seqno)` cache entry kept being served.
//! These tests drive a real `Terminal` with real escape sequences and assert
//! that no `(stable_row, seqno)` pair is ever observed with two different
//! contents.

use super::*;
use std::collections::HashMap;

fn line_content(line: &Line) -> String {
    line.as_str().to_string()
}

struct Witness {
    seen: HashMap<(StableRowIndex, SequenceNo), String>,
    violations: Vec<String>,
}

impl Witness {
    fn new() -> Self {
        Self {
            seen: HashMap::new(),
            violations: vec![],
        }
    }

    fn observe(&mut self, term: &TestTerm, label: &str) {
        let screen = term.screen();
        let mut records = vec![];
        screen.for_each_phys_line(|phys, line| {
            records.push((
                screen.phys_to_stable_row_index(phys),
                line.current_seqno(),
                line_content(line),
            ));
        });
        for (stable, seqno, content) in records {
            match self.seen.get(&(stable, seqno)) {
                Some(prev) if *prev != content => {
                    self.violations.push(format!(
                        "at [{}]: stable_row={} seqno={} previously had content {:?} but now has {:?}",
                        label, stable, seqno, prev, content
                    ));
                }
                _ => {
                    self.seen.insert((stable, seqno), content);
                }
            }
        }
    }
}

/// Ordinary output + scrollback trimming.
#[test]
fn stable_seqno_invariant_plain_output() {
    let mut term = TestTerm::new(6, 20, 8);
    let mut w = Witness::new();
    w.observe(&term, "start");
    for i in 0..60 {
        term.print(format!("line {}\r\n", i));
        w.observe(&term, &format!("after line {}", i));
    }
    assert!(
        w.violations.is_empty(),
        "invariant violations:\n{}",
        w.violations.join("\n")
    );
}

/// Scroll region anchored at the top of the screen but not extending to the
/// bottom, on the PRIMARY screen (scrollback allowed).
#[test]
fn stable_seqno_invariant_top_anchored_scroll_region() {
    let mut term = TestTerm::new(10, 20, 10);
    let mut w = Witness::new();

    // fill the screen and some scrollback with distinct content
    for i in 0..25 {
        term.print(format!("row{:02}\r\n", i));
    }
    w.observe(&term, "filled");

    // Scroll region covering the top half of the visible screen only.
    term.set_scroll_region(0, 4);
    for i in 0..12 {
        term.cup(0, 4);
        term.print(format!("new{:02}\n", i));
        w.observe(&term, &format!("region scroll {}", i));
    }

    assert!(
        w.violations.is_empty(),
        "invariant violations:\n{}",
        w.violations.join("\n")
    );
}

/// IL/DL inside a scroll region.
#[test]
fn stable_seqno_invariant_insert_delete_lines() {
    let mut term = TestTerm::new(8, 20, 10);
    let mut w = Witness::new();
    for i in 0..20 {
        term.print(format!("row{:02}\r\n", i));
    }
    w.observe(&term, "filled");

    for i in 0..10 {
        term.cup(0, 2);
        term.print("\x1b[2L"); // insert 2 lines
        w.observe(&term, &format!("IL {}", i));
        term.cup(0, 3);
        term.delete_lines(1);
        w.observe(&term, &format!("DL {}", i));
        term.print(format!("mut{:02}", i));
        w.observe(&term, &format!("write {}", i));
    }

    assert!(
        w.violations.is_empty(),
        "invariant violations:\n{}",
        w.violations.join("\n")
    );
}

/// Alternate-screen enter/leave with content on both screens.
#[test]
fn stable_seqno_invariant_alt_screen() {
    let mut term = TestTerm::new(6, 20, 10);
    let mut w = Witness::new();
    for i in 0..15 {
        term.print(format!("prim{:02}\r\n", i));
    }
    w.observe(&term, "primary filled");

    for round in 0..4 {
        term.print("\x1b[?1049h");
        w.observe(&term, &format!("entered alt {}", round));
        for i in 0..5 {
            term.print(format!("alt{}-{}\r\n", round, i));
            w.observe(&term, &format!("alt write {} {}", round, i));
        }
        term.print("\x1b[?1049l");
        w.observe(&term, &format!("left alt {}", round));
        term.print(format!("post{}\r\n", round));
        w.observe(&term, &format!("post {}", round));
    }

    assert!(
        w.violations.is_empty(),
        "invariant violations:\n{}",
        w.violations.join("\n")
    );
}

/// Reverse index / scroll-down at the top of the screen (RI), which is what
/// `less`/pagers use when scrolling backwards.
#[test]
fn stable_seqno_invariant_reverse_index() {
    let mut term = TestTerm::new(8, 20, 10);
    let mut w = Witness::new();
    for i in 0..20 {
        term.print(format!("row{:02}\r\n", i));
    }
    w.observe(&term, "filled");

    for i in 0..12 {
        term.cup(0, 0);
        term.print("\x1bM"); // RI
        term.print(format!("ri{:02}", i));
        w.observe(&term, &format!("RI {}", i));
    }

    assert!(
        w.violations.is_empty(),
        "invariant violations:\n{}",
        w.violations.join("\n")
    );
}

/// Resize (with and without reflow) between bursts of output.
#[test]
fn stable_seqno_invariant_resize() {
    let mut term = TestTerm::new(8, 20, 20);
    let mut w = Witness::new();
    for i in 0..30 {
        term.print(format!("row{:02} some longer text here\r\n", i));
    }
    w.observe(&term, "filled");

    for (rows, cols) in [
        (8usize, 14usize),
        (8, 20),
        (6, 30),
        (10, 12),
        (8, 20),
        (8, 20),
    ] {
        term.resize(TerminalSize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
            dpi: 0,
        });
        w.observe(&term, &format!("resize {}x{}", rows, cols));
        for i in 0..4 {
            term.print(format!("after{}x{}-{}\r\n", rows, cols, i));
            w.observe(
                &term,
                &format!("write after resize {}x{} {}", rows, cols, i),
            );
        }
    }

    assert!(
        w.violations.is_empty(),
        "invariant violations:\n{}",
        w.violations.join("\n")
    );
}

/// Bottom-anchored scroll region (the common `CSI 2;N r` shape).
#[test]
fn stable_seqno_invariant_bottom_anchored_scroll_region() {
    let mut term = TestTerm::new(10, 20, 10);
    let mut w = Witness::new();
    for i in 0..25 {
        term.print(format!("row{:02}\r\n", i));
    }
    w.observe(&term, "filled");

    term.set_scroll_region(2, 9);
    for i in 0..12 {
        term.cup(0, 9);
        term.print(format!("new{:02}\n", i));
        w.observe(&term, &format!("region scroll {}", i));
    }

    assert!(
        w.violations.is_empty(),
        "invariant violations:\n{}",
        w.violations.join("\n")
    );
}

/// Erase-in-display followed by rewrites: many lines end up sharing one seqno.
#[test]
fn stable_seqno_invariant_ed_then_rewrite() {
    let mut term = TestTerm::new(8, 20, 10);
    let mut w = Witness::new();
    for round in 0..6 {
        term.erase_in_display(EraseInDisplay::EraseDisplay);
        w.observe(&term, &format!("ED {}", round));
        for i in 0..8 {
            term.cup(0, i as isize);
            term.print(format!("r{}c{}", round, i));
        }
        w.observe(&term, &format!("rewrite {}", round));
        term.cup(0, 7);
        term.print("\r\n");
        w.observe(&term, &format!("scroll {}", round));
    }
    assert!(
        w.violations.is_empty(),
        "invariant violations:\n{}",
        w.violations.join("\n")
    );
}
