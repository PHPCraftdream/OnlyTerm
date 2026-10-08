use crate::gui_api::guiwin::GuiWin;
use crate::overlay::menu_style::TextMenuStyle;
use onlyterm_config::keyassignment::{Confirmation, KeyAssignment};
use onlyterm_mux::termwiztermtab::TermWizTerminal;
use onlyterm_mux_funcs::MuxPane;
use termwiz::input::{InputEvent, KeyCode, KeyEvent, MouseButtons, MouseEvent};
use termwiz::surface::{Change, CursorVisibility, Position};
use termwiz::terminal::Terminal;

fn run_confirmation_impl(message: &str, term: &mut TermWizTerminal) -> anyhow::Result<bool> {
    term.set_raw_mode()?;
    let size = term.get_screen_size()?;
    let style = TextMenuStyle::new();
    let text_width = size.cols.saturating_sub(8).max(1);
    let x_pos = 3;
    let wrapped = textwrap::fill(message, text_width);
    let message_rows = wrapped.lines().count();
    let top_row = size.rows.saturating_sub(message_rows + 4) / 2;
    let button_row = (top_row + message_rows + 1).min(size.rows.saturating_sub(3));
    #[derive(Copy, Clone, PartialEq, Eq)]
    enum ActiveButton {
        Yes,
        No,
    }
    let mut active = ActiveButton::No;
    let yes_x = x_pos;
    let yes_w = 7;
    let no_x = yes_x + yes_w + 3;
    let no_w = 6;
    #[allow(clippy::result_large_err)]
    let render = |term: &mut TermWizTerminal, active: ActiveButton| -> termwiz::Result<()> {
        let mut changes = style.frame(
            size.cols,
            size.rows,
            "Confirmation",
            "",
            "Arrows / Tab: select  Enter: accept  Y / N  Esc: cancel",
        );
        changes.push(Change::CursorVisibility(CursorVisibility::Hidden));
        for (y, row) in wrapped.lines().enumerate() {
            changes.extend([
                Change::CursorPosition {
                    x: Position::Absolute(x_pos),
                    y: Position::Absolute(top_row + y),
                },
                Change::AllAttributes(style.normal.clone()),
                Change::Text(row.trim_end().to_string()),
            ]);
        }
        for (x, label, selected) in [
            (yes_x, " [Y]es ", active == ActiveButton::Yes),
            (no_x, " [N]o ", active == ActiveButton::No),
        ] {
            changes.extend([
                Change::CursorPosition {
                    x: Position::Absolute(x),
                    y: Position::Absolute(button_row),
                },
                Change::AllAttributes(if selected {
                    style.selected.clone()
                } else {
                    style.label.clone()
                }),
                Change::Text(label.into()),
            ]);
        }
        term.render(&changes)?;
        term.flush()
    };
    render(term, active)?;
    while let Ok(Some(event)) = term.poll_input(None) {
        match event {
            InputEvent::Key(KeyEvent {
                key: KeyCode::Char('y' | 'Y'),
                ..
            }) => return Ok(true),
            InputEvent::Key(KeyEvent {
                key: KeyCode::Char('n' | 'N') | KeyCode::Escape,
                ..
            }) => return Ok(false),
            InputEvent::Key(KeyEvent {
                key: KeyCode::Enter | KeyCode::Char('\r'),
                ..
            }) => return Ok(active == ActiveButton::Yes),
            InputEvent::Key(KeyEvent {
                key:
                    KeyCode::UpArrow
                    | KeyCode::DownArrow
                    | KeyCode::LeftArrow
                    | KeyCode::RightArrow
                    | KeyCode::Tab,
                ..
            }) => {
                active = if active == ActiveButton::Yes {
                    ActiveButton::No
                } else {
                    ActiveButton::Yes
                };
            }
            InputEvent::Mouse(MouseEvent {
                x,
                y,
                mouse_buttons,
                ..
            }) => {
                let (x, y) = (x as usize, y as usize);
                if y == button_row && x >= yes_x && x < yes_x + yes_w {
                    active = ActiveButton::Yes;
                    if mouse_buttons == MouseButtons::LEFT {
                        return Ok(true);
                    }
                } else if y == button_row && x >= no_x && x < no_x + no_w {
                    active = ActiveButton::No;
                    if mouse_buttons == MouseButtons::LEFT {
                        return Ok(false);
                    }
                } else if mouse_buttons != MouseButtons::NONE {
                    return Ok(false);
                }
            }
            _ => {}
        }
        render(term, active)?;
    }
    Ok(false)
}

/// Shows the confirmation overlay and waits for the user to answer.
///
/// `args.action`/`args.cancel` used to be `EmitEvent` names dispatched to a
/// rhai handler registered via `onlyterm.action_callback`; with the scripting
/// layer removed there is no handler registry left to receive the answer, so
/// the result is simply discarded once the overlay resolves. The `EmitEvent`
/// shape is still validated here (rather than accepting any `KeyAssignment`)
/// so that a config which still uses the old `Confirmation { action, cancel }`
/// shape fails the same way it used to instead of silently doing something
/// unexpected.
pub fn show_confirmation_overlay(
    mut term: TermWizTerminal,
    args: Confirmation,
    _window: GuiWin,
    _pane: MuxPane,
) -> anyhow::Result<()> {
    match *args.action {
        KeyAssignment::EmitEvent(_) => {}
        _ => {
            anyhow::bail!("Confirmation requires action to be defined by onlyterm.action_callback")
        }
    };

    // The confirm/cancel result no longer has anywhere to go (no rhai handler
    // registry exists to receive it), so just run the prompt to completion and
    // drop the answer.
    let _ = run_confirmation_impl(&args.message, &mut term);
    Ok(())
}
