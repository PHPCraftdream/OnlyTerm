use super::*;

pub(super) fn derive(action: &KeyAssignment) -> Option<Option<CommandDef>> {
    Some(Some(match action {
        PasteFrom(ClipboardPasteSource::PrimarySelection) => CommandDef {
            brief: "Paste primary selection".into(),
            doc: "Pastes text from the primary selection".into(),
            keys: vec![(Modifiers::SHIFT, "Insert".into())],
            args: &[ArgType::ActivePane],
            menubar: &["Edit"],
            icon: Some("md_content_paste"),
        },
        CopyTextTo {
            text: _,
            destination: ClipboardCopyDestination::PrimarySelection,
        }
        | CopyTo(ClipboardCopyDestination::PrimarySelection) => CommandDef {
            brief: "Copy to primary selection".into(),
            doc: "Copies text to the primary selection".into(),
            keys: vec![(Modifiers::CTRL, "Insert".into())],
            args: &[ArgType::ActivePane],
            menubar: &["Edit"],
            icon: Some("md_content_copy"),
        },
        CopyTextTo {
            text: _,
            destination: ClipboardCopyDestination::Clipboard,
        }
        | CopyTo(ClipboardCopyDestination::Clipboard) => CommandDef {
            brief: "Copy to clipboard".into(),
            doc: "Copies text to the clipboard".into(),
            keys: vec![
                (Modifiers::SUPER, "c".into()),
                (Modifiers::NONE, "Copy".into()),
            ],
            args: &[ArgType::ActivePane],
            menubar: &["Edit"],
            icon: Some("md_content_copy"),
        },
        CopySelectionOrInterrupt => CommandDef {
            brief: "Copy selection, or send Ctrl+C to interrupt".into(),
            doc: "If the active pane has a text selection, copies it to \
                  the clipboard and clears the selection. Otherwise, sends \
                  a literal Ctrl+C to the pane so it keeps working as an \
                  interrupt key. Bound by default to Ctrl+C (physical-key, \
                  layout-independent) so a plain Ctrl+C never gets \
                  unconditionally swallowed as \"copy\"."
                .into(),
            keys: vec![(Modifiers::CTRL, "c".into())],
            args: &[ArgType::ActivePane],
            menubar: &["Edit"],
            icon: Some("md_content_copy"),
        },
        CopyTextTo {
            text: _,
            destination: ClipboardCopyDestination::ClipboardAndPrimarySelection,
        }
        | CopyTo(ClipboardCopyDestination::ClipboardAndPrimarySelection) => CommandDef {
            brief: "Copy to clipboard and primary selection".into(),
            doc: "Copies text to the clipboard and the primary selection".into(),
            keys: vec![(Modifiers::CTRL, "Insert".into())],
            args: &[ArgType::ActivePane],
            menubar: &["Edit"],
            icon: Some("md_content_copy"),
        },
        PasteFrom(ClipboardPasteSource::Clipboard) => CommandDef {
            brief: "Paste from clipboard".into(),
            doc: "Pastes text from the clipboard".into(),
            keys: vec![
                (Modifiers::CTRL, "v".into()),
                (Modifiers::SUPER, "v".into()),
                (Modifiers::NONE, "Paste".into()),
            ],
            args: &[ArgType::ActivePane],
            menubar: &["Edit"],
            icon: Some("md_content_paste"),
        },
        ToggleFullScreen => CommandDef {
            brief: "Toggle full screen mode".into(),
            doc: "Switch between normal and full screen mode".into(),
            keys: vec![(Modifiers::ALT, "Return".into())],
            args: &[ArgType::ActiveWindow],
            menubar: &["View"],
            icon: Some("md_fullscreen"),
        },
        ToggleAlwaysOnTop => CommandDef {
            brief: "Toggle always on Top".into(),
            doc: "Toggles the window between floating and non-floating states to stay on top of other windows.".into(),
            keys: vec![],
            args: &[ArgType::ActiveWindow],
            menubar: &["Window"],
            icon: None,

        },
        ToggleAlwaysOnBottom => CommandDef {
            brief: "Toggle always on Bottom".into(),
            doc: "Toggles the window to remain behind all other windows.".into(),
            keys: vec![],
            args: &[ArgType::ActiveWindow],
            menubar: &["Window"],
            icon: None,
        },
        SetWindowLevel(WindowLevel::AlwaysOnTop) => CommandDef {
            brief: "Always on Top".into(),
            doc: "Set the window level to be on top of other windows.".into(),
            keys: vec![],
            args: &[ArgType::ActiveWindow],
            menubar: &["Window", "Level"],
            icon: None,
        },
        SetWindowLevel(WindowLevel::Normal) => CommandDef {
            brief: "Normal".into(),
            doc: "Set window level to normal".into(),
            keys: vec![],
            args: &[ArgType::ActiveWindow],
            menubar: &["Window", "Level"],
            icon: None,
        },
        SetWindowLevel(WindowLevel::AlwaysOnBottom) => CommandDef {
            brief: "Always on Bottom".into(),
            doc: "Set window to remain behind all other windows.".into(),
            keys: vec![],
            args: &[ArgType::ActiveWindow],
            menubar: &["Window", "Level"],
            icon: None,
        },
        Hide => CommandDef {
            brief: "Hide/Minimize Window".into(),
            doc: "Hides/Mimimizes the current window".into(),
            keys: vec![(Modifiers::SUPER, "m".into())],
            args: &[ArgType::ActiveWindow],
            menubar: &["Window"],
            icon: Some("md_window_minimize"),
        },
        Show => CommandDef {
            brief: "Show/Restore Window".into(),
            doc: "Show/Restore the current window".into(),
            keys: vec![],
            args: &[ArgType::ActiveWindow],
            menubar: &[],
            icon: Some("md_window_restore"),
        },
        HideApplication => CommandDef {
            brief: "Hide Application".into(),
            doc: "Hides all of the windows of the application. \
              This is macOS specific."
                .into(),
            keys: vec![(Modifiers::SUPER, "h".into())],
            args: &[],
            menubar: &["OnlyTerm"],
            icon: None,
        },
        SpawnWindow => CommandDef {
            brief: "New Window".into(),
            doc: "Launches the default program into a new window".into(),
            keys: vec![(Modifiers::SUPER, "n".into())],
            args: &[],
            menubar: &["Shell"],
            icon: Some("cod_empty_window"),
        },
        ClearScrollback(ScrollbackEraseMode::ScrollbackOnly) => CommandDef {
            brief: "Clear scrollback".into(),
            doc: "Clears any text that has scrolled out of the \
              viewport of the current pane"
                .into(),
            keys: vec![(Modifiers::SUPER, "k".into())],
            args: &[ArgType::ActivePane],
            menubar: &["Edit"],
            icon: Some("cod_clear_all"),
        },
        ClearScrollback(ScrollbackEraseMode::ScrollbackAndViewport) => CommandDef {
            brief: "Clear the scrollback and viewport".into(),
            doc: "Removes all content from the screen and scrollback".into(),
            keys: vec![],
            args: &[ArgType::ActivePane],
            menubar: &["Edit"],
            icon: Some("cod_clear_all"),
        },
        Search(Pattern::CurrentSelectionOrEmptyString) => CommandDef {
            brief: "Search pane output".into(),
            doc: "Enters the search mode UI for the current pane".into(),
            keys: vec![(Modifiers::SUPER, "f".into())],
            args: &[ArgType::ActivePane],
            menubar: &["Edit"],
            icon: Some("oct_search"),
        },
        Search(_) => CommandDef {
            brief: "Search pane output".into(),
            doc: "Enters the search mode UI for the current pane".into(),
            keys: vec![],
            args: &[ArgType::ActivePane],
            menubar: &[],
            icon: Some("oct_search"),
        },
        ShowDebugOverlay => CommandDef {
            brief: "Show debug overlay".into(),
            doc: "Activates the debug overlay showing version/environment info and a live log tail".into(),
            keys: vec![(Modifiers::CTRL.union(Modifiers::SHIFT), "l".into())],
            args: &[ArgType::ActiveWindow],
            menubar: &["Help"],
            icon: Some("cod_debug"),
        },
        ShowVersionOverlay => CommandDef {
            brief: "Show version overlay".into(),
            doc: "Shows a centered overlay with the OnlyTerm version and commit hash, \
                  and copies that text to the clipboard"
                .into(),
            keys: vec![(Modifiers::CTRL, "i".into())],
            args: &[ArgType::ActiveWindow],
            menubar: &["Help"],
            icon: Some("cod_info"),
        },
        OpenConfigFile => CommandDef {
            brief: "Open configuration file".into(),
            doc: "Opens your onlyterm.ktav configuration file, creating a \
                  commented starter file first if you don't have one yet"
                .into(),
            keys: vec![(Modifiers::CTRL, "o".into())],
            args: &[ArgType::ActiveWindow],
            menubar: &["Help"],
            icon: Some("cod_settings_gear"),
        },
        ActivateNewTabOptions => CommandDef {
            brief: "Show New Tab Options dialog".into(),
            doc: "Shows a modal dialog to select shell, elevation, and priority options for a new tab".into(),
            keys: vec![(Modifiers::CTRL.union(Modifiers::SHIFT), "n".into())],
            args: &[ArgType::ActiveWindow],
            menubar: &["File"],
            icon: Some("md_plus_circle_outline"),
        },
        InputSelector(_) => CommandDef {
            brief: "Prompt the user to choose from a list".into(),
            doc: "Activates the selector overlay and wait for input".into(),
            keys: vec![],
            args: &[ArgType::ActiveWindow],
            menubar: &[],
            icon: None,
        },
        Confirmation(_) => CommandDef {
            brief: "Prompt the user for confirmation".into(),
            doc: "Activates the confirmation overlay and wait for input".into(),
            keys: vec![],
            args: &[ArgType::ActiveWindow],
            menubar: &[],
            icon: None,
        },
        PromptInputLine(_) => CommandDef {
            brief: "Prompt the user for a line of text".into(),
            doc: "Activates the prompt overlay and wait for input".into(),
            keys: vec![],
            args: &[ArgType::ActiveWindow],
            menubar: &[],
            icon: None,
        },
        QuickSelect => CommandDef {
            brief: "Enter QuickSelect mode".into(),
            doc: "Activates the quick selection UI for the current pane".into(),
            keys: vec![(Modifiers::CTRL.union(Modifiers::SHIFT), "Space".into())],
            args: &[ArgType::ActivePane],
            menubar: &["Edit"],
            icon: None,
        },
        QuickSelectArgs(_) => CommandDef {
            brief: "Enter QuickSelect mode".into(),
            doc: "Activates the quick selection UI for the current pane".into(),
            keys: vec![],
            args: &[ArgType::ActivePane],
            menubar: &[],
            icon: None,
        },
        CharSelect(_) => CommandDef {
            brief: "Enter Emoji / Character selection mode".into(),
            doc: "Activates the character selection UI for the current pane".into(),
            keys: vec![(Modifiers::CTRL.union(Modifiers::SHIFT), "u".into())],
            args: &[ArgType::ActivePane],
            menubar: &["Edit"],
            icon: Some("md_sticker_emoji"),
        },
        PaneSelect(PaneSelectArguments {
            mode: PaneSelectMode::Activate,
            ..
        }) => CommandDef {
            brief: "Enter Pane selection mode".into(),
            doc: "Activates the pane selection UI".into(),
            keys: vec![], // FIXME: find a new assignment
            args: &[ArgType::ActivePane],
            menubar: &["Window"],
            icon: Some("cod_multiple_windows"),
        },
        PaneSelect(PaneSelectArguments {
            mode: PaneSelectMode::SwapWithActive,
            ..
        }) => CommandDef {
            brief: "Swap a pane with the active pane".into(),
            doc: "Activates the pane selection UI".into(),
            keys: vec![], // FIXME: find a new assignment
            args: &[ArgType::ActivePane],
            menubar: &["Window"],
            icon: Some("cod_multiple_windows"),
        },
        PaneSelect(PaneSelectArguments {
            mode: PaneSelectMode::SwapWithActiveKeepFocus,
            ..
        }) => CommandDef {
            brief: "Swap a pane with the active pane, keeping focus".into(),
            doc: "Activates the pane selection UI".into(),
            keys: vec![], // FIXME: find a new assignment
            args: &[ArgType::ActivePane],
            menubar: &["Window"],
            icon: Some("cod_multiple_windows"),
        },
        PaneSelect(PaneSelectArguments {
            mode: PaneSelectMode::MoveToNewTab,
            ..
        }) => CommandDef {
            brief: "Move a pane into its own tab".into(),
            doc: "Activates the pane selection UI".into(),
            keys: vec![], // FIXME: find a new assignment
            args: &[ArgType::ActivePane],
            menubar: &["Window"],
            icon: Some("cod_multiple_windows"),
        },
        PaneSelect(PaneSelectArguments {
            mode: PaneSelectMode::MoveToNewWindow,
            ..
        }) => CommandDef {
            brief: "Move a pane into its own window".into(),
            doc: "Activates the pane selection UI".into(),
            keys: vec![], // FIXME: find a new assignment
            args: &[ArgType::ActivePane],
            menubar: &["Window"],
            icon: Some("cod_multiple_windows"),
        },
        _ => return None,
    }))
}
