//! Terminal resize and scrollback reflow behavior tests.
//! Split out from the parent test module; helpers come from `super::*`.
use super::*;

/// This test skips over an edge case with cursor positioning,
/// while sizing down, but tries to trip over the same edge
/// case while sizing back up again
#[test]
fn test_resize_2162_by_2_then_up_1() {
    let num_lines = 4;
    let num_cols = 20;

    let mut term = TestTerm::new(num_lines, num_cols, 0);
    term.print("some long long text");
    assert_visible_contents(
        &term,
        file!(),
        line!(),
        &["some long long text", "", "", ""],
    );
    term.assert_cursor_pos(19, 0, None, Some(0));
    term.resize(TerminalSize {
        rows: num_lines,
        cols: num_cols - 2,
        pixel_width: 0,
        pixel_height: 0,
        dpi: 0,
    });
    assert_visible_contents(
        &term,
        file!(),
        line!(),
        &["some long long tex", "t", "", ""],
    );
    eprintln!("check cursor pos 2");
    term.assert_cursor_pos(1, 1, None, Some(6));
    term.resize(TerminalSize {
        rows: num_lines - 1,
        cols: num_cols,
        pixel_width: 0,
        pixel_height: 0,
        dpi: 0,
    });
    assert_visible_contents(&term, file!(), line!(), &["some long long text", "", ""]);
    eprintln!("check cursor pos 3");
    term.assert_cursor_pos(19, 0, None, Some(7));
    term.resize(TerminalSize {
        rows: num_lines,
        cols: num_cols,
        pixel_width: 0,
        pixel_height: 0,
        dpi: 0,
    });
    assert_visible_contents(
        &term,
        file!(),
        line!(),
        &["some long long text", "", "", ""],
    );
    eprintln!("check cursor pos 3");
    term.assert_cursor_pos(19, 0, None, Some(8));
}

/// This test skips over an edge case with cursor positioning,
/// so it passes even ahead of a fix for issue 2162.
#[test]
fn test_resize_2162_by_2() {
    let num_lines = 4;
    let num_cols = 20;

    let mut term = TestTerm::new(num_lines, num_cols, 0);
    term.print("some long long text");
    assert_visible_contents(
        &term,
        file!(),
        line!(),
        &["some long long text", "", "", ""],
    );
    term.assert_cursor_pos(19, 0, None, Some(0));
    term.resize(TerminalSize {
        rows: num_lines,
        cols: num_cols - 2,
        pixel_width: 0,
        pixel_height: 0,
        dpi: 0,
    });
    assert_visible_contents(
        &term,
        file!(),
        line!(),
        &["some long long tex", "t", "", ""],
    );
    eprintln!("check cursor pos 2");
    term.assert_cursor_pos(1, 1, None, Some(6));
    term.resize(TerminalSize {
        rows: num_lines,
        cols: num_cols,
        pixel_width: 0,
        pixel_height: 0,
        dpi: 0,
    });
    assert_visible_contents(
        &term,
        file!(),
        line!(),
        &["some long long text", "", "", ""],
    );
    eprintln!("check cursor pos 3");
    term.assert_cursor_pos(19, 0, None, Some(7));
}

/// This case tickles an edge case where the cursor ends
/// up drifting away from where the line wraps and ends up
/// in the wrong place
#[test]
fn test_resize_2162() {
    let num_lines = 4;
    let num_cols = 20;

    let mut term = TestTerm::new(num_lines, num_cols, 0);
    term.print("some long long text");
    assert_visible_contents(
        &term,
        file!(),
        line!(),
        &["some long long text", "", "", ""],
    );
    term.assert_cursor_pos(19, 0, None, Some(0));
    term.resize(TerminalSize {
        rows: num_lines,
        cols: num_cols - 1,
        pixel_width: 0,
        pixel_height: 0,
        dpi: 0,
    });
    assert_visible_contents(
        &term,
        file!(),
        line!(),
        &["some long long text", "", "", ""],
    );
    eprintln!("check cursor pos 2");
    term.assert_cursor_pos(19, 0, None, Some(6));
    term.resize(TerminalSize {
        rows: num_lines,
        cols: num_cols,
        pixel_width: 0,
        pixel_height: 0,
        dpi: 0,
    });
    assert_visible_contents(
        &term,
        file!(),
        line!(),
        &["some long long text", "", "", ""],
    );
    eprintln!("check cursor pos 3");
    term.assert_cursor_pos(19, 0, None, Some(7));
}

