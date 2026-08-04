use super::*;

use crate::inputmap::InputMap;
use config::window::WindowLevel;
use config::{ConfigHandle, DeferredKeyCode};
use mux::domain::DomainState;
use mux::Mux;
use ordered_float::NotNan;
use std::borrow::Cow;
use std::cmp::Ordering;
use std::convert::TryFrom;
use window::{KeyCode, Modifiers};

/// A helper function used to synthesize key binding permutations.
/// If the input is a character on a US ANSI keyboard layout, returns
/// the typical character that is produced when holding down
/// the shift key and pressing the original key.
/// This doesn't produce an exhaustive list because there are only
/// a handful of default assignments in the command DEFS below.
fn us_layout_shift(s: &str) -> String {
    match s {
        "1" => "!".to_string(),
        "2" => "@".to_string(),
        "3" => "#".to_string(),
        "4" => "$".to_string(),
        "5" => "%".to_string(),
        "6" => "^".to_string(),
        "7" => "&".to_string(),
        "8" => "*".to_string(),
        "9" => "(".to_string(),
        "0" => ")".to_string(),
        "[" => "{".to_string(),
        "]" => "}".to_string(),
        "=" => "+".to_string(),
        "-" => "_".to_string(),
        "'" => "\"".to_string(),
        s if s.len() == 1 => s.to_ascii_uppercase(),
        s => s.to_string(),
    }
}

impl CommandDef {
    /// Blech. Depending on the OS, a shifted key combination
    /// such as CTRL-SHIFT-L may present as either:
    /// CTRL+SHIFT + mapped lowercase l
    /// CTRL+SHIFT + mapped uppercase l
    /// CTRL       + mapped uppercase l
    ///
    /// This logic synthesizes the different combinations so
    /// that it isn't such a headache to maintain the mapping
    /// and prevents missing cases.
    ///
    /// Note that the mapped form of these things assumes
    /// US layout for some of the special shifted/punctuation cases.
    /// It's not perfect.
    ///
    /// The synthesis here requires that the defaults in
    /// the keymap below use the lowercase form of single characters!
    ///
    /// In addition to the SHIFT-related permutations above, this also
    /// synthesizes a `KeyCode::Physical` variant alongside any binding
    /// that combines a `KeyCode::Char` with CTRL and/or SUPER.
    /// Those modifier chords are the ones typically used for application-level
    /// shortcuts (Copy, Paste, tab/pane navigation, etc.) and their intent
    /// is defined by the *position* of the key on the keyboard (eg: the "C"
    /// key on a US ANSI layout), not by the Unicode character that the
    /// active keyboard layout happens to produce for that position.
    ///
    /// On layouts that are not Latin-based (Cyrillic, Greek, CJK IMEs, etc.)
    /// the character produced for that physical key won't match the
    /// `KeyCode::Char` we registered (eg: the "C" position on a Russian
    /// ЙЦУКЕН layout produces 'с', not 'c'), so without a physical fallback
    /// entry the default binding would silently fail to trigger.
    /// Registering the physical variant alongside the mapped one allows the
    /// physical-key-first lookup pass (see `raw_key_event_impl` in
    /// `keyevent.rs`) to resolve these regardless of the active layout,
    /// while leaving plain, unmodified text entry (no CTRL/SUPER) completely
    /// untouched so CJK/Cyrillic/etc. typing keeps working exactly as before.
    fn permute_keys(&self, config: &ConfigHandle) -> Vec<(Modifiers, KeyCode)> {
        let mut keys = vec![];

        // Only Char keys combined with CTRL and/or SUPER are eligible for a
        // layout-independent physical fallback; bare typing (no modifiers)
        // and pure-SHIFT combinations must resolve exactly as before so that
        // non-Latin scripts are never affected.
        fn push_with_phys_fallback(
            keys: &mut Vec<(Modifiers, KeyCode)>,
            mods: Modifiers,
            key: KeyCode,
        ) {
            let wants_phys_fallback = matches!(key, KeyCode::Char(_))
                && (mods.contains(Modifiers::CTRL) || mods.contains(Modifiers::SUPER));

            if wants_phys_fallback {
                if let Some(phys) = key.to_phys() {
                    let phys_key = KeyCode::Physical(phys);
                    if phys_key != key {
                        keys.push((mods, phys_key));
                    }
                }
            }

            keys.push((mods, key));
        }

        for (mods, label) in &self.keys {
            let mods = *mods;
            let key = DeferredKeyCode::try_from(label.as_str())
                .unwrap()
                .resolve(config.key_map_preference)
                .clone();

            let ukey = DeferredKeyCode::try_from(us_layout_shift(label))
                .unwrap()
                .resolve(config.key_map_preference)
                .clone();

            push_with_phys_fallback(&mut keys, mods, key.clone());

            if mods == Modifiers::SUPER {
                // We want each SUPER/CMD version of the keys to also have
                // CTRL+SHIFT version(s) for environments where SUPER/CMD
                // is reserved for the window manager.
                // This bit synthesizes those.
                push_with_phys_fallback(&mut keys, Modifiers::CTRL | Modifiers::SHIFT, key.clone());
                if ukey != key {
                    push_with_phys_fallback(
                        &mut keys,
                        Modifiers::CTRL | Modifiers::SHIFT,
                        ukey.clone(),
                    );
                    // `InputMap::lookup_key` always strips SHIFT from the
                    // *query* before searching (`key.normalize_shift(...)`),
                    // so a stored entry that still has SHIFT set (like the
                    // CTRL|SHIFT ones just above) can never actually be
                    // found - the only way CTRL+SHIFT+<letter> is ever
                    // reachable is by storing it as bare CTRL + the
                    // *shifted/uppercase* character, matching what a
                    // normalized query collapses down to. Register that
                    // directly (not via push_with_phys_fallback): its
                    // automatic physical-key fallback would push
                    // `(CTRL, Physical(<key>))`, which is ambiguous with -
                    // and collides with - a plain physical CTRL+<letter>
                    // press with no shift at all (physical keycodes carry
                    // no case/shift information), which is exactly what
                    // broke Ctrl+C previously.
                    keys.push((Modifiers::CTRL, ukey.clone()));
                }
            } else if mods.contains(Modifiers::SHIFT) && ukey != key {
                keys.push((mods, ukey.clone()));
                keys.push((mods - Modifiers::SHIFT, ukey.clone()));
            }
        }

        keys
    }

