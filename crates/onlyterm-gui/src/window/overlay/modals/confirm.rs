use crate::gui_api::guiwin::GuiWin;
use config::keyassignment::{Confirmation, KeyAssignment};
use mux::termwiztermtab::TermWizTerminal;
use mux_funcs::MuxPane;
use termwiz::cell::AttributeChange;
use termwiz::color::ColorAttribute;
use termwiz::input::{InputEvent, KeyCode, KeyEvent, MouseButtons, MouseEvent};
use termwiz::surface::{Change, CursorVisibility, Position};
use termwiz::terminal::Terminal;

fn run_confirmation_impl(message: &str, term: &mut TermWizTerminal) -> anyhow::Result<bool> {
    term.set_raw_mode()?;

    let size = term.get_screen_size()?;

    // Render 80% wide, centered
    let text_width = size.cols * 80 / 100;
    let x_pos = size.cols * 10 / 100;

    // Fit text to the width
    let wrapped = textwrap::fill(message, text_width);

    let message_rows = wrapped.split("\n").count();
    // Now we want to vertically center the prompt in the view.
    // After the prompt there will be a blank line and then the "buttons",
    // so we add two to the number of rows.
    let top_row = (size.rows - (message_rows + 2)) / 2;

    let button_row = top_row + message_rows + 1;
    let mut active = ActiveButton::None;

    let yes_x = x_pos;
    let yes_w = 7;

    let no_x =  yes_x + yes_w + 8 /* spacer */;
    let no_w = 6;

    #[derive(Copy, Clone, PartialEq, Eq)]
    enum ActiveButton {
        None,
        Yes,
        No,
    }

    // Closure returns termwiz::Result; Err (termwiz::Error) is an external
    // 136-byte type, and boxing it would change the closure's type.
    #[allow(clippy::result_large_err)]
    let render = |term: &mut TermWizTerminal, active: ActiveButton| -> termwiz::Result<()> {
        let mut changes = vec![
            Change::ClearScreen(ColorAttribute::Default),
            Change::CursorVisibility(CursorVisibility::Hidden),
        ];

        for (y, row) in wrapped.split("\n").enumerate() {
            let row = row.trim_end();
            changes.push(Change::CursorPosition {
                x: Position::Absolute(x_pos),
                y: Position::Absolute(top_row + y),
            });
            changes.push(Change::Text(row.to_string()));
        }

        changes.push(Change::CursorPosition {
            x: Position::Absolute(x_pos),
            y: Position::Absolute(button_row),
        });

        if active == ActiveButton::Yes {
            changes.push(AttributeChange::Reverse(true).into());
        }
        changes.push(" [Y]es ".into());
        if active == ActiveButton::Yes {
            changes.push(AttributeChange::Reverse(false).into());
        }

        changes.push("        ".into());

        if active == ActiveButton::No {
            changes.push(AttributeChange::Reverse(true).into());
        }
        changes.push(" [N]o ".into());
        if active == ActiveButton::No {
            changes.push(AttributeChange::Reverse(false).into());
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
            }) => {
                return Ok(true);
            }
            InputEvent::Key(KeyEvent {
                key: KeyCode::Char('n' | 'N'),
                ..
            })
            | InputEvent::Key(KeyEvent {
                key: KeyCode::Escape,
                ..
            }) => {
                return Ok(false);
            }
            InputEvent::Mouse(MouseEvent {
                x,
                y,
                mouse_buttons,
                ..
            }) => {
                let x = x as usize;
                let y = y as usize;
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
                } else {
                    active = ActiveButton::None;
                }

                if mouse_buttons != MouseButtons::NONE {
                    // Treat any other mouse button as cancel
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