/// Test the behavior of wrapped lines when we resize the terminal
/// wider and then narrower.
#[test]
fn test_resize_wrap() {
    const LINES: usize = 8;
    let mut term = TestTerm::new(LINES, 4, 0);
    term.print("111\r\n2222aa\r\n333\r\n");
    assert_visible_contents(
        &term,
        file!(),
        line!(),
        &["111", "2222", "aa", "333", "", "", "", ""],
    );
    term.resize(TerminalSize {
        rows: LINES,
        cols: 5,
        pixel_width: 0,
        pixel_height: 0,
        dpi: 0,
    });
    assert_visible_contents(
        &term,
        file!(),
        line!(),
        &["111", "2222a", "a", "333", "", "", "", ""],
    );
    term.resize(TerminalSize {
        rows: LINES,
        cols: 6,
        pixel_width: 0,
        pixel_height: 0,
        dpi: 0,
    });
    assert_visible_contents(
        &term,
        file!(),
        line!(),
        &["111", "2222aa", "333", "", "", "", "", ""],
    );
    term.resize(TerminalSize {
        rows: LINES,
        cols: 7,
        pixel_width: 0,
        pixel_height: 0,
        dpi: 0,
    });
    assert_visible_contents(
        &term,
        file!(),
        line!(),
        &["111", "2222aa", "333", "", "", "", "", ""],
    );
    term.resize(TerminalSize {
        rows: LINES,
        cols: 8,
        ..Default::default()
    });
    assert_visible_contents(
        &term,
        file!(),
        line!(),
        &["111", "2222aa", "333", "", "", "", "", ""],
    );

    // Resize smaller again
    term.resize(TerminalSize {
        rows: LINES,
        cols: 7,
        ..Default::default()
    });
    assert_visible_contents(
        &term,
        file!(),
        line!(),
        &["111", "2222aa", "333", "", "", "", "", ""],
    );
    term.resize(TerminalSize {
        rows: LINES,
        cols: 6,
        ..Default::default()
    });
    assert_visible_contents(
        &term,
        file!(),
        line!(),
        &["111", "2222aa", "333", "", "", "", "", ""],
    );
    term.resize(TerminalSize {
        rows: LINES,
        cols: 5,
        ..Default::default()
    });
    assert_visible_contents(
        &term,
        file!(),
        line!(),
        &["111", "2222a", "a", "333", "", "", "", ""],
    );
    term.resize(TerminalSize {
        rows: LINES,
        cols: 4,
        ..Default::default()
    });
    assert_visible_contents(
        &term,
        file!(),
        line!(),
        &["111", "2222", "aa", "333", "", "", "", ""],
    );
}

#[test]
fn test_resize_wrap_issue_971() {
    const LINES: usize = 4;
    let mut term = TestTerm::new(LINES, 4, 0);
    term.print("====\r\nSS\r\n");
    assert_visible_contents(&term, file!(), line!(), &["====", "SS", "", ""]);
    term.resize(TerminalSize {
        rows: LINES,
        cols: 6,
        ..Default::default()
    });
    assert_visible_contents(&term, file!(), line!(), &["====", "SS", "", ""]);
}

#[test]
fn test_resize_wrap_sgc_issue_978() {
    const LINES: usize = 4;
    let mut term = TestTerm::new(LINES, 4, 0);
    term.print("\u{1b}(0qqqq\u{1b}(B\r\nSS\r\n");
    assert_visible_contents(&term, file!(), line!(), &["────", "SS", "", ""]);
    term.resize(TerminalSize {
        rows: LINES,
        cols: 6,
        ..Default::default()
    });
    assert_visible_contents(&term, file!(), line!(), &["────", "SS", "", ""]);
}

#[test]
fn test_resize_wrap_dectcm_issue_978() {
    const LINES: usize = 4;
    let mut term = TestTerm::new(LINES, 4, 0);
    term.print("\u{1b}[?25l====\u{1b}[?25h\r\nSS\r\n");
    assert_visible_contents(&term, file!(), line!(), &["====", "SS", "", ""]);
    term.resize(TerminalSize {
        rows: LINES,
        cols: 6,
        ..Default::default()
    });
    assert_visible_contents(&term, file!(), line!(), &["====", "SS", "", ""]);
}

#[test]
fn test_resize_wrap_escape_code_issue_978() {
    const LINES: usize = 4;
    let mut term = TestTerm::new(LINES, 4, 0);
    term.print("====\u{1b}[0m\r\nSS\r\n");
    assert_visible_contents(&term, file!(), line!(), &["====", "SS", "", ""]);
    term.resize(TerminalSize {
        rows: LINES,
        cols: 6,
        ..Default::default()
    });
    assert_visible_contents(&term, file!(), line!(), &["====", "SS", "", ""]);
}

