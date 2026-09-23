//! Native ConPTY startup/resize observations, not a row-zero prompt policy.
use super::*;

#[test]
fn native_cmd_color_startup_and_resize_keep_prompt_attached() {
    // Oracle: conpty.dll 1.22.2502.04002, CMD /d /q /k with chcp + color f8.
    // Native CONOUT$ starts at (6, 1), then moves to (9, 0) after shrink + xyz.
    // See docs/investigations/2026-09-06-native-prompt-row.md.
    for rows in [24, 53] {
        for fragmented in [false, true] {
            let mut term = TestTerm::new(rows, 80, 1000);
            term.enable_conpty_quirks();
            // Normalize only title/feature announcements; keep the full-screen
            // color fill and DECSC/DECRC from the native capture.
            let mut startup = String::from("\x1b7");
            for row in 1..=rows {
                startup.push_str(&format!("\x1b[{};1H\x1b[0;90;107m{}", row, " ".repeat(80)));
            }
            startup.push_str("\x1b8\x1b[0;90;107m\r\nPROBE>");
            if fragmented {
                for byte in startup.as_bytes() {
                    term.print([*byte]);
                }
            } else {
                term.print(&startup);
            }
            std::assert_eq!((term.cursor_pos().x, term.cursor_pos().y), (6, 1));
            std::assert_eq!(
                term.screen().visible_lines()[1].as_str().trim_end(),
                "PROBE>"
            );
            term.resize(TerminalSize {
                rows: rows - 1,
                cols: 80,
                ..Default::default()
            });
            // Native output does not repaint the prompt after this resize.
            term.print("\x1b[1;7Hxyz\x1b[1;10H");
            std::assert_eq!((term.cursor_pos().x, term.cursor_pos().y), (9, 0));
            std::assert_eq!(
                term.screen().visible_lines()[0].as_str().trim_end(),
                "PROBE>xyz"
            );
        }
    }
}
