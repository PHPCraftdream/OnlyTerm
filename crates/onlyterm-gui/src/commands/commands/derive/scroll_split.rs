use super::*;

pub(super) fn derive(action: &KeyAssignment) -> Option<Option<CommandDef>> {
    Some(Some(match action {
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
        AdjustPaneSize(PaneDirection::Next | PaneDirection::Prev, _) => return Some(None),
        ActivatePaneDirection(PaneDirection::Next | PaneDirection::Prev) => return Some(None),
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
        _ => return None,
    }))
}