/// UP-35 / upstream issue #6623: resizing the *primary* screen (no
/// alt screen involved at all) after enough output has scrolled the
/// screen should leave the cursor on the actual last line of output,
/// not one line above it.
///
/// Repro: `seq 100` (maximized window, screen scrolls, prompt/cursor
/// ends up on the last visible row); "restore" the window to a
/// smaller size; the cursor must still be on the bottom row.
#[test]
fn test_resize_reflow_cursor_primary_screen_issue_6623() {
    let cols = 20;
    let mut term = TestTerm::new(10, cols, 1000);

    // Simulate `seq 100`: 100 numbered lines, each terminated with
    // CRLF, leaving the cursor on the (currently blank) line that
    // follows "100", in column 0 -- this is the bottom row of the
    // viewport since the screen has scrolled.
    for i in 1..=100 {
        term.print(format!("{}\r\n", i));
    }

    term.assert_cursor_pos(0, 9, Some("cursor on last (blank) row before resize"), None);

    // "Restore" the window to a smaller size (rows only; columns
    // unchanged, so no line-rewrap is triggered -- this isolates the
    // vertical cursor recomputation done by Screen::resize).
    term.resize(TerminalSize {
        rows: 5,
        cols,
        pixel_width: 0,
        pixel_height: 0,
        dpi: 0,
    });

    term.assert_cursor_pos(
        0,
        4,
        Some("cursor should stay on the bottom row after resize"),
        None,
    );
}

/// UP-35 / upstream issues #6669 and #5100: resizing the terminal
/// while the *alternate* screen is active (eg. while inside `nvim`)
/// must correctly reflow the *primary* screen's saved cursor (the one
/// that DECRC/exiting the alt screen will restore), rather than
/// leaving it pointing at a position computed for the old geometry.
///
/// This deliberately mirrors `test_resize_2162_by_2_then_up_1` (a
/// *non*-alt-screen resize/rewrap regression test), but performs the
/// resize while the alt screen is active instead. This is important:
/// a width change forces `Screen::resize` to rewrap long lines and
/// compute a genuinely new (x, y), landing on (1, 1) -- a position
/// that could not be produced by accident (eg. by simply clamping a
/// stale saved cursor to the new screen bounds). If the primary
/// screen's saved cursor is not reflowed/updated while the alt screen
/// is active, DECRC (triggered here by leaving the alt screen) will
/// restore a stale, unreflowed cursor position instead.
#[test]
fn test_resize_reflow_cursor_alt_screen_issue_6669_5100() {
    let num_lines = 4;
    let num_cols = 20;

    let mut term = TestTerm::new(num_lines, num_cols, 0);

    // Produce output on the primary screen, then note where the
    // cursor is, matching the setup of test_resize_2162_by_2_then_up_1.
    term.print("some long long text");
    assert_visible_contents(
        &term,
        file!(),
        line!(),
        &["some long long text", "", "", ""],
    );
    term.assert_cursor_pos(19, 0, None, Some(0));

    // Enter the alternate screen (eg. nvim starting up). This
    // implicitly does a DECSC, saving the primary screen's cursor
    // position (19, 0).
    term.set_mode("?1049", true);

    // Do something inside the alt screen; this must not affect the
    // primary screen's content or its saved cursor.
    term.print("editing in nvim");

    // Resize narrower while the alt screen is active. Because the
    // primary screen allows scrollback/rewrap (unlike the alt
    // screen), this must rewrap "some long long text" into
    // "some long long tex" / "t", and adjust the *saved* primary
    // cursor from (19, 0) to (1, 1) -- exactly as it would if this
    // resize had happened while the primary screen was active (see
    // test_resize_2162_by_2_then_up_1).
    term.resize(TerminalSize {
        rows: num_lines,
        cols: num_cols - 2,
        pixel_width: 0,
        pixel_height: 0,
        dpi: 0,
    });

    // Leave the alternate screen (eg. quitting nvim). This does a
    // DECRC, restoring the primary screen's cursor from the saved
    // position -- which must have been reflowed for the new geometry
    // by the preceding resize(), not left pointing at the stale (19, 0).
    term.set_mode("?1049", false);

    assert_visible_contents(
        &term,
        file!(),
        line!(),
        &["some long long tex", "t", "", ""],
    );
    term.assert_cursor_pos(
        1,
        1,
        Some("cursor restored via DECRC should be reflowed for the new size, not stale (19, 0) clamped"),
        None,
    );
}
