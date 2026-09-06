//! Terminal resize and scrollback reflow behavior tests.
//! Split out from the parent test module; helpers come from `super::*`.
use super::*;

#[test]
fn conpty_resize_does_not_create_blank_only_scrollback() {
    let mut term = TestTerm::new(24, 80, 1000);
    term.enable_conpty_quirks();
    term.print("\r\nprompt> ");
    let stable_cursor = term.screen().visible_row_to_stable_row(term.cursor_pos().y);
    for rows in [23, 18, 30, 24] {
        term.resize(TerminalSize {
            rows,
            cols: 80,
            ..Default::default()
        });
        std::assert_eq!(term.screen().scrollback_rows(), rows);
        std::assert_eq!(term.cursor_pos().y, 0);
        std::assert_eq!(
            term.screen().visible_row_to_stable_row(term.cursor_pos().y),
            stable_cursor
        );
        std::assert_eq!(term.screen().visible_lines()[0].as_str(), "prompt> ");
    }
    term.print("input");
    std::assert_eq!(term.screen().visible_lines()[0].as_str(), "prompt> input");
}

#[test]
fn conpty_resize_preserves_text_and_visible_blank_row_decoration() {
    for prefix in [
        "history",
        "\x1b[41m   \x1b[0m",
        "\x1b[4m   \x1b[0m",
        "\x1b]8;;https://example.invalid/\x1b\\ \x1b]8;;\x1b\\",
    ] {
        let mut term = TestTerm::new(24, 80, 1000);
        term.enable_conpty_quirks();
        term.print(format!("{}\r\nprompt> ", prefix));
        let original = term.screen().all_lines()[0].clone();
        term.resize(TerminalSize {
            rows: 23,
            cols: 80,
            ..Default::default()
        });
        std::assert_eq!(term.screen().scrollback_rows(), 24);
        std::assert_eq!(term.screen().all_lines()[0], original);
        std::assert_eq!(term.cursor_pos().y, 0);
    }
}

#[test]
fn conpty_resize_uses_active_palette_for_blank_background() {
    for palette_override in [false, true] {
        let mut term = TestTerm::new(24, 80, 1000);
        term.enable_conpty_quirks();
        let white = term.palette().colors.0[15];
        let red = term.palette().colors.0[1];
        term.palette_mut().background = white;
        if palette_override {
            term.palette_mut().colors.0[15] = red;
        }
        term.print("\x1b[107m   \x1b[0m\r\nprompt> ");
        term.resize(TerminalSize {
            rows: 23,
            cols: 80,
            ..Default::default()
        });
        std::assert_eq!(
            term.screen().scrollback_rows(),
            if palette_override { 24 } else { 23 }
        );
        std::assert_eq!(term.cursor_pos().y, 0);
    }
}

#[test]
fn conpty_resize_does_not_discard_preexisting_blank_history() {
    let mut term = TestTerm::new(24, 80, 1000);
    term.enable_conpty_quirks();
    for _ in 0..25 {
        term.print("\r\n");
    }
    term.print("prompt> ");
    let before_top = term.screen().phys_to_stable_row_index(0);
    let before_rows = term.screen().scrollback_rows();
    std::assert!(before_rows > 24);
    term.resize(TerminalSize {
        rows: 23,
        cols: 80,
        ..Default::default()
    });
    std::assert_eq!(term.screen().phys_to_stable_row_index(0), before_top);
    std::assert_eq!(term.screen().scrollback_rows(), before_rows);
}

#[test]
fn conpty_shrink_moves_prompt_over_leading_blank_like_native_console() {
    let mut term = TestTerm::new(24, 80, 1000);
    term.enable_conpty_quirks();
    term.print("\r\nprompt> ");
    for rows in [23, 22, 18] {
        term.resize(TerminalSize {
            rows,
            cols: 80,
            ..Default::default()
        });
        std::assert_eq!(term.cursor_pos().y, 0);
        std::assert_eq!(term.screen().visible_lines()[0].as_str(), "prompt> ");
    }
    // ConPTY sends only the edit and its absolute position, not the prompt.
    term.print("\x1b[1;9Hinput");
    std::assert_eq!(term.screen().visible_lines()[0].as_str(), "prompt> input");
}

