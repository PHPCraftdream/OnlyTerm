use crate::gui_api::guiwin::GuiWin;
use mux::termwiztermtab::TermWizTerminal;
use termwiz::cell::{AttributeChange, CellAttributes, Intensity};
use termwiz::color::ColorAttribute;
use termwiz::input::{InputEvent, KeyCode, KeyEvent, Modifiers};
use termwiz::surface::Change;
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
/// version would be surprising. Pressing Ctrl+Shift+C copies on demand
/// instead (via `gui_win.window`, since this closure runs on its own
/// spawned thread with no direct access to the `TermWindow` that owns
/// the real clipboard-setting code).
pub fn show_version_overlay(mut term: TermWizTerminal, gui_win: GuiWin) -> anyhow::Result<()> {
    term.no_grab_mouse_in_raw_mode();
    term.set_raw_mode()?;
    term.render(&[Change::Title("OnlyTerm version".to_string())])?;

    let version = config::wezterm_version();
    let commit_hash = config::wezterm_commit_hash();
    let commit_count = config::wezterm_commit_count();
    let build_time = config::wezterm_build_time();

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

    let total_display_lines = box_lines.len() + 2 /* blank + hint */;
    let top_pad = screen_rows.saturating_sub(total_display_lines) / 2;

    let mut boxed_text = "\r\n".repeat(top_pad);
    for line in &box_lines {
        boxed_text.push_str(&center(line, screen_cols));
        boxed_text.push_str("\r\n");
    }
    boxed_text.push_str("\r\n");

    let render = |term: &mut TermWizTerminal, copied: bool| -> anyhow::Result<()> {
        let hint_line = if copied {
            "Copied to clipboard  ·  Press Esc to close"
        } else {
            "Press Ctrl+Shift+C to copy  ·  Press Esc to close"
        };
        let hint_text = format!("{}\r\n", center(hint_line, screen_cols));

        term.render(&[
            Change::ClearScreen(ColorAttribute::Default),
            AttributeChange::Intensity(Intensity::Bold).into(),
            Change::Text(boxed_text.clone()),
            Change::AllAttributes(CellAttributes::default()),
            AttributeChange::Intensity(Intensity::Half).into(),
            Change::Text(hint_text),
            Change::AllAttributes(CellAttributes::default()),
        ])?;
        Ok(())
    };

    render(&mut term, false)?;

    loop {
        match term.poll_input(None)? {
            Some(InputEvent::Key(KeyEvent {
                key: KeyCode::Escape,
                ..
            })) => return Ok(()),
            // Ctrl+Shift+C for copy, not bare `c`: a bare unmodified key is
            // layout-dependent by design (it's meant to be literal text
            // input), so on a non-US layout the physical C key wouldn't
            // even produce the char 'c'. Chords that include CTRL (or ALT)
            // instead resolve through this fork's physical-key-preference
            // normalization (crates/window/src/os/windows/window.rs), so
            // this works the same regardless of the active layout -- same
            // fix class as the Alt+V passthrough fix earlier this session.
            // Checked before the bare Ctrl+C/Ctrl+D close arm below so the
            // Shift-qualified chord doesn't fall through to it.
            //
            // Deliberately NOT checking `modifiers.contains(Modifiers::SHIFT)`
            // here: `window.rs`'s `normalize_shift()` always folds a
            // Shift+letter chord into the uppercase letter with the
            // *general* SHIFT bit removed (only positional LEFT_SHIFT/
            // RIGHT_SHIFT survive that step), and `TermWizTerminalPane::
            // key_down` (crates/mux/src/termwiztermtab.rs) strips those
            // positional bits too before this overlay ever sees the event.
            // So a real Ctrl+Shift+C physically never arrives here with the
            // general SHIFT bit set -- requiring it made this arm
            // unreachable. `InputMap::lookup_key` already accounts for the
            // same fold when matching ordinary keybindings; matching on the
            // uppercased char alone is the same convention applied here.
            Some(InputEvent::Key(KeyEvent {
                key: KeyCode::Char('C'),
                modifiers,
                ..
            })) if modifiers.contains(Modifiers::CTRL) => {
                gui_win
                    .window
                    .set_clipboard(::window::Clipboard::Clipboard, clipboard_text.clone());
                render(&mut term, true)?;
            }
            Some(InputEvent::Key(KeyEvent {
                key: KeyCode::Char('c') | KeyCode::Char('d'),
                modifiers,
                ..
            })) if modifiers.contains(Modifiers::CTRL) => return Ok(()),
            None => return Ok(()),
            _ => {}
        }
    }
}
