use crate::scripting::guiwin::GuiWin;
use config::keyassignment::{KeyAssignment, PromptInputLine};
use mux::termwiztermtab::TermWizTerminal;
use mux_lua::MuxPane;
use termwiz::input::{InputEvent, KeyCode, KeyEvent};
use termwiz::lineedit::*;
use termwiz::surface::Change;
use termwiz::terminal::Terminal;

struct PromptHost {
    history: BasicHistory,
}

impl PromptHost {
    fn new() -> Self {
        Self {
            history: BasicHistory::default(),
        }
    }
}

impl LineEditorHost for PromptHost {
    fn history(&mut self) -> &mut dyn History {
        &mut self.history
    }

    fn resolve_action(
        &mut self,
        event: &InputEvent,
        editor: &mut LineEditor<'_>,
    ) -> Option<Action> {
        let (line, _cursor) = editor.get_line_and_cursor();
        if line.is_empty()
            && matches!(
                event,
                InputEvent::Key(KeyEvent {
                    key: KeyCode::Escape,
                    ..
                })
            )
        {
            Some(Action::Cancel)
        } else {
            None
        }
    }
}

/// Shows the line-prompt overlay and waits for the user's input.
///
/// `args.action` used to be an `EmitEvent` name dispatched to a rhai handler
/// registered via `wezterm.action_callback`, which received the entered line;
/// with the scripting layer removed there is no handler registry left to
/// receive it, so the entered text is simply discarded once the overlay
/// resolves. The `EmitEvent` shape is still validated here so that a config
/// still using the old `PromptInputLine { action }` shape fails the same way
/// it used to instead of silently doing something unexpected.
pub fn show_line_prompt_overlay(
    mut term: TermWizTerminal,
    args: PromptInputLine,
    _window: GuiWin,
    _pane: MuxPane,
) -> anyhow::Result<()> {
    match *args.action {
        KeyAssignment::EmitEvent(_) => {}
        _ => anyhow::bail!(
            "PromptInputLine requires action to be defined by wezterm.action_callback"
        ),
    };

    term.no_grab_mouse_in_raw_mode();
    let mut text = args.description.replace("\r\n", "\n").replace("\n", "\r\n");
    text.push_str("\r\n");
    term.render(&[Change::Text(text)])?;

    let mut host = PromptHost::new();
    let mut editor = LineEditor::new(&mut term);
    editor.set_prompt(&args.prompt);
    // The entered line no longer has anywhere to go (no rhai handler registry
    // exists to receive it), so just run the prompt to completion and drop
    // the result.
    let _line =
        editor.read_line_with_optional_initial_value(&mut host, args.initial_value.as_deref())?;

    Ok(())
}