#[test]
fn conpty_shrink_keeps_cursor_attached_to_its_visible_text() {
    for cols in [145, 100] {
        let mut term = TestTerm::new(53, 145, 1000);
        term.enable_conpty_quirks();
        for row in 0..53 {
            term.cup(0, row);
            term.print(format!("row {}", row));
        }
        term.cup(0, 13);
        term.print("prompt> input");
        term.resize(TerminalSize {
            rows: 40,
            cols,
            ..Default::default()
        });
        let visible = term.screen().visible_lines();
        let prompt_row = visible
            .iter()
            .position(|line| line.as_str().starts_with("prompt> input"))
            .unwrap();
        std::assert_eq!(prompt_row, 0);
        std::assert_eq!(term.cursor_pos().y as usize, prompt_row);
        term.print("X");
        std::assert_eq!(
            term.screen().visible_lines()[prompt_row].as_str(),
            "prompt> inputX"
        );
    }
}

/// ConPTY preserves the existing visible cursor row when a window grows.
/// This mirrors the common PSReadLine redraw sequence after a resize: clear,
/// home, then rewrite the prompt/input. Covers the reported 40-to-53-row
/// geometry; this is not yet a reproducer of the observed mismatch.
#[test]
fn test_conpty_resize_grow_then_psreadline_redraw_keeps_cursor_row() {
    let mut term = TestTerm::new(40, 120, 1000);
    term.enable_conpty_quirks();
    for row in 0..80 {
        term.print(format!("history {}\r\n", row));
    }
    term.cup(0, 39);
    term.print("prompt> input");
    std::assert_eq!((term.cursor_pos().x, term.cursor_pos().y), (13, 39));

    term.resize(TerminalSize {
        rows: 53,
        cols: 120,
        ..Default::default()
    });
    std::assert_eq!((term.cursor_pos().x, term.cursor_pos().y), (13, 39));

    term.print("\x1b[2J\x1b[Hprompt>\r\ninput");
    std::assert_eq!((term.cursor_pos().x, term.cursor_pos().y), (5, 1));
    let visible = term.screen().visible_lines();
    std::assert_eq!(visible.len(), 53);
    std::assert_eq!(visible[0].as_str().trim_end(), "prompt>");
    std::assert_eq!(visible[1].as_str().trim_end(), "input");
}

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

/// A cursor resting in column 0 of a line that is *not* the continuation
/// of a wrapped line must stay exactly where it is when the terminal is
/// widened.
///
/// `rewrap_lines` has a special case for a cursor that reflow places in
/// column 0: a terminal leaves the cursor parked in the last column of a
/// full line (with the wrap pending) rather than moving it to column 0 of
/// the next row, so a logical cursor offset that is an exact multiple of
/// the new width has to be pushed back onto the end of the previous row.
/// The condition that selects that case only asked whether the cursor
/// landed in column 0 of a row other than the first -- which is also true
/// for the utterly ordinary cursor position produced by a plain newline.
/// Every widening resize therefore yanked such a cursor up onto the end
/// of the previous row, and the next character the program printed landed
/// at the far right of the line above instead of at the start of its own.
///
/// This is what garbled a `--start-conf` tab: its startup commands are
/// typed in immediately, so the shell is mid-output when the window
/// maximizes and the resize lands right on a just-emitted newline.
#[test]
fn test_resize_wider_keeps_a_column_zero_cursor_on_its_own_row() {
    let mut term = TestTerm::new(4, 10, 0);

    // A completed line plus a newline: the cursor is legitimately at
    // column 0 of row 1, and row 0 is not a wrapped line.
    term.print("hello\r\n");
    term.assert_cursor_pos(0, 1, Some("plain newline puts the cursor on row 1"), None);

    term.resize(TerminalSize {
        rows: 4,
        cols: 20,
        ..Default::default()
    });

    term.assert_cursor_pos(
        0,
        1,
        Some("widening must not move a column-zero cursor onto the previous row"),
        None,
    );

    // ...and printing must continue on that row, not at the tail of the
    // one above it.
    term.print("X");
    assert_visible_contents(&term, file!(), line!(), &["hello", "X", "", ""]);
}

/// The flip side of the case above: a cursor that reflow *does* place in
/// column 0 because its logical line wrapped exactly at the new width
/// still has to be pushed back onto the end of the previous row, which is
/// where a terminal really parks it. `test_resize_2162` covers the same
/// invariant from the outside; this states it directly so that narrowing
/// the special case can't silently remove it.
#[test]
fn test_resize_keeps_a_wrapped_cursor_at_the_end_of_the_previous_row() {
    let mut term = TestTerm::new(4, 20, 0);
    term.print("some long long text");
    term.assert_cursor_pos(19, 0, None, Some(0));

    term.resize(TerminalSize {
        rows: 4,
        cols: 19,
        ..Default::default()
    });

    term.assert_cursor_pos(
        19,
        0,
        Some("a cursor parked at the wrap point belongs on the previous row"),
        None,
    );
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
