use super::*;

pub(super) fn derive(action: &KeyAssignment) -> Option<Option<CommandDef>> {
    Some(Some(match action {
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
            keys: vec![(Modifiers::SUPER, "t".into())],
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
                             filling all of the space in the tab"
                .to_string()
                .into(),
            keys: vec![],
            args: &[ArgType::ActiveWindow],
            menubar: &[],
            icon: Some("md_fullscreen"),
        },
        SetPaneZoomState(false) => CommandDef {
            brief: "Un-Zooms the current Pane".to_string().into(),
            doc: "Takes the current pane out of the zoomed state"
                .to_string()
                .into(),
            keys: vec![],
            args: &[ArgType::ActiveWindow],
            menubar: &[],
            icon: Some("md_fullscreen"),
        },
        EmitEvent(name) => CommandDef {
            brief: format!("Emit event `{name}`").into(),
            doc: "Emits the named event, causing any \
                             associated event handler(s) to trigger"
                .to_string()
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
        ActivatePaneLayoutMenu => CommandDef {
            brief: "Pane Layout".into(),
            doc: "Shows numbered split and close actions for the active pane".into(),
            keys: vec![(Modifiers::NONE, "F3".into())],
            args: &[ArgType::ActivePane],
            menubar: &["Shell"],
            icon: None,
        },
        _ => return None,
    }))
}