    /// Produces the list of default key assignments and actions.
    /// Used by the InputMap.
    pub fn default_key_assignments(
        config: &ConfigHandle,
    ) -> Vec<(Modifiers, KeyCode, KeyAssignment)> {
        let mut result = vec![];
        for cmd in Self::expanded_commands(config) {
            for (mods, code) in cmd.keys {
                result.push((mods, code.clone(), cmd.action.clone()));
            }
        }
        result
    }

    fn expand_action(
        action: KeyAssignment,
        config: &ConfigHandle,
        is_built_in: bool,
    ) -> Option<ExpandedCommand> {
        match derive_command_from_key_assignment(&action) {
            None => {
                if is_built_in {
                    log::warn!(
                        "{action:?} is a default action, but we cannot derive a CommandDef for it"
                    );
                }
                None
            }
            Some(def) => {
                let keys = if is_built_in && config.disable_default_key_bindings {
                    vec![]
                } else {
                    def.permute_keys(config)
                };
                Some(ExpandedCommand {
                    brief: def.brief,
                    doc: def.doc,
                    keys,
                    action,
                    menubar: def.menubar,
                    icon: def.icon.map(Cow::Borrowed),
                })
            }
        }
    }

    /// Produces the complete set of expanded commands.
    pub fn expanded_commands(config: &ConfigHandle) -> Vec<ExpandedCommand> {
        let mut result = vec![];

        for action in compute_default_actions() {
            if let Some(command) = Self::expand_action(action, config, true) {
                result.push(command);
            }
        }

        result
    }

