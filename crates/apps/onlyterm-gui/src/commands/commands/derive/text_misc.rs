use super::*;

pub(super) fn derive(action: &KeyAssignment) -> Option<Option<CommandDef>> {
    Some(Some(match action {
        OpenUri(uri) => match uri.as_ref() {
            "https://wezterm.org/" => CommandDef {
                brief: "Documentation".into(),
                doc: "Visit the onlyterm documentation website".into(),
                keys: vec![],
                args: &[],
                menubar: &["Help"],
                icon: Some("md_help"),
            },
            "https://github.com/wezterm/wezterm/discussions/" => CommandDef {
                brief: "Discuss on GitHub".into(),
                doc: "Visit onlyterm's GitHub discussion".into(),
                keys: vec![],
                args: &[],
                menubar: &["Help"],
                icon: Some("oct_comment_discussion"),
            },
            "https://github.com/wezterm/wezterm/issues/" => CommandDef {
                brief: "Search or report issue on GitHub".into(),
                doc: "Visit onlyterm's GitHub issues".into(),
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
            // Deliberately unbound by default: CTRL+Enter is bound to
            // `SendChar(CTRL, 'j')` below instead. A faithful modified-Enter
            // is what this action sends, but very few applications act on
            // one -- Codex CLI ignores it, and so does Windows Terminal's
            // equivalent -- whereas CTRL+J is the universally understood
            // "insert a line feed" chord. This action stays available for
            // anyone who does want the faithful form via their own config.
            keys: vec![],
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
        SendChar(mods, c) => {
            let (brief, doc, keys) = if *mods == Modifiers::CTRL && *c == 'j' {
                (
                    "Send CTRL+J, or a newline".into(),
                    "Sends CTRL+j through whatever keyboard protocol the active pane's \
                     app has negotiated (eg. an app using win32-input-mode or kitty \
                     keyboard protocol gets a properly encoded modified keypress), \
                     falling back to a literal LF (0x0A) byte if no protocol was \
                     negotiated. This gives Ctrl+J protocol awareness, so apps like \
                     Codex CLI that expect win32-input-mode-encoded keystrokes get the \
                     correct form instead of a mixed raw-byte/CSI-u stream.\n\n\
                     CTRL+Enter is bound here too, rather than to the faithful \
                     `SendEnterOrNewline(CTRL)`: a modified-Enter is what that chord \
                     literally is, but hardly anything acts on one (Codex CLI ignores \
                     it, as does Windows Terminal), while CTRL+J is the universally \
                     understood \"insert a line feed\" chord. Both chords therefore \
                     insert a newline, which is what a user pressing either of them \
                     is asking for."
                        .into(),
                    vec![
                        (Modifiers::CTRL, "j".into()),
                        (Modifiers::CTRL, "Enter".into()),
                    ],
                )
            } else {
                (
                    format!("Send '{}' via negotiated protocol or raw byte", c).into(),
                    format!(
                        "Sends character '{}' with the given modifiers through whatever \
                         keyboard protocol the pane's app has negotiated (win32-input-mode \
                         or kitty), falling back to the raw character byte (or its \
                         standard control-code encoding, if CTRL is held) if no protocol \
                         was negotiated.",
                        c
                    )
                    .into(),
                    vec![],
                )
            };
            CommandDef {
                brief,
                doc,
                keys,
                args: &[ArgType::ActivePane],
                menubar: &[],
                icon: Some("md_keyboard_return"),
            }
        }
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
        DisableDefaultAssignment => return Some(None),
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
                           workspace and switch to it"
                .to_string()
                .into(),
            doc: "Spawn the default program into a new \
                         workspace and switch to it"
                .to_string()
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
        _ => return None,
    }))
}
