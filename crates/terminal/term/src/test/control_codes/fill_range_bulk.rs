//! Regression tests for the bulk rewrite of `Line::fill_range` (the erase
//! primitive behind EL/ED/ECH). Complements the exhaustive differential
//! test in `onlyterm-surface` with realistic, end-to-end terminal
//! scenarios: wide characters straddling an erase boundary, Background
//! Color Erase (BCE, i.e. a non-default erase attribute must not
//! truncate the line), and OSC 8 hyperlinks inside the erased region.
use super::*;
use k9::assert_equal as assert_eq;

/// A wide character immediately to the left of an EL0 (erase-to-end)
/// boundary must be nerfed (blanked) rather than left half-erased; real
/// content further to the left must survive untouched.
#[test]
fn el0_nerfs_wide_char_at_left_boundary() {
    let mut term = TestTerm::new(1, 8, 0);
    // A=col0 B=col1 日=col2-3 本=col4-5 C=col6 D=col7
    term.print("AB日本CD");
    assert_visible_contents(&term, file!(), line!(), &["AB日本CD"]);

    // Cursor lands on the continuation column of 本 (col5); EL0 erases
    // [5, 8). The nerf must blank 本's leading column (col4) too, since
    // it can no longer render correctly with only half of itself left.
    term.cup(5, 0);
    term.erase_in_line(EraseInLine::EraseToEndOfLine);
    assert_visible_contents(&term, file!(), line!(), &["AB日"]);
}

/// Same as above, but for ED (erase-to-end-of-display): the current row
/// is filled via the same `fill_range` primitive.
#[test]
fn ed0_nerfs_wide_char_at_left_boundary() {
    let mut term = TestTerm::new(2, 8, 0);
    term.print("AB日本CD\r\nxyz");
    term.cup(5, 0);
    term.erase_in_display(EraseInDisplay::EraseToEndOfDisplay);
    // Row 1 ("xyz") is erased in full, from column 0: this touches an
    // in-bounds index of the pre-existing content, so -- like the
    // `fill_range` differential test's "all-default-blank interior
    // touch" case -- it reproduces the legacy quirk of staying at the
    // full erased width instead of collapsing to an empty line.
    assert_visible_contents(&term, file!(), line!(), &["AB日", "        "]);
}

/// ECH straddling a wide character on *both* edges: the character to the
/// left of the erased range is nerfed, and the character whose leading
/// column falls inside the erased range is removed outright, without
/// corrupting the (already-blank) placeholder column after it.
#[test]
fn ech_wide_chars_at_both_boundaries() {
    let mut term = TestTerm::new(1, 8, 0);
    // A=col0 日=col1-2 本=col3-4 B=col5 C=col6 D=col7
    term.print("A日本BCD");
    assert_visible_contents(&term, file!(), line!(), &["A日本BCD"]);

    // Cursor on 日's continuation column (col2); erase 2 cells: [2, 4).
    // That covers 日's continuation (left edge, must nerf col1) and
    // 本's leading column (right edge, its own continuation at col4 is
    // simply left as the blank placeholder it already was).
    term.cup(2, 0);
    term.print("\x1b[2X");
    assert_visible_contents(&term, file!(), line!(), &["A    BCD"]);
}

/// Background Color Erase: when the erase attribute has a non-default
/// background, the erased region must be painted with that background
/// and must *not* be truncated away, even though it's otherwise blank.
/// A wide character nerfed at the erase boundary keeps its own (older)
/// attributes rather than picking up the new erase color.
#[test]
fn el0_bce_paints_background_without_truncating() {
    let mut term = TestTerm::new(1, 8, 0);
    term.print("AB日本CD");
    term.cup(5, 0);
    // Set background to blue, then erase to end of line.
    term.print("\x1b[44m");
    term.erase_in_line(EraseInLine::EraseToEndOfLine);

    let line = &term.screen().visible_lines()[0];
    assert_eq!(line.len(), 8, "BCE must not truncate the erased region");
    assert_eq!(line.as_str(), "AB日    ");

    let bg = color::AnsiColor::Navy;
    let cells: Vec<_> = line.visible_cells().collect();
    // col4 is the nerfed remnant of 本 -- it keeps 本's original
    // (default) attributes, not the new erase background.
    assert_eq!(cells[3].attrs().background(), Default::default());
    // The freshly erased columns (5, 6, 7) get the new background.
    for cell in &cells[4..] {
        assert_eq!(
            cell.attrs().background(),
            bg.into(),
            "erased cell must carry the BCE background"
        );
    }
}

/// Erasing a region that contains an OSC 8 hyperlink must leave no
/// visible cell with that hyperlink attribute. (Note: `Line::has_hyperlink`
/// itself is a sticky "this line has ever had one" bit that no mutator --
/// old or new -- ever clears, so it deliberately isn't part of this
/// assertion; the real observable is the per-cell attribute.)
#[test]
fn el2_erases_hyperlinked_cells() {
    let mut term = TestTerm::new(1, 10, 0);
    let link = Arc::new(Hyperlink::new("http://example.com"));
    term.hyperlink(&link);
    term.print("link text");
    term.hyperlink_off();

    assert!(term.screen().visible_lines()[0].has_hyperlink());
    assert!(term.screen().visible_lines()[0]
        .visible_cells()
        .any(|c| c.attrs().hyperlink().is_some()));

    term.cup(0, 0);
    term.erase_in_line(EraseInLine::EraseLine);

    assert!(
        term.screen().visible_lines()[0]
            .visible_cells()
            .all(|c| c.attrs().hyperlink().is_none()),
        "erasing the entire line must leave no cell carrying the hyperlink"
    );
}

/// DECALN (`ESC # 8`) fills every cell of every row with 'E' -- this is
/// the same `fill_range` primitive with a non-blank fill cell reaching
/// the full width, exercising the append/pad path of the clustered fast
/// path on an empty line.
#[test]
fn decaln_fills_full_screen_with_e() {
    let mut term = TestTerm::new(2, 4, 0);
    term.print("\x1b#8");
    assert_visible_contents(&term, file!(), line!(), &["EEEE", "EEEE"]);
}
