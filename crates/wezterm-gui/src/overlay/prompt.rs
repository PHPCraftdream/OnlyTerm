use crate::gui_api::guiwin::GuiWin;
use config::keyassignment::{KeyAssignment, PromptInputLine};
use mux::termwiztermtab::TermWizTerminal;
use mux::Mux;
use mux_funcs::MuxPane;
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
/// receive it, so for that legacy shape the entered text is simply discarded
/// once the overlay resolves (the `EmitEvent` shape is still validated so
/// that a config still using it fails the same way it used to, instead of
/// silently doing something unexpected). `KeyAssignment::RenameCurrentTab`
/// (task #430) is the one action this function gives real, native meaning
/// to: once the line is entered (and not cancelled), it's applied directly
/// as the new title of the tab that owns `_pane`.
pub fn show_line_prompt_overlay(
    mut term: TermWizTerminal,
    args: PromptInputLine,
    _window: GuiWin,
    pane: MuxPane,
) -> anyhow::Result<()> {
    match *args.action {
        KeyAssignment::EmitEvent(_) | KeyAssignment::RenameCurrentTab => {}
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
    let line =
        editor.read_line_with_optional_initial_value(&mut host, args.initial_value.as_deref())?;

    if let (KeyAssignment::RenameCurrentTab, Some(new_title)) = (&*args.action, line) {
        // Resolve via the pane this overlay actually covers, not "whatever
        // tab happens to be active right now" -- the two usually coincide,
        // but going through the pane is correct even if the active tab
        // changed elsewhere while this prompt was open.
        let mux = Mux::get();
        if let Some((_domain_id, _window_id, tab_id)) = mux.resolve_pane_id(pane.0) {
            if let Some(tab) = mux.get_tab(tab_id) {
                tab.set_title(&new_title);
            }
        }
    }
    // Any other action (currently only the legacy EmitEvent shape) has
    // nowhere left to send the entered line, so it's simply dropped.

    Ok(())
}