    pub fn actions_for_palette_and_menubar(config: &ConfigHandle) -> Vec<ExpandedCommand> {
        let mut result = Self::expanded_commands(config);

        // Generate some stuff based on the config
        for cmd in &config.launch_menu {
            let label = match cmd.label.as_ref() {
                Some(label) => label.to_string(),
                None => match cmd.args.as_ref() {
                    Some(args) => args.join(" "),
                    None => "(default shell)".to_string(),
                },
            };
            result.push(ExpandedCommand {
                brief: format!("{label} (New Tab)").into(),
                doc: "".into(),
                keys: vec![],
                action: KeyAssignment::SpawnCommandInNewTab(cmd.clone()),
                menubar: &["Shell"],
                icon: Some("md_tab_plus".into()),
            });
        }

        // Generate some stuff based on the mux state
        if let Some(mux) = Mux::try_get() {
            let mut domains = mux.iter_domains();
            domains.sort_by(|a, b| {
                let a_state = a.state();
                let b_state = b.state();
                if a_state != b_state {
                    return if a_state == DomainState::Attached {
                        Ordering::Less
                    } else {
                        Ordering::Greater
                    };
                }
                a.domain_id().cmp(&b.domain_id())
            });
            for dom in &domains {
                let name = dom.domain_name();
                // FIXME: use domain_label here, but needs to be async
                let label = name;

                if dom.spawnable() {
                    if dom.state() == DomainState::Attached {
                        result.push(ExpandedCommand {
                            brief: format!("New Tab (Domain {label})").into(),
                            doc: "".into(),
                            keys: vec![],
                            action: KeyAssignment::SpawnCommandInNewTab(SpawnCommand {
                                domain: SpawnTabDomain::DomainName(name.to_string()),
                                ..SpawnCommand::default()
                            }),
                            menubar: &["Shell"],
                            icon: Some("md_tab_plus".into()),
                        });
                    } else {
                        result.push(ExpandedCommand {
                            brief: format!("Attach Domain {label}").into(),
                            doc: "".into(),
                            keys: vec![],
                            action: KeyAssignment::AttachDomain(name.to_string()),
                            menubar: &["Shell", "Attach"],
                            icon: Some("md_pipe".into()),
                        });
                    }
                }
            }
            for dom in &domains {
                let name = dom.domain_name();
                // FIXME: use domain_label here, but needs to be async
                let label = name;

                if dom.state() == DomainState::Attached {
                    if name == "local" {
                        continue;
                    }
                    result.push(ExpandedCommand {
                        brief: format!("Detach Domain {label}").into(),
                        doc: "".into(),
                        keys: vec![],
                        action: KeyAssignment::DetachDomain(SpawnTabDomain::DomainName(
                            name.to_string(),
                        )),
                        menubar: &["Shell", "Detach"],
                        icon: Some("md_pipe_disconnected".into()),
                    });
                }
            }

            let active_workspace = mux.active_workspace();
            for workspace in mux.iter_workspaces() {
                if workspace != active_workspace {
                    result.push(ExpandedCommand {
                        brief: format!("Switch to workspace {workspace}").into(),
                        doc: "".into(),
                        keys: vec![],
                        action: KeyAssignment::SwitchToWorkspace {
                            name: Some(workspace.clone()),
                            spawn: None,
                        },
                        menubar: &["Window", "Workspace"],
                        icon: None,
                    });
                }
            }
            result.push(ExpandedCommand {
                brief: "Create new Workspace".into(),
                doc: "".into(),
                keys: vec![],
                action: KeyAssignment::SwitchToWorkspace {
                    name: None,
                    spawn: None,
                },
                menubar: &["Window", "Workspace"],
                icon: None,
            });
        }

        // And sweep to pick up stuff from their key assignments
        let inputmap = InputMap::new(config);
        for ((keycode, mods), entry) in inputmap.keys.default.iter() {
            if result
                .iter()
                .position(|cmd| cmd.action == entry.action)
                .is_some()
            {
                continue;
            }
            if let Some(cmd) = derive_command_from_key_assignment(&entry.action) {
                result.push(ExpandedCommand {
                    brief: cmd.brief,
                    doc: cmd.doc,
                    keys: vec![(*mods, keycode.clone())],
                    action: entry.action.clone(),
                    menubar: cmd.menubar,
                    icon: cmd.icon.map(Cow::Borrowed),
                });
            }
        }
        for table in inputmap.keys.by_name.values() {
            for entry in table.values() {
                if result
                    .iter()
                    .position(|cmd| cmd.action == entry.action)
                    .is_some()
                {
                    continue;
                }
                if let Some(cmd) = derive_command_from_key_assignment(&entry.action) {
                    result.push(ExpandedCommand {
                        brief: cmd.brief,
                        doc: cmd.doc,
                        keys: vec![],
                        action: entry.action.clone(),
                        menubar: cmd.menubar,
                        icon: cmd.icon.map(Cow::Borrowed),
                    });
                }
            }
        }

        result
    }

