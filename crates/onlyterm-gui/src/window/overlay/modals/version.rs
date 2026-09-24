use crate::gui_api::guiwin::GuiWin;
use mux::termwiztermtab::TermWizTerminal;
use termwiz::cell::{AttributeChange, CellAttributes, Intensity};
use termwiz::color::ColorAttribute;
use termwiz::input::{InputEvent, KeyCode, KeyEvent, Modifiers, MouseButtons, MouseEvent};
use termwiz::surface::{Change, Position};
use termwiz::terminal::Terminal;
use window::WindowOps;

/// Centers `line` within a field of `width` columns, biasing any odd
/// leftover space to the right.
fn center(line: &str, width: usize) -> String {
    let len = line.chars().count();
    if len >= width {
        return line.to_string();
    }
    let total_pad = width - len;
    let left_pad = total_pad / 2;
    let right_pad = total_pad - left_pad;
    format!("{}{}{}", " ".repeat(left_pad), line, " ".repeat(right_pad))
}

/// Centers a whole block of `lines` together as a unit within `width`
/// columns: every line gets the *same* left margin (computed from the
/// widest line in the block), so lines of differing length -- like
/// "Version:", "Commit:", "Built:" with values of different width --
/// line up as a left-justified column instead of each drifting to its
/// own independent center (which is what plain per-line `center` gives
/// you, and looks visibly ragged for a label/value list).
fn center_block(lines: &[String], width: usize) -> Vec<String> {
    let block_width = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0);
    let left_pad = width.saturating_sub(block_width) / 2;
    lines
        .iter()
        .map(|l| format!("{}{}", " ".repeat(left_pad), l))
        .collect()
}

