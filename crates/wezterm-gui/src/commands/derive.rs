use super::*;

use config::window::WindowLevel;
use window::Modifiers;

/// Given "1" return "1st", "2" -> "2nd" and so on
fn english_ordinal(n: isize) -> String {
    let n = n.to_string();
    if n.ends_with('1') && !n.ends_with("11") {
        format!("{n}st")
    } else if n.ends_with('2') && !n.ends_with("12") {
        format!("{n}nd")
    } else if n.ends_with('3') && !n.ends_with("13") {
        format!("{n}rd")
    } else {
        format!("{n}th")
    }
}

fn spawn_command_from_action(action: &KeyAssignment) -> Option<&SpawnCommand> {
    match action {
        SplitPane(config::keyassignment::SplitPane { command, .. }) => Some(command),
        SplitHorizontal(command)
        | SplitVertical(command)
        | SpawnCommandInNewWindow(command)
        | SpawnCommandInNewTab(command) => Some(command),
        _ => None,
    }
}

fn label_string(action: &KeyAssignment, candidate: String) -> String {
    if let Some(label) = spawn_command_from_action(action).and_then(|cmd| cmd.label_for_palette()) {
        label
    } else {
        candidate
    }
}

/// Describes a key assignment action; returns a bunch
/// of metadata that is useful in the command palette/menubar context.
/// This function will be called for the result of compute_default_actions(),
/// but can also be used to describe user-provided commands
pub fn derive_command_from_key_assignment(action: &KeyAssignment) -> Option<CommandDef> {
    Some(match action {
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
        DecreaseFontSize => CommandDef {
            brief: "Decrease font size".into(),
            doc: "Scales the font size smaller by 10%".into(),
            keys: vec![
                (Modifiers::SUPER, "-".into()),
                (Modifiers::CTRL, "-".into()),
            ],
            args: &[ArgType::ActiveWindow],
            menubar: &["View", "Font Size"],
            icon: Some("md_format_size"),
        },
        IncreaseFontSize => CommandDef {
            brief: "Increase font size".into(),
            doc: "Scales the font size larger by 10%".into(),
            keys: vec![
                (Modifiers::SUPER, "=".into()),
                (Modifiers::CTRL, "=".into()),
            ],
            args: &[ArgType::ActiveWindow],
            menubar: &["View", "Font Size"],
            icon: Some("md_format_size"),
        },
        ResetFontSize => CommandDef {
            brief: "Reset font size".into(),
            doc: "Restores the font size to match your configuration file".into(),
            keys: vec![
                (Modifiers::SUPER, "0".into()),
                (Modifiers::CTRL, "0".into()),
            ],
            args: &[ArgType::ActiveWindow],
            menubar: &["View", "Font Size"],
            icon: Some("md_format_size"),
        },
        ResetFontAndWindowSize => CommandDef {
            brief: "Reset the window and font size".into(),
            doc: "Restores the original window and font size".into(),
            keys: vec![],
            args: &[ArgType::ActiveWindow],
            menubar: &["View", "Font Size"],
            icon: Some("md_format_size"),
        },
        SpawnTab(SpawnTabDomain::CurrentPaneDomain) => CommandDef {
            brief: "New Tab".into(),
            doc: "Create a new tab in the same domain as the current pane".into(),
            keys: vec![(Modifiers::SUPER, "t".into()), (Modifiers::CTRL, "t".into())],
            args: &[ArgType::ActiveWindow],
            menubar: &["Shell"],
            icon: Some("md_tab_plus"),
        },
        SpawnTab(SpawnTabDomain::DefaultDomain) => CommandDef {
            brief: "New Tab (Default Domain)".into(),
            doc: "Create a new tab in the default domain".into(),
            keys: vec![],
            args: &[ArgType::ActiveWindow],
            menubar: &["Shell"],
            icon: Some("md_tab_plus"),
        },
        SpawnTab(SpawnTabDomain::DomainName(name)) => CommandDef {
            brief: format!("New Tab (`{name}` Domain)").into(),
            doc: format!("Create a new tab in the domain named {name}").into(),
            keys: vec![],
            args: &[ArgType::ActiveWindow],
            menubar: &["Shell"],
            icon: Some("md_tab_plus"),
        },
        SpawnTab(SpawnTabDomain::DomainId(id)) => CommandDef {
            brief: format!("New Tab (Domain with id {id})").into(),
            doc: format!("Create a new tab in the domain with id {id}").into(),
            keys: vec![],
            args: &[ArgType::ActiveWindow],
            menubar: &["Shell"],
            icon: Some("md_tab_plus"),
        },
        SpawnCommandInNewTab(cmd) => CommandDef {
            brief: label_string(action, format!("Spawn a new Tab with {cmd:?}").to_string()).into(),
            doc: format!("Spawn a new Tab with {cmd:?}").into(),
            keys: vec![],
            args: &[],
            menubar: &[],
            icon: Some("md_tab_plus"),
        },
        SpawnCommandInNewWindow(cmd) => CommandDef {
            brief: label_string(
                action,
                format!("Spawn a new Window with {cmd:?}").to_string(),
            )
            .into(),
            doc: format!("Spawn a new Window with {cmd:?}").into(),
            keys: vec![],
            args: &[],
            menubar: &[],
            icon: Some("md_open_in_new"),
        },
        ActivateTab(-1) => CommandDef {
            brief: "Activate right-most tab".into(),
            doc: "Activates the tab on the far right".into(),
            keys: vec![(Modifiers::SUPER, "9".into())],
            args: &[ArgType::ActiveWindow],
            menubar: &["Window", "Select Tab"],
            icon: None,
        },
        ActivateTab(n) => {
            let n = *n;
            let ordinal = english_ordinal(n + 1);
            let mut keys = if (0..=7).contains(&n) {
                vec![(Modifiers::SUPER, (n + 1).to_string())]
            } else {
                vec![]
            };
            // Windows/Linux: Alt+1..Alt+9 activate tabs 1-9 (indices 0-8),
            // and Alt+0 activates the 10th tab (index 9).
            if (0..=8).contains(&n) {
                keys.push((Modifiers::ALT, (n + 1).to_string()));
            } else if n == 9 {
                keys.push((Modifiers::ALT, "0".into()));
            }
            CommandDef {
                brief: format!("Activate {ordinal} Tab").into(),
                doc: format!("Activates the {ordinal} tab").into(),
                keys,
                args: &[ArgType::ActiveWindow],
                menubar: &["Window", "Select Tab"],
                icon: None,
            }
        }
        ActivatePaneByIndex(n) => {
            let n = *n;
            let ordinal = english_ordinal(n as isize);
            CommandDef {
                brief: format!("Activate {ordinal} Pane").into(),
                doc: format!("Activates the {ordinal} Pane").into(),
                keys: vec![],
                args: &[ArgType::ActiveWindow],
                menubar: &[],
                icon: None,
            }
        }
        SetPaneZoomState(true) => CommandDef {
            brief: "Zooms the current Pane".to_string().into(),
            doc: "Places the current pane into the zoomed state, \
                             filling all of the space in the tab".to_string()
            .into(),
            keys: vec![],
            args: &[ArgType::ActiveWindow],
            menubar: &[],
            icon: Some("md_fullscreen"),
        },
        SetPaneZoomState(false) => CommandDef {
            brief: "Un-Zooms the current Pane".to_string().into(),
            doc: "Takes the current pane out of the zoomed state".to_string().into(),
            keys: vec![],
            args: &[ArgType::ActiveWindow],
            menubar: &[],
            icon: Some("md_fullscreen"),
        },
        EmitEvent(name) => CommandDef {
            brief: format!("Emit event `{name}`").into(),
            doc: "Emits the named event, causing any \
                             associated event handler(s) to trigger".to_string()
            .into(),
            keys: vec![],
            args: &[ArgType::ActiveWindow],
            menubar: &[],
            icon: None,
        },
        CloseCurrentTab { confirm: true } => CommandDef {
            brief: "Close current Tab".into(),
            doc: "Closes the current tab, terminating all the \
            processes that are running in its panes."
                .into(),
            keys: vec![(Modifiers::SUPER, "w".into())],
            args: &[ArgType::ActiveTab],
            menubar: &["Shell"],
            icon: Some("md_close_box_outline"),
        },
        CloseCurrentTab { confirm: false } => CommandDef {
            brief: "Close current Tab".into(),
            doc: "Closes the current tab, terminating all the \
            processes that are running in its panes."
                .into(),
            keys: vec![(Modifiers::CTRL, "w".into())],
            args: &[ArgType::ActiveTab],
            menubar: &[],
            icon: Some("md_close_box_outline"),
        },
        CloseCurrentPane { confirm: true } => CommandDef {
            brief: "Close current Pane".into(),
            doc: "Closes the current pane, terminating the \
            processes that are running inside it."
                .into(),
            keys: vec![],
            args: &[ArgType::ActivePane],
            menubar: &["Shell"],
            icon: Some("md_close_box_outline"),
        },
        CloseCurrentPane { confirm: false } => CommandDef {
            brief: "Close current Pane".into(),
            doc: "Closes the current pane, terminating the \
            processes that are running inside it."
                .into(),
            keys: vec![],
            args: &[ArgType::ActivePane],
            menubar: &[],
            icon: Some("md_close_box_outline"),
        },
        ActivateWindow(n) => {
            let n = *n;
            let ordinal = english_ordinal(n as isize + 1);
            CommandDef {
                brief: format!("Activate {ordinal} Window").into(),
                doc: format!("Activates the {ordinal} window").into(),
                keys: vec![],
                args: &[ArgType::ActiveWindow],
                menubar: &["Window", "Select Window"],
                icon: None,
            }
        }
        ActivateWindowRelative(-1) => CommandDef {
            brief: "Activate the preceeding window".into(),
            doc: "Activates the preceeding window. If this is the first \
            window then cycles around and activates last window"
                .into(),
            keys: vec![],
            args: &[ArgType::ActiveWindow],
            menubar: &["Window", "Select Window"],
            icon: None,
        },
        ActivateWindowRelative(1) => CommandDef {
            brief: "Activate the next window".into(),
            doc: "Activates the next window. If this is the last \
            window then cycles around and activates first window"
                .into(),
            keys: vec![],
            args: &[ArgType::ActiveWindow],
            menubar: &["Window", "Select Window"],
            icon: None,
        },
        ActivateWindowRelative(n) => {
            let (direction, amount) = if *n < 0 {
                ("backwards", -n)
            } else {
                ("forwards", *n)
            };
            let ordinal = english_ordinal(amount + 1);
            CommandDef {
                brief: format!("Activate the {ordinal} window {direction}").into(),
                doc: format!(
                    "Activates the {ordinal} window, moving {direction}. \
                         Wraps around to the other end"
                )
                .into(),
                keys: vec![],
                args: &[ArgType::ActiveWindow],
                menubar: &[],
                icon: None,
            }
        }
        ActivateWindowRelativeNoWrap(-1) => CommandDef {
            brief: "Activate the preceeding window".into(),
            doc: "Activates the preceeding window, stopping at the first \
            window"
                .into(),
            keys: vec![],
            args: &[ArgType::ActiveWindow],
            menubar: &["Window", "Select Window"],
            icon: None,
        },
        ActivateWindowRelativeNoWrap(1) => CommandDef {
            brief: "Activate the next window".into(),
            doc: "Activates the next window, stopping at the last \
            window"
                .into(),
            keys: vec![],
            args: &[ArgType::ActiveWindow],
            menubar: &["Window", "Select Window"],
            icon: None,
        },
        ActivateWindowRelativeNoWrap(n) => {
            let (direction, amount) = if *n < 0 {
                ("backwards", -n)
            } else {
                ("forwards", *n)
            };
            let ordinal = english_ordinal(amount + 1);
            CommandDef {
                brief: format!("Activate the {ordinal} window {direction}").into(),
                doc: format!("Activates the {ordinal} window, moving {direction}.").into(),
                keys: vec![],
                args: &[ArgType::ActiveWindow],
                menubar: &[],
                icon: None,
            }
        }
        ActivateTabRelative(-1) => CommandDef {
            brief: "Activate the tab to the left".into(),
            doc: "Activates the tab to the left. If this is the left-most \
            tab then cycles around and activates the right-most tab"
                .into(),
            keys: vec![
                (Modifiers::SUPER.union(Modifiers::SHIFT), "[".into()),
                (Modifiers::CTRL.union(Modifiers::SHIFT), "Tab".into()),
                (Modifiers::CTRL, "PageUp".into()),
            ],
            args: &[ArgType::ActiveWindow],
            menubar: &["Window", "Select Tab"],
            icon: None,
        },
        ActivateTabRelative(1) => CommandDef {
            brief: "Activate the tab to the right".into(),
            doc: "Activates the tab to the right. If this is the right-most \
            tab then cycles around and activates the left-most tab"
                .into(),
            keys: vec![
                (Modifiers::SUPER.union(Modifiers::SHIFT), "]".into()),
                (Modifiers::CTRL, "Tab".into()),
                (Modifiers::CTRL, "PageDown".into()),
            ],
            args: &[ArgType::ActiveWindow],
            menubar: &["Window", "Select Tab"],
            icon: None,
        },
        ActivateTabRelative(n) => {
            let (direction, amount) = if *n < 0 { ("left", -n) } else { ("right", *n) };
            let ordinal = english_ordinal(amount + 1);
            CommandDef {
                brief: format!("Activate the {ordinal} tab to the {direction}").into(),
                doc: format!(
                    "Activates the {ordinal} tab to the {direction}. \
                         Wraps around to the other end"
                )
                .into(),
                keys: vec![],
                args: &[ArgType::ActiveWindow],
                menubar: &[],
                icon: None,
            }
        }
        ActivateTabRelativeNoWrap(-1) => CommandDef {
            brief: "Activate the tab to the left (no wrapping)".into(),
            doc: "Activates the tab to the left. Stopping at the left-most tab".into(),
            keys: vec![],
            args: &[ArgType::ActiveWindow],
            menubar: &[],
            icon: None,
        },
        ActivateTabRelativeNoWrap(1) => CommandDef {
            brief: "Activate the tab to the right (no wrapping)".into(),
            doc: "Activates the tab to the right. Stopping at the right-most tab".into(),
            keys: vec![],
            args: &[ArgType::ActiveWindow],
            menubar: &[],
            icon: None,
        },
        ActivateTabRelativeNoWrap(n) => {
            let (direction, amount) = if *n < 0 { ("left", -n) } else { ("right", *n) };
            let ordinal = english_ordinal(amount + 1);
            CommandDef {
                brief: format!("Activate the {ordinal} tab to the {direction}").into(),
                doc: format!("Activates the {ordinal} tab to the {direction}").into(),
                keys: vec![],
                args: &[ArgType::ActiveWindow],
                menubar: &[],
                icon: None,
            }
        }
        ReloadConfiguration => CommandDef {
            brief: "Reload configuration".into(),
            doc: "Reloads the configuration file".into(),
            keys: vec![(Modifiers::SUPER, "r".into())],
            args: &[],
            menubar: &["OnlyTerm"],
            icon: Some("md_reload"),
        },
        QuitApplication => CommandDef {
            brief: "Quit OnlyTerm".into(),
            doc: "Quits OnlyTerm".into(),
            keys: vec![(Modifiers::SUPER, "q".into())],
            args: &[],
            menubar: &["OnlyTerm"],
            icon: Some("oct_stop"),
        },
        MoveTabRelative(-1) => CommandDef {
            brief: "Move tab one place to the left".into(),
            doc: "Rearranges the tabs so that the current tab moves \
            one place to the left"
                .into(),
            keys: vec![(Modifiers::CTRL.union(Modifiers::SHIFT), "PageUp".into())],
            args: &[ArgType::ActiveTab],
            menubar: &["Window", "Move Tab"],
            icon: Some("fa_long_arrow_left"),
        },
        MoveTabRelative(1) => CommandDef {
            brief: "Move tab one place to the right".into(),
            doc: "Rearranges the tabs so that the current tab moves \
            one place to the right"
                .into(),
            keys: vec![(Modifiers::CTRL.union(Modifiers::SHIFT), "PageDown".into())],
            args: &[ArgType::ActiveTab],
            menubar: &["Window", "Move Tab"],
            icon: Some("fa_long_arrow_right"),
        },
        MoveTabRelative(n) => {
            let (direction, amount, icon) = if *n < 0 {
                ("left", (-n).to_string(), "md_chevron_double_left")
            } else {
                ("right", n.to_string(), "md_chevron_double_right")
            };

            CommandDef {
                brief: format!("Move tab {amount} place(s) to the {direction}").into(),
                doc: format!(
                    "Rearranges the tabs so that the current tab moves \
            {amount} place(s) to the {direction}"
                )
                .into(),
                keys: vec![],
                args: &[ArgType::ActiveTab],
                menubar: &[],
                icon: Some(icon),
            }
        }
        MoveTab(n) => {
            let n = (*n) + 1;
            CommandDef {
                brief: format!("Move tab to index {n}").into(),
                doc: format!(
                    "Rearranges the tabs so that the current tab \
                             moves to position {n}"
                )
                .into(),
                keys: vec![],
                args: &[ArgType::ActiveTab],
                menubar: &[],
                icon: None,
            }
        }
        RenameCurrentTab => CommandDef {
            brief: "Rename Tab".into(),
            doc: "Prompts for a new title for the current tab, pre-filled \
            with its current title (same convention as F2 in Windows \
            Explorer). The tab bar can also be double-clicked to trigger \
            this."
                .into(),
            keys: vec![(Modifiers::NONE, "F2".into())],
            args: &[ArgType::ActiveTab],
            menubar: &["Window"],
            icon: Some("fa_pencil"),
        },
        ScrollByPage(amount) => {
            let amount = amount.into_inner();
            if amount == -1.0 {
                CommandDef {
                    brief: "Scroll Up One Page".into(),
                    doc: "Scrolls the viewport up by 1 page".into(),
                    keys: vec![(Modifiers::SHIFT, "PageUp".into())],
                    args: &[ArgType::ActivePane],
                    menubar: &["View"],
                    icon: None,
                }
            } else if amount == 1.0 {
                CommandDef {
                    brief: "Scroll Down One Page".into(),
                    doc: "Scrolls the viewport down by 1 page".into(),
                    keys: vec![(Modifiers::SHIFT, "PageDown".into())],
                    args: &[ArgType::ActivePane],
                    menubar: &["View"],
                    icon: None,
                }
            } else if amount < 0.0 {
                let amount = -amount;
                CommandDef {
                    brief: format!("Scroll Up {amount} Page(s)").into(),
                    doc: format!("Scrolls the viewport up by {amount} pages").into(),
                    keys: vec![],
                    args: &[ArgType::ActivePane],
                    menubar: &["View"],
                    icon: None,
                }
            } else {
                CommandDef {
                    brief: format!("Scroll Down {amount} Page(s)").into(),
                    doc: format!("Scrolls the viewport down by {amount} pages").into(),
                    keys: vec![],
                    args: &[ArgType::ActivePane],
                    menubar: &["View"],
                    icon: None,
                }
            }
        }
        ScrollByLine(n) => {
            let (direction, amount) = if *n < 0 {
                ("up", (-n).to_string())
            } else {
                ("down", n.to_string())
            };
            CommandDef {
                brief: format!("Scroll {direction} {amount} line(s)").into(),
                doc: format!(
                    "Scrolls the viewport {direction} by \
                             {amount} line(s)"
                )
                .into(),
                keys: vec![],
                args: &[ArgType::ActivePane],
                menubar: &[],
                icon: None,
            }
        }
        ScrollToPrompt(n) => {
            let (direction, amount) = if *n < 0 { ("up", -n) } else { ("down", *n) };
            let ordinal = english_ordinal(amount);
            CommandDef {
                brief: format!("Scroll {direction} {amount} prompt(s)").into(),
                doc: format!(
                    "Scrolls the viewport {direction} to the \
                             {ordinal} semantic prompt zone in that direction"
                )
                .into(),
                keys: vec![],
                args: &[ArgType::ActivePane],
                menubar: &[],
                icon: Some("oct_terminal"),
            }
        }
        ScrollByCurrentEventWheelDelta => CommandDef {
            brief: "Scrolls based on the mouse wheel position \
                in the current mouse event"
                .into(),
            doc: "Scrolls based on the mouse wheel position \
                in the current mouse event"
                .into(),
            keys: vec![],
            args: &[ArgType::ActivePane],
            menubar: &[],
            icon: None,
        },
        ScrollToBottom => CommandDef {
            brief: "Scroll to the bottom".into(),
            doc: "Scrolls to the bottom of the viewport".into(),
            keys: vec![],
            args: &[ArgType::ActivePane],
            menubar: &["View"],
            icon: Some("md_format_align_bottom"),
        },
        ScrollToTop => CommandDef {
            brief: "Scroll to the top".into(),
            doc: "Scrolls to the top of the viewport".into(),
            keys: vec![],
            args: &[ArgType::ActivePane],
            menubar: &["View"],
            icon: Some("md_format_align_top"),
        },
        ActivateCopyMode => CommandDef {
            brief: "Activate Copy Mode".into(),
            doc: "Enter mouse-less copy mode to select text using only \
            the keyboard"
                .into(),
            keys: vec![(Modifiers::CTRL.union(Modifiers::SHIFT), "x".into())],
            args: &[ArgType::ActivePane],
            menubar: &["Edit"],
            icon: Some("md_content_copy"),
        },
        SplitVertical(SpawnCommand {
            domain: SpawnTabDomain::CurrentPaneDomain,
            ..
        }) => CommandDef {
            brief: label_string(action, "Split Vertically (Top/Bottom)".to_string()).into(),
            doc: "Split the current pane vertically into two panes, by spawning \
            the default program into the bottom half"
                .into(),
            keys: vec![(
                Modifiers::CTRL
                    .union(Modifiers::ALT)
                    .union(Modifiers::SHIFT),
                "'".into(),
            )],
            args: &[ArgType::ActivePane],
            menubar: &["Shell"],
            icon: Some("cod_split_vertical"),
        },
        SplitHorizontal(SpawnCommand {
            domain: SpawnTabDomain::CurrentPaneDomain,
            ..
        }) => CommandDef {
            brief: label_string(action, "Split Horizontally (Left/Right)".to_string()).into(),
            doc: "Split the current pane horizontally into two panes, by spawning \
            the default program into the right hand side"
                .into(),
            keys: vec![(
                Modifiers::CTRL
                    .union(Modifiers::ALT)
                    .union(Modifiers::SHIFT),
                "5".into(),
            )],
            args: &[ArgType::ActivePane],
            menubar: &["Shell"],
            icon: Some("cod_split_horizontal"),
        },
        SplitHorizontal(_) => CommandDef {
            brief: label_string(action, "Split Horizontally (Left/Right)".to_string()).into(),
            doc: "Split the current pane horizontally into two panes, by spawning \
            the default program into the right hand side"
                .into(),
            keys: vec![],
            args: &[ArgType::ActivePane],
            menubar: &[],
            icon: Some("cod_split_horizontal"),
        },
        SplitVertical(_) => CommandDef {
            brief: label_string(action, "Split Vertically (Top/Bottom)".to_string()).into(),
            doc: "Split the current pane veritically into two panes, by spawning \
            the default program into the bottom"
                .into(),
            keys: vec![],
            args: &[ArgType::ActivePane],
            menubar: &[],
            icon: Some("cod_split_vertical"),
        },
        AdjustPaneSize(PaneDirection::Left, amount) => CommandDef {
            brief: format!("Resize Pane {amount} cell(s) to the Left").into(),
            doc: "Adjusts the closest split divider to the left".into(),
            keys: vec![(
                Modifiers::CTRL
                    .union(Modifiers::ALT)
                    .union(Modifiers::SHIFT),
                "LeftArrow".into(),
            )],
            args: &[ArgType::ActivePane],
            menubar: &["Window", "Resize Pane"],
            icon: None,
        },
        AdjustPaneSize(PaneDirection::Right, amount) => CommandDef {
            brief: format!("Resize Pane {amount} cell(s) to the Right").into(),
            doc: "Adjusts the closest split divider to the right".into(),
            keys: vec![(
                Modifiers::CTRL
                    .union(Modifiers::ALT)
                    .union(Modifiers::SHIFT),
                "RightArrow".into(),
            )],
            args: &[ArgType::ActivePane],
            menubar: &["Window", "Resize Pane"],
            icon: None,
        },
        AdjustPaneSize(PaneDirection::Up, amount) => CommandDef {
            brief: format!("Resize Pane {amount} cell(s) Upwards").into(),
            doc: "Adjusts the closest split divider towards the top".into(),
            keys: vec![(
                Modifiers::CTRL
                    .union(Modifiers::ALT)
                    .union(Modifiers::SHIFT),
                "UpArrow".into(),
            )],
            args: &[ArgType::ActivePane],
            menubar: &["Window", "Resize Pane"],
            icon: None,
        },
        AdjustPaneSize(PaneDirection::Down, amount) => CommandDef {
            brief: format!("Resize Pane {amount} cell(s) Downwards").into(),
            doc: "Adjusts the closest split divider towards the bottom".into(),
            keys: vec![(
                Modifiers::CTRL
                    .union(Modifiers::ALT)
                    .union(Modifiers::SHIFT),
                "DownArrow".into(),
            )],
            args: &[ArgType::ActivePane],
            menubar: &["Window", "Resize Pane"],
            icon: None,
        },
        AdjustPaneSize(PaneDirection::Next | PaneDirection::Prev, _) => return None,
        ActivatePaneDirection(PaneDirection::Next | PaneDirection::Prev) => return None,
        ActivatePaneDirection(PaneDirection::Left) => CommandDef {
            brief: "Activate Pane Left".into(),
            doc: "Activates the pane to the left of the current pane".into(),
            keys: vec![(Modifiers::CTRL.union(Modifiers::SHIFT), "LeftArrow".into())],
            args: &[ArgType::ActivePane],
            menubar: &["Window", "Select Pane"],
            icon: Some("fa_long_arrow_left"),
        },
        ActivatePaneDirection(PaneDirection::Right) => CommandDef {
            brief: "Activate Pane Right".into(),
            doc: "Activates the pane to the right of the current pane".into(),
            keys: vec![(Modifiers::CTRL.union(Modifiers::SHIFT), "RightArrow".into())],
            args: &[ArgType::ActivePane],
            menubar: &["Window", "Select Pane"],
            icon: Some("fa_long_arrow_right"),
        },
        ActivatePaneDirection(PaneDirection::Up) => CommandDef {
            brief: "Activate Pane Up".into(),
            doc: "Activates the pane to the top of the current pane".into(),
            keys: vec![(Modifiers::CTRL.union(Modifiers::SHIFT), "UpArrow".into())],
            args: &[ArgType::ActivePane],
            menubar: &["Window", "Select Pane"],
            icon: Some("fa_long_arrow_up"),
        },
        ActivatePaneDirection(PaneDirection::Down) => CommandDef {
            brief: "Activate Pane Down".into(),
            doc: "Activates the pane to the bottom of the current pane".into(),
            keys: vec![(Modifiers::CTRL.union(Modifiers::SHIFT), "DownArrow".into())],
            args: &[ArgType::ActivePane],
            menubar: &["Window", "Select Pane"],
            icon: Some("fa_long_arrow_down"),
        },
        TogglePaneZoomState => CommandDef {
            brief: "Toggle Pane Zoom".into(),
            doc: "Toggles the zoom state for the current pane".into(),
            keys: vec![(Modifiers::CTRL.union(Modifiers::SHIFT), "z".into())],
            args: &[ArgType::ActivePane],
            menubar: &["Window"],
            icon: Some("md_fullscreen"),
        },
        ActivateLastTab => CommandDef {
            brief: "Activate the last active tab".into(),
            doc: "If there was no prior active tab, has no effect.".into(),
            keys: vec![],
            args: &[ArgType::ActiveWindow],
            menubar: &["Window", "Select Tab"],
            icon: None,
        },
        ClearKeyTableStack => CommandDef {
            brief: "Clear the key table stack".into(),
            doc: "Removes all entries from the stack".into(),
            keys: vec![],
            args: &[ArgType::ActiveWindow],
            menubar: &["Edit"],
            icon: None,
        },
        OpenLinkAtMouseCursor => CommandDef {
            brief: "Open link at mouse cursor".into(),
            doc: "If there is no link under the mouse cursor, has no effect.".into(),
            keys: vec![],
            args: &[ArgType::ActivePane],
            menubar: &["Shell"],
            icon: None,
        },
        CopyLinkAtMouseCursor(destination) => CommandDef {
            brief: format!("Copy link at mouse cursor to {destination:?}").into(),
            doc: "If there is no link under the mouse cursor, has no effect.".into(),
            keys: vec![],
            args: &[ArgType::ActivePane],
            menubar: &["Shell"],
            icon: None,
        },
        ShowLauncherArgs(_) => CommandDef {
            brief: "Show the launcher".into(),
            doc: "Shows the launcher menu".into(),
            keys: vec![],
            args: &[ArgType::ActiveWindow],
            menubar: &["Shell"],
            icon: None,
        },
        ShowTabNavigator => CommandDef {
            brief: "Navigate tabs".into(),
            doc: "Shows the tab navigator".into(),
            keys: vec![],
            args: &[ArgType::ActiveWindow],
            menubar: &["Window", "Select Tab"],
            icon: Some("cod_list_flat"),
        },
        DetachDomain(SpawnTabDomain::CurrentPaneDomain) => CommandDef {
            brief: "Detach the domain of the active pane".into(),
            doc: "Detaches (disconnects from) the domain of the active pane".into(),
            keys: vec![],
            args: &[ArgType::ActivePane],
            menubar: &["Shell", "Detach"],
            icon: Some("md_pipe_disconnected"),
        },
        DetachDomain(SpawnTabDomain::DefaultDomain) => CommandDef {
            brief: "Detach the default domain".into(),
            doc: "Detaches (disconnects from) the default domain".into(),
            keys: vec![],
            args: &[ArgType::ActivePane],
            menubar: &["Shell", "Detach"],
            icon: Some("md_pipe_disconnected"),
        },
        DetachDomain(SpawnTabDomain::DomainName(name)) => CommandDef {
            brief: format!("Detach the `{name}` domain").into(),
            doc: format!("Detaches (disconnects from) the domain named `{name}`").into(),
            keys: vec![],
            args: &[ArgType::ActivePane],
            menubar: &["Shell", "Detach"],
            icon: Some("md_pipe_disconnected"),
        },
        DetachDomain(SpawnTabDomain::DomainId(id)) => CommandDef {
            brief: format!("Detach the domain with id {id}").into(),
            doc: format!("Detaches (disconnects from) the domain with id {id}").into(),
            keys: vec![],
            args: &[ArgType::ActivePane],
            menubar: &["Shell", "Detach"],
            icon: Some("md_pipe_disconnected"),
        },
        OpenUri(uri) => match uri.as_ref() {
            "https://wezterm.org/" => CommandDef {
                brief: "Documentation".into(),
                doc: "Visit the wezterm documentation website".into(),
                keys: vec![],
                args: &[],
                menubar: &["Help"],
                icon: Some("md_help"),
            },
            "https://github.com/wezterm/wezterm/discussions/" => CommandDef {
                brief: "Discuss on GitHub".into(),
                doc: "Visit wezterm's GitHub discussion".into(),
                keys: vec![],
                args: &[],
                menubar: &["Help"],
                icon: Some("oct_comment_discussion"),
            },
            "https://github.com/wezterm/wezterm/issues/" => CommandDef {
                brief: "Search or report issue on GitHub".into(),
                doc: "Visit wezterm's GitHub issues".into(),
                keys: vec![],
                args: &[],
                menubar: &["Help"],
                icon: Some("fa_ticket"),
            },
            _ => CommandDef {
                brief: format!("Open {uri} in your browser").into(),
                doc: format!("Open {uri} in your browser").into(),
                keys: vec![],
                args: &[],
                menubar: &[],
                icon: Some("oct_browser"),
            },
        },
        SendEnterOrNewline(mods) if *mods == Modifiers::CTRL => CommandDef {
            brief: "Send CTRL+Enter, or a newline".into(),
            doc: "Sends Enter with CTRL held through whatever keyboard \
                  protocol the active pane's app has negotiated (eg. an \
                  app using the kitty keyboard protocol gets a properly \
                  disambiguated modified-Enter). If the app hasn't \
                  negotiated such a protocol, sends a line feed (LF, \
                  0x0A) instead, so a newline can still be inserted \
                  reliably (eg: in a multi-line prompt)."
                .into(),
            keys: vec![(Modifiers::CTRL, "Enter".into())],
            args: &[ArgType::ActivePane],
            menubar: &[],
            icon: Some("md_keyboard_return"),
        },
        SendEnterOrNewline(mods) if *mods == Modifiers::SHIFT => CommandDef {
            brief: "Send SHIFT+Enter, or a newline".into(),
            doc: "Sends Enter with SHIFT held through whatever keyboard \
                  protocol the active pane's app has negotiated (eg. an \
                  app using the kitty keyboard protocol gets a properly \
                  disambiguated modified-Enter). If the app hasn't \
                  negotiated such a protocol, sends a line feed (LF, \
                  0x0A) instead, so a newline can still be inserted \
                  reliably (eg: in a multi-line prompt)."
                .into(),
            keys: vec![(Modifiers::SHIFT, "Enter".into())],
            args: &[ArgType::ActivePane],
            menubar: &[],
            icon: Some("md_keyboard_return"),
        },
        SendEnterOrNewline(_) => CommandDef {
            brief: "Send modified Enter, or a newline".into(),
            doc: "Sends Enter with the given modifier held through \
                  whatever keyboard protocol the active pane's app has \
                  negotiated, falling back to a line feed (LF, 0x0A) if \
                  it hasn't negotiated one."
                .into(),
            keys: vec![],
            args: &[ArgType::ActivePane],
            menubar: &[],
            icon: Some("md_keyboard_return"),
        },
        SendString(text) => CommandDef {
            brief: format!(
                "Sends `{text}` to the active pane, \
                           as though you typed it"
            )
            .into(),
            doc: format!(
                "Sends `{text}` to the active pane, as \
                         though you typed it"
            )
            .into(),
            keys: vec![],
            args: &[],
            menubar: &[],
            icon: Some("md_keyboard_variant"),
        },
        SendKey(key) => CommandDef {
            brief: format!(
                "Sends {key:?} to the active pane, \
                           as though you typed it"
            )
            .into(),
            doc: format!(
                "Sends {key:?} to the active pane, \
                         as though you typed it"
            )
            .into(),
            keys: vec![],
            args: &[],
            menubar: &[],
            icon: Some("md_keyboard_variant"),
        },
        Nop => CommandDef {
            brief: "Does nothing".into(),
            doc: "Has no effect".into(),
            keys: vec![],
            args: &[],
            menubar: &[],
            icon: None,
        },
        DisableDefaultAssignment => return None,
        SelectTextAtMouseCursor(mode) => CommandDef {
            brief: format!(
                "Selects text at the mouse cursor \
                           location using {mode:?}"
            )
            .into(),
            doc: format!(
                "Selects text at the mouse cursor \
                         location using {mode:?}"
            )
            .into(),
            keys: vec![],
            args: &[],
            menubar: &[],
            icon: None,
        },
        ExtendSelectionToMouseCursor(mode) => CommandDef {
            brief: format!(
                "Extends the selection text to the mouse \
                           cursor location using {mode:?}"
            )
            .into(),
            doc: format!(
                "Extends the selection text to the mouse \
                         cursor location using {mode:?}"
            )
            .into(),
            keys: vec![],
            args: &[],
            menubar: &[],
            icon: None,
        },
        ClearSelection => CommandDef {
            brief: "Clears the selection in the current pane".into(),
            doc: "Clears the selection in the current pane".into(),
            keys: vec![],
            args: &[],
            menubar: &[],
            icon: None,
        },
        CompleteSelection(destination) => CommandDef {
            brief: format!("Completes selection, and copy {destination:?}").into(),
            doc: format!(
                "Completes text selection using the mouse, and copies \
                to {destination:?}"
            )
            .into(),
            keys: vec![],
            args: &[],
            menubar: &[],
            icon: None,
        },
        CompleteSelectionOrOpenLinkAtMouseCursor(destination) => CommandDef {
            brief: format!(
                "Open a URL or Completes selection \
            by copying to {destination:?}"
            )
            .into(),
            doc: format!(
                "If the mouse is over a link, open it, otherwise, completes \
                text selection using the mouse, and copies to {destination:?}"
            )
            .into(),
            keys: vec![],
            args: &[],
            menubar: &[],
            icon: None,
        },
        StartWindowDrag => CommandDef {
            brief: "Requests a window drag operation from \
                the window environment"
                .into(),
            doc: "Requests a window drag operation from \
                the window environment"
                .into(),
            keys: vec![],
            args: &[],
            menubar: &[],
            icon: Some("md_drag"),
        },
        Multiple(actions) => {
            let mut brief = String::new();
            for act in actions {
                if !brief.is_empty() {
                    brief.push_str(", ");
                }
                match derive_command_from_key_assignment(act) {
                    Some(cmd) => {
                        brief.push_str(&cmd.brief);
                    }
                    None => {
                        brief.push_str(&format!("{act:?}"));
                    }
                }
            }
            CommandDef {
                brief: brief.into(),
                doc: "Performs multiple nested actions".into(),
                keys: vec![],
                args: &[ArgType::ActivePane],
                menubar: &[],
                icon: None,
            }
        }
        SwitchToWorkspace {
            name: None,
            spawn: None,
        } => CommandDef {
            brief: "Spawn the default program into a new \
                           workspace and switch to it".to_string()
            .into(),
            doc: "Spawn the default program into a new \
                         workspace and switch to it".to_string()
            .into(),
            keys: vec![],
            args: &[],
            menubar: &["Window", "Workspace"],
            icon: None,
        },
        SwitchToWorkspace {
            name: Some(name),
            spawn: None,
        } => CommandDef {
            brief: format!(
                "Switch to workspace `{name}`, spawn the \
                           default program if that workspace doesn't already exist"
            )
            .into(),
            doc: format!(
                "Switch to workspace `{name}`, spawn the \
                         default program if that workspace doesn't already exist"
            )
            .into(),
            keys: vec![],
            args: &[],
            menubar: &["Window", "Workspace"],
            icon: None,
        },
        SwitchToWorkspace {
            name: Some(name),
            spawn: Some(prog),
        } => CommandDef {
            brief: format!(
                "Switch to workspace `{name}`, spawn {prog:?} \
                           if that workspace doesn't already exist"
            )
            .into(),
            doc: format!(
                "Switch to workspace `{name}`, spawn {prog:?} \
                         if that workspace doesn't already exist"
            )
            .into(),
            keys: vec![],
            args: &[],
            menubar: &["Window", "Workspace"],
            icon: None,
        },
        SwitchToWorkspace {
            name: None,
            spawn: Some(prog),
        } => CommandDef {
            brief: format!("Spawn the {prog:?} into a new workspace and switch to it").into(),
            doc: format!("Spawn the {prog:?} into a new workspace and switch to it").into(),
            keys: vec![],
            args: &[],
            menubar: &["Window", "Workspace"],
            icon: None,
        },
        SwitchWorkspaceRelative(n) => {
            let (direction, amount) = if *n < 0 {
                ("previous", -n)
            } else {
                ("next", *n)
            };
            let ordinal = english_ordinal(amount);
            CommandDef {
                brief: format!("Switch to {ordinal} {direction} workspace").into(),
                doc: format!(
                    "Switch to the {ordinal} {direction} workspace, \
                             ordered lexicographically by workspace name"
                )
                .into(),
                keys: vec![],
                args: &[ArgType::ActivePane],
                menubar: &["Window", "Workspace"],
                icon: None,
            }
        }
        ActivateKeyTable { name, .. } => CommandDef {
            brief: format!("Activate key table `{name}`").into(),
            doc: format!("Activate key table `{name}`").into(),
            keys: vec![],
            args: &[ArgType::ActivePane],
            menubar: &[],
            icon: None,
        },
        PopKeyTable => CommandDef {
            brief: "Pop the current key table".into(),
            doc: "Pop the current key table".into(),
            keys: vec![],
            args: &[ArgType::ActivePane],
            menubar: &[],
            icon: None,
        },
        AttachDomain(name) => CommandDef {
            brief: format!("Attach domain `{name}`").into(),
            doc: format!("Attach domain `{name}`").into(),
            keys: vec![],
            args: &[ArgType::ActivePane],
            menubar: &["Shell", "Attach"],
            icon: Some("md_pipe"),
        },
        CopyMode(copy_mode) => CommandDef {
            brief: format!("{copy_mode:?}").into(),
            doc: "".into(),
            keys: vec![],
            args: &[ArgType::ActivePane],
            menubar: &["Edit", "Copy Mode"],
            icon: None,
        },
        RotatePanes(direction) => CommandDef {
            brief: format!("Rotate panes {direction:?}").into(),
            doc: format!("Rotate panes {direction:?}").into(),
            keys: vec![],
            args: &[ArgType::ActivePane],
            menubar: &["Window", "Rotate Pane"],
            icon: Some(match direction {
                RotationDirection::Clockwise => "md_rotate_right",
                RotationDirection::CounterClockwise => "md_rotate_left",
            }),
        },
        SplitPane(split) => {
            let direction = split.direction;
            CommandDef {
                brief: label_string(action, format!("Split the current pane {direction:?}")).into(),
                doc: format!("Split the current pane {direction:?}").into(),
                keys: vec![],
                args: &[ArgType::ActivePane],
                menubar: &[],
                icon: match split.direction {
                    PaneDirection::Up | PaneDirection::Down => Some("cod_split_vertical"),
                    PaneDirection::Left | PaneDirection::Right => Some("cod_split_horizontal"),
                    PaneDirection::Next | PaneDirection::Prev => None,
                },
            }
        }
        ResetTerminal => CommandDef {
            brief: "Reset the terminal emulation state in the current pane".into(),
            doc: "Reset the terminal emulation state in the current pane".into(),
            keys: vec![],
            args: &[ArgType::ActivePane],
            menubar: &["Shell"],
            icon: None,
        },
        ActivateCommandPalette => CommandDef {
            brief: "Activate Command Palette".into(),
            doc: "Shows the command palette modal".into(),
            keys: vec![(Modifiers::CTRL.union(Modifiers::SHIFT), "p".into())],
            args: &[ArgType::ActivePane],
            menubar: &["Edit"],
            icon: None,
        },
    })
}