    #[cfg(not(target_os = "macos"))]
    pub fn recreate_menubar(_config: &ConfigHandle) {}

    /// Update the menubar to reflect the current config state.
    /// We cannot simply build a completely new one and replace it at runtime,
    /// because something in cocoa get's unhappy and crashes shortly after.
    /// The strategy we have is to try to find the existing item with the
    /// same action and update it.
    /// We use the macos menu item tag to do a mark-sweep style garbage
    /// collection to figure out which items were not reused/updated
    /// and remove them at the end.
    #[cfg(target_os = "macos")]
    pub fn recreate_menubar(config: &ConfigHandle) {
        use window::os::macos::menu::*;

        let inputmap = InputMap::new(config);

        let mut candidates_for_removal = vec![];
        #[allow(unexpected_cfgs)] // <https://github.com/SSheldon/rust-objc/issues/125>
        let wezterm_perform_key_assignment_sel = sel!(weztermPerformKeyAssignment:);

        /// Mark menu items as candidates for removal
        fn mark_candidates(menu: &Menu, candidates: &mut Vec<MenuItem>, action: SEL) {
            for item in menu.items() {
                if let Some(submenu) = item.get_sub_menu() {
                    mark_candidates(&submenu, candidates, action);
                }
                if item.get_action() == Some(action) {
                    item.set_tag(0);
                    candidates.push(item);
                }
            }
        }

        let main_menu = match Menu::get_main_menu() {
            Some(existing) => {
                mark_candidates(
                    &existing,
                    &mut candidates_for_removal,
                    wezterm_perform_key_assignment_sel,
                );

                existing
            }
            None => {
                let menu = Menu::new_with_title("MainMenu");
                menu.assign_as_main_menu();
                menu
            }
        };

        let mut commands = Self::actions_for_palette_and_menubar(config);
        commands.retain(|cmd| !cmd.menubar.is_empty());

        // Prefer to put the menus in this order
        let mut order: Vec<&'static str> = vec!["OnlyTerm", "Shell", "Edit", "View", "Window"];
        // Add any other menus on the end
        for cmd in &commands {
            if !order.contains(&cmd.menubar[0]) {
                order.push(cmd.menubar[0]);
            }
        }

        for &title in &order {
            for cmd in &commands {
                if cmd.menubar[0] != title {
                    continue;
                }

                let mut submenu = main_menu.get_or_create_sub_menu(&cmd.menubar[0], |menu| {
                    if cmd.menubar[0] == "Window" {
                        menu.assign_as_windows_menu();
                        // macOS will insert stuff at the top and bottom, so we add
                        // a separator to tidy things up a bit
                        menu.add_item(&MenuItem::new_separator());
                    } else if cmd.menubar[0] == "OnlyTerm" {
                        menu.assign_as_app_menu();

                        let about_item = MenuItem::new_with(
                            &format!("OnlyTerm {}", config::wezterm_version()),
                            Some(wezterm_perform_key_assignment_sel),
                            "",
                        );
                        about_item.set_tool_tip("Click to copy version number");
                        about_item.set_represented_item(RepresentedItem::KeyAssignment(
                            KeyAssignment::CopyTextTo {
                                text: config::wezterm_version().to_string(),
                                destination: ClipboardCopyDestination::ClipboardAndPrimarySelection,
                            },
                        ));

                        menu.add_item(&about_item);
                        menu.add_item(&MenuItem::new_separator());

                        let services_menu = Menu::new_with_title("Services");
                        services_menu.assign_as_services_menu();
                        let services_item = MenuItem::new_with("Services", None, "");
                        menu.add_item(&services_item);
                        services_item.set_sub_menu(&services_menu);

                        menu.add_item(&MenuItem::new_separator());
                    } else if cmd.menubar[0] == "Help" {
                        menu.assign_as_help_menu();
                    }
                });

                // Fill out any submenu hierarchy
                for sub_title in cmd.menubar.iter().skip(1) {
                    submenu = submenu.get_or_create_sub_menu(sub_title, |_menu| {});
                }

                let mut candidate = inputmap.locate_app_wide_key_assignment(&cmd.action);
                candidate.sort_by(|(a_key, a_mods), (b_key, b_mods)| {
                    fn score_mods(mods: &Modifiers) -> usize {
                        let mut score: usize = mods.bits() as usize;
                        // Prefer keys with CMD on macOS
                        if mods.contains(Modifiers::SUPER) {
                            score += 1000;
                        }
                        score
                    }

                    let a_mods = score_mods(a_mods);
                    let b_mods = score_mods(b_mods);

                    match b_mods.cmp(&a_mods) {
                        Ordering::Equal => {}
                        ordering => return ordering,
                    }

                    a_key.cmp(&b_key)
                });

                fn key_code_to_equivalent(key: &KeyCode) -> String {
                    match key {
                        KeyCode::Hyper
                        | KeyCode::Super
                        | KeyCode::Meta
                        | KeyCode::Cancel
                        | KeyCode::Composed(_)
                        | KeyCode::RawCode(_) => "".to_string(),
                        KeyCode::Char(c) => c.to_string(),
                        KeyCode::Physical(phys) => key_code_to_equivalent(&phys.to_key_code()),
                        _ => "".to_string(),
                    }
                }

                let short_cut = candidate
                    .get(0)
                    .map(|(key, _)| key_code_to_equivalent(key))
                    .unwrap_or_else(String::new);

                let represented_item = RepresentedItem::KeyAssignment(cmd.action.clone());
                let item = match submenu.get_item_with_represented_item(&represented_item) {
                    Some(existing) => {
                        existing.set_title(&cmd.brief);
                        existing.set_key_equivalent(&short_cut);
                        existing
                    }
                    None => {
                        let item = MenuItem::new_with(
                            &cmd.brief,
                            Some(wezterm_perform_key_assignment_sel),
                            &short_cut,
                        );
                        submenu.add_item(&item);
                        item
                    }
                };

                if !short_cut.is_empty() {
                    let mods: Modifiers = candidate[0].1;
                    let mut equiv_mods = NSEventModifierFlags::empty();

                    equiv_mods.set(
                        NSEventModifierFlags::NSShiftKeyMask,
                        mods.contains(Modifiers::SHIFT),
                    );
                    equiv_mods.set(
                        NSEventModifierFlags::NSAlternateKeyMask,
                        mods.contains(Modifiers::ALT),
                    );
                    equiv_mods.set(
                        NSEventModifierFlags::NSControlKeyMask,
                        mods.contains(Modifiers::CTRL),
                    );
                    equiv_mods.set(
                        NSEventModifierFlags::NSCommandKeyMask,
                        mods.contains(Modifiers::SUPER),
                    );

                    item.set_key_equiv_modifier_mask(equiv_mods);
                }

                item.set_represented_item(represented_item);
                item.set_tool_tip(&cmd.doc);
                // Update the tag to indicate that this item should
                // not be removed by the sweep below
                item.set_tag(1);
            }
        }

        // Now sweep away any items that were not updated
        for item in candidates_for_removal {
            if item.get_tag() == 0 {
                item.get_menu().map(|menu| menu.remove_item(&item));
            }
        }
    }
}