/// Shows a bordered, centered box with the running OnlyTerm version,
/// commit, and build time. This is the character-cell-grid equivalent of
/// the GDI "Loading" placeholder for a stack that has no per-cell
/// font-size control (see `overlay::debug` for the sibling plain-text
/// version/environment overlay this one visually replaces for the common
/// "what version am I running" question) -- bold text, a box border, and
/// generous letter-spacing on the title stand in for a literal large
/// font.
///
/// Does *not* copy anything to the clipboard on its own -- clobbering
/// whatever the user already had copied just because they glanced at the
/// version would be surprising. A clickable Copy button copies on demand
/// instead (via `gui_win.window`, since this closure runs on its own
/// spawned thread with no direct access to the `TermWindow` that owns
/// the real clipboard-setting code).
///
/// That button replaces an advertised "Press Ctrl+Shift+C to copy" that
/// could never have worked: SHIFT|CTRL C is a *global* default binding to
/// `CopyTo(Clipboard)`, so `TermWindow::process_key` resolves and consumes
/// it before the keypress is ever written to this pane's input stream --
/// the overlay's own match arm for it was unreachable, and the global
/// action it ran instead copies the (empty) selection, so pressing it did
/// nothing at all.
pub fn show_version_overlay(mut term: TermWizTerminal, gui_win: GuiWin) -> anyhow::Result<()> {
    // Mouse grab is deliberately left on (unlike the sibling `debug` and
    // `prompt` overlays, which call `no_grab_mouse_in_raw_mode`): without it
    // the pane never receives `InputEvent::Mouse` and the Copy button below
    // could not be clicked. The cost is that dragging no longer selects text
    // inside this overlay, which is precisely what the button makes
    // unnecessary here.
    term.set_raw_mode()?;
    term.render(&[Change::Title("OnlyTerm version".to_string())])?;

    let version = config::onlyterm_version();
    let commit_hash = config::onlyterm_commit_hash();
    let commit_count = config::onlyterm_commit_count();
    let build_time = config::onlyterm_build_time();

    let clipboard_text = format!(
        "OnlyTerm {version}\nCommit: #{commit_count} ({commit_hash})\nBuilt: {build_time}\n"
    );

    // Letter-spaced app name as a stand-in banner: there is no way to
    // request a larger font for a single cell-grid line in this overlay
    // type (see the module doc comment above), so extra spacing between
    // letters plus bold intensity is what gives the title visual weight.
    let spaced_title = "OnlyTerm"
        .chars()
        .map(|c| c.to_string())
        .collect::<Vec<_>>()
        .join(" ");

    // Right-align the labels themselves (not just the values after them)
    // so "Version:", "Commit:", and "Built:" -- three different lengths
    // -- all end their colon on the same column, giving a clean vertical
    // seam down the middle of the box instead of a ragged left edge.
    let labels = ["Version:", "Commit:", "Built:"];
    let label_width = labels.iter().map(|l| l.chars().count()).max().unwrap_or(0);
    let info_raw_lines = [
        format!("{:>label_width$} {version}", labels[0]),
        format!(
            "{:>label_width$} #{commit_count} ({commit_hash})",
            labels[1]
        ),
        format!("{:>label_width$} {build_time}", labels[2]),
    ];
    let info_width = info_raw_lines
        .iter()
        .map(|l| l.chars().count())
        .max()
        .unwrap_or(0);

    let inner_width = spaced_title.chars().count().max(info_width) + 4; // 2 columns of padding on each side

    let info_lines = center_block(&info_raw_lines, inner_width);

    let mut box_lines = Vec::with_capacity(info_lines.len() + 4);
    box_lines.push(format!("┌{}┐", "─".repeat(inner_width)));
    box_lines.push(format!("│{}│", center(&spaced_title, inner_width)));
    box_lines.push(format!("│{}│", " ".repeat(inner_width)));
    for line in &info_lines {
        box_lines.push(format!("│{}│", center(line, inner_width)));
    }
    box_lines.push(format!("└{}┘", "─".repeat(inner_width)));

    let (screen_cols, screen_rows) = {
        let size = term.get_screen_size()?;
        (size.cols, size.rows)
    };

    let total_display_lines = box_lines.len() + 3 /* blank + button + hint */;
    let top_pad = screen_rows.saturating_sub(total_display_lines) / 2;

    let mut boxed_text = "\r\n".repeat(top_pad);
    for line in &box_lines {
        boxed_text.push_str(&center(line, screen_cols));
        boxed_text.push_str("\r\n");
    }
    boxed_text.push_str("\r\n");

    // The button's screen position has to be known outside the render
    // closure too, since hit-testing a click is exactly "is this cell inside
    // the button". `boxed_text` above ends by emitting the blank separator
    // row, which leaves the next free row one past the box.
    const BUTTON_LABEL: &str = "[ Copy ]";
    let button_w = BUTTON_LABEL.chars().count();
    let button_x = screen_cols.saturating_sub(button_w) / 2;
    let button_row = top_pad + box_lines.len() + 1;

    let render = |term: &mut TermWizTerminal, copied: bool, hovered: bool| -> anyhow::Result<()> {
        let hint_line = if copied {
            "Copied to clipboard  ·  Press Esc to close"
        } else {
            "Click Copy, or press Enter  ·  Press Esc to close"
        };

        let mut changes = vec![
            Change::ClearScreen(ColorAttribute::Default),
            AttributeChange::Intensity(Intensity::Bold).into(),
            Change::Text(boxed_text.clone()),
            Change::AllAttributes(CellAttributes::default()),
            Change::CursorPosition {
                x: Position::Absolute(button_x),
                y: Position::Absolute(button_row),
            },
        ];
        if hovered {
            changes.push(AttributeChange::Reverse(true).into());
        }
        changes.push(Change::Text(BUTTON_LABEL.to_string()));
        changes.push(Change::AllAttributes(CellAttributes::default()));

        changes.push(Change::CursorPosition {
            x: Position::Absolute(0),
            y: Position::Absolute(button_row + 1),
        });
        changes.push(AttributeChange::Intensity(Intensity::Half).into());
        changes.push(Change::Text(center(hint_line, screen_cols)));
        changes.push(Change::AllAttributes(CellAttributes::default()));

        term.render(&changes)?;
        Ok(())
    };

    let mut copied = false;
    let mut hovered = false;
    render(&mut term, copied, hovered)?;

    let copy = |term: &mut TermWizTerminal, copied: &mut bool, hovered: bool| {
        gui_win
            .window
            .set_clipboard(::window::Clipboard::Clipboard, clipboard_text.clone());
        *copied = true;
        render(term, *copied, hovered)
    };

    loop {
        match term.poll_input(None)? {
            Some(InputEvent::Key(KeyEvent {
                key: KeyCode::Escape,
                ..
            })) => return Ok(()),
            // Enter and Space activate the button, matching how a focused
            // button behaves anywhere else. Neither is claimed by a global
            // key assignment, so unlike the Ctrl+Shift+C this replaces (see
            // the function doc comment), both actually reach this overlay.
            Some(InputEvent::Key(KeyEvent {
                key: KeyCode::Enter | KeyCode::Char('\r') | KeyCode::Char(' '),
                ..
            })) => copy(&mut term, &mut copied, hovered)?,
            Some(InputEvent::Key(KeyEvent {
                key: KeyCode::Char('c') | KeyCode::Char('d'),
                modifiers,
                ..
            })) if modifiers.contains(Modifiers::CTRL) => return Ok(()),
            Some(InputEvent::Mouse(MouseEvent {
                x,
                y,
                mouse_buttons,
                ..
            })) => {
                let x = x as usize;
                let y = y as usize;
                let now_hovered = y == button_row && x >= button_x && x < button_x + button_w;
                let hover_changed = now_hovered != hovered;
                hovered = now_hovered;

                if hovered && mouse_buttons == MouseButtons::LEFT {
                    copy(&mut term, &mut copied, hovered)?;
                } else if hover_changed {
                    // Only repaint when the highlight actually changes:
                    // grabbing the mouse delivers an event for every cell of
                    // motion anywhere in the pane, and a repaint clears and
                    // redraws the entire screen.
                    render(&mut term, copied, hovered)?;
                }
            }
            None => return Ok(()),
            _ => {}
        }
    }
}