/// Returns a list of key assignment actions that should be
/// included in the default key assignments and command palette.
fn compute_default_actions() -> Vec<KeyAssignment> {
    // These are ordered by their position within the various menus
    vec![
        // ----------------- OnlyTerm
        ReloadConfiguration,
        #[cfg(target_os = "macos")]
        HideApplication,
        #[cfg(target_os = "macos")]
        QuitApplication,
        // ----------------- Shell
        SpawnTab(SpawnTabDomain::CurrentPaneDomain),
        SpawnWindow,
        SplitVertical(SpawnCommand {
            domain: SpawnTabDomain::CurrentPaneDomain,
            ..Default::default()
        }),
        SplitHorizontal(SpawnCommand {
            domain: SpawnTabDomain::CurrentPaneDomain,
            ..Default::default()
        }),
        CloseCurrentTab { confirm: true },
        CloseCurrentTab { confirm: false },
        CloseCurrentPane { confirm: true },
        DetachDomain(SpawnTabDomain::CurrentPaneDomain),
        ResetTerminal,
        // ----------------- Edit
        SendEnterOrNewline(Modifiers::CTRL),
        SendEnterOrNewline(Modifiers::SHIFT),
        #[cfg(not(target_os = "macos"))]
        PasteFrom(ClipboardPasteSource::PrimarySelection),
        #[cfg(not(target_os = "macos"))]
        CopyTo(ClipboardCopyDestination::PrimarySelection),
        CopyTo(ClipboardCopyDestination::Clipboard),
        CopySelectionOrInterrupt,
        PasteFrom(ClipboardPasteSource::Clipboard),
        ClearScrollback(ScrollbackEraseMode::ScrollbackOnly),
        ClearScrollback(ScrollbackEraseMode::ScrollbackAndViewport),
        QuickSelect,
        CharSelect(CharSelectArguments::default()),
        ActivateCopyMode,
        ClearKeyTableStack,
        ActivateCommandPalette,
        // ----------------- View
        DecreaseFontSize,
        IncreaseFontSize,
        ResetFontSize,
        ResetFontAndWindowSize,
        ScrollByPage(NotNan::new(-1.0).unwrap()),
        ScrollByPage(NotNan::new(1.0).unwrap()),
        ScrollToTop,
        ScrollToBottom,
        // ----------------- Window
        ToggleFullScreen,
        ToggleAlwaysOnTop,
        ToggleAlwaysOnBottom,
        SetWindowLevel(WindowLevel::AlwaysOnBottom),
        SetWindowLevel(WindowLevel::Normal),
        SetWindowLevel(WindowLevel::AlwaysOnTop),
        Hide,
        Search(Pattern::CurrentSelectionOrEmptyString),
        PaneSelect(PaneSelectArguments {
            alphabet: String::new(),
            mode: PaneSelectMode::Activate,
            show_pane_ids: false,
        }),
        PaneSelect(PaneSelectArguments {
            alphabet: String::new(),
            mode: PaneSelectMode::SwapWithActive,
            show_pane_ids: false,
        }),
        PaneSelect(PaneSelectArguments {
            alphabet: String::new(),
            mode: PaneSelectMode::SwapWithActiveKeepFocus,
            show_pane_ids: false,
        }),
        PaneSelect(PaneSelectArguments {
            alphabet: String::new(),
            mode: PaneSelectMode::MoveToNewTab,
            show_pane_ids: false,
        }),
        PaneSelect(PaneSelectArguments {
            alphabet: String::new(),
            mode: PaneSelectMode::MoveToNewWindow,
            show_pane_ids: false,
        }),
        RotatePanes(RotationDirection::Clockwise),
        RotatePanes(RotationDirection::CounterClockwise),
        ActivateTab(0),
        ActivateTab(1),
        ActivateTab(2),
        ActivateTab(3),
        ActivateTab(4),
        ActivateTab(5),
        ActivateTab(6),
        ActivateTab(7),
        ActivateTab(8),
        ActivateTab(9),
        ActivateTab(-1),
        ActivateTabRelative(-1),
        ActivateTabRelative(1),
        ActivateWindow(0),
        ActivateWindow(1),
        ActivateWindow(2),
        ActivateWindow(3),
        ActivateWindow(4),
        ActivateWindow(5),
        ActivateWindow(6),
        ActivateWindow(7),
        ActivateWindow(8),
        ActivateWindow(9),
        ActivateWindowRelative(-1),
        ActivateWindowRelative(1),
        MoveTabRelative(-1),
        MoveTabRelative(1),
        AdjustPaneSize(PaneDirection::Left, 1),
        AdjustPaneSize(PaneDirection::Right, 1),
        AdjustPaneSize(PaneDirection::Up, 1),
        AdjustPaneSize(PaneDirection::Down, 1),
        ActivatePaneDirection(PaneDirection::Left),
        ActivatePaneDirection(PaneDirection::Right),
        ActivatePaneDirection(PaneDirection::Up),
        ActivatePaneDirection(PaneDirection::Down),
        TogglePaneZoomState,
        ActivateLastTab,
        ShowLauncher,
        ShowTabNavigator,
        // ----------------- Help
        OpenUri("https://wezterm.org/".to_string()),
        OpenUri("https://github.com/wezterm/wezterm/discussions/".to_string()),
        OpenUri("https://github.com/wezterm/wezterm/issues/".to_string()),
        ShowDebugOverlay,
        OpenConfigFile,
        // ----------------- Misc
        OpenLinkAtMouseCursor,
    ]
}
