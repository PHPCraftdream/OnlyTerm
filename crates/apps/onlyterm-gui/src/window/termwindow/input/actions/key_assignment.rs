use super::*;

impl TermWindow {
    /// Returns the Prompt semantic zones
    fn get_semantic_prompt_zones(&mut self, pane: &Arc<dyn Pane>) -> &[StableRowIndex] {
        let cache = self.semantic_zones.entry(pane.pane_id()).or_default();

        let seqno = pane.get_current_seqno();
        if cache.seqno != seqno {
            let zones = pane.get_semantic_zones().unwrap_or_else(|_| vec![]);
            let mut zones: Vec<StableRowIndex> = zones
                .into_iter()
                .filter_map(|zone| {
                    if zone.semantic_type == onlyterm_term::SemanticType::Prompt {
                        Some(zone.start_y)
                    } else {
                        None
                    }
                })
                .collect();
            // dedup to avoid issues where both left and right prompts are
            // defined: we only care if there were 1+ prompts on a line,
            // not about how many prompts are on a line.
            // <https://github.com/wezterm/wezterm/issues/1121>
            zones.dedup();
            cache.zones = zones;
            cache.seqno = seqno;
        }
        &cache.zones
    }

    fn scroll_to_prompt(&mut self, amount: isize, pane: &Arc<dyn Pane>) -> anyhow::Result<()> {
        let dims = pane.get_dimensions();
        let position = self
            .get_viewport(pane.pane_id())
            .unwrap_or(dims.physical_top);
        let zone = {
            let zones = self.get_semantic_prompt_zones(pane);
            let idx = match zones.binary_search(&position) {
                Ok(idx) | Err(idx) => idx,
            };
            let idx = ((idx as isize) + amount).max(0) as usize;
            zones.get(idx).cloned()
        };
        if let Some(zone) = zone {
            self.set_viewport(pane.pane_id(), Some(zone), dims);
        }

        if let Some(win) = self.window.as_ref() {
            win.invalidate();
        }
        Ok(())
    }

    fn scroll_by_page(&mut self, amount: f64, pane: &Arc<dyn Pane>) -> anyhow::Result<()> {
        let dims = pane.get_dimensions();
        let position = self
            .get_viewport(pane.pane_id())
            .unwrap_or(dims.physical_top) as f64
            + (amount * dims.viewport_rows as f64);
        self.set_viewport(pane.pane_id(), Some(position as isize), dims);
        if let Some(win) = self.window.as_ref() {
            win.invalidate();
        }
        Ok(())
    }

    fn scroll_by_current_event_wheel_delta(&mut self, pane: &Arc<dyn Pane>) -> anyhow::Result<()> {
        if let Some(event) = &self.current_mouse_event {
            let amount = match event.kind {
                MouseEventKind::VertWheel(amount) => -amount,
                _ => return Ok(()),
            };
            self.scroll_by_line(amount.into(), pane)?;
        }
        Ok(())
    }

    fn scroll_by_line(&mut self, amount: isize, pane: &Arc<dyn Pane>) -> anyhow::Result<()> {
        let dims = pane.get_dimensions();
        let position = self
            .get_viewport(pane.pane_id())
            .unwrap_or(dims.physical_top)
            .saturating_add(amount);
        self.set_viewport(pane.pane_id(), Some(position), dims);
        if let Some(win) = self.window.as_ref() {
            win.invalidate();
        }
        Ok(())
    }

    fn move_tab_relative(&mut self, delta: isize) -> anyhow::Result<()> {
        let mux = Mux::get();
        let window = mux
            .get_window(self.mux_window_id)
            .ok_or_else(|| anyhow!("no such window"))?;

        let max = window.len();
        ensure!(max > 0, "no more tabs");

        let active = window.get_active_idx();
        let tab = active as isize + delta;
        let tab = if tab < 0 {
            0usize
        } else if tab >= max as isize {
            max - 1
        } else {
            tab as usize
        };

        drop(window);
        self.move_tab(tab)
    }

    pub fn perform_key_assignment(
        &mut self,
        pane: &Arc<dyn Pane>,
        assignment: &KeyAssignment,
    ) -> anyhow::Result<PerformAssignmentResult> {
        use KeyAssignment::*;

        if let Some(modal) = self.get_modal() {
            if modal.perform_assignment(assignment, self) {
                return Ok(PerformAssignmentResult::Handled);
            }
        }

        match pane.perform_assignment(assignment) {
            PerformAssignmentResult::Unhandled => {}
            result => return Ok(result),
        }

        let window = self.window.clone();

        match assignment {
            ActivateKeyTable {
                name,
                timeout_milliseconds,
                replace_current,
                one_shot,
                until_unknown,
                prevent_fallback,
            } => {
                anyhow::ensure!(
                    self.input_map.has_table(name),
                    "ActivateKeyTable: no key_table named {}",
                    name
                );
                self.key_table_state.activate(KeyTableArgs {
                    name,
                    timeout_milliseconds: *timeout_milliseconds,
                    replace_current: *replace_current,
                    one_shot: *one_shot,
                    until_unknown: *until_unknown,
                    prevent_fallback: *prevent_fallback,
                });
                self.update_title();
            }
            PopKeyTable => {
                self.key_table_state.pop();
                self.update_title();
            }
            ClearKeyTableStack => {
                self.key_table_state.clear_stack();
                self.update_title();
            }
            Multiple(actions) => {
                for a in actions {
                    self.perform_key_assignment(pane, a)?;
                }
            }
            SpawnTab(spawn_where) => {
                self.spawn_tab(spawn_where);
            }
            SpawnWindow => {
                self.spawn_command(&SpawnCommand::default(), SpawnWhere::NewWindow);
            }
            SpawnCommandInNewTab(spawn) => {
                self.spawn_command(spawn, SpawnWhere::NewTab);
            }
            SpawnCommandInNewWindow(spawn) => {
                self.spawn_command(spawn, SpawnWhere::NewWindow);
            }
            SplitHorizontal(spawn) => {
                log::trace!("SplitHorizontal {:?}", spawn);
                self.spawn_command(
                    spawn,
                    SpawnWhere::SplitPane(SplitRequest {
                        direction: SplitDirection::Horizontal,
                        target_is_second: true,
                        size: MuxSplitSize::Percent(50),
                        top_level: false,
                    }),
                );
            }
            SplitVertical(spawn) => {
                log::trace!("SplitVertical {:?}", spawn);
                self.spawn_command(
                    spawn,
                    SpawnWhere::SplitPane(SplitRequest {
                        direction: SplitDirection::Vertical,
                        target_is_second: true,
                        size: MuxSplitSize::Percent(50),
                        top_level: false,
                    }),
                );
            }
            ToggleFullScreen => {
                self.window.as_ref().unwrap().toggle_fullscreen();
            }
            ToggleAlwaysOnTop => {
                let window = self.window.clone().unwrap();
                let current_level = self.window_state.as_window_level();

                match current_level {
                    WindowLevel::AlwaysOnTop => {
                        window.set_window_level(WindowLevel::Normal);
                    }
                    WindowLevel::AlwaysOnBottom | WindowLevel::Normal => {
                        window.set_window_level(WindowLevel::AlwaysOnTop);
                    }
                }
            }
            ToggleAlwaysOnBottom => {
                let window = self.window.clone().unwrap();
                let current_level = self.window_state.as_window_level();

                match current_level {
                    WindowLevel::AlwaysOnBottom => {
                        window.set_window_level(WindowLevel::Normal);
                    }
                    WindowLevel::AlwaysOnTop | WindowLevel::Normal => {
                        window.set_window_level(WindowLevel::AlwaysOnBottom);
                    }
                }
            }
            SetWindowLevel(level) => {
                let window = self.window.clone().unwrap();
                window.set_window_level(level.clone());
            }
            CopyTo(dest) => {
                let text = self.selection_text(pane);
                self.copy_to_clipboard(*dest, text);
            }
            CopySelectionOrInterrupt => {
                let text = self.selection_text(pane);
                if !text.is_empty() {
                    self.copy_to_clipboard(ClipboardCopyDestination::Clipboard, text);
                    self.clear_selection(pane);
                } else {
                    // Route through whatever keyboard protocol the app in
                    // the pane has negotiated (win32-input-mode or kitty),
                    // rather than always writing the legacy `\x03` byte: an
                    // app that asked for eg. win32-input-mode (confirmed via
                    // live escape-sequence capture to be what Codex CLI
                    // negotiates) expects Ctrl+C in that app's requested
                    // form and may not treat a bare `\x03` as an interrupt
                    // while that mode is active.
                    let event = synthetic_key_down(KeyCode::Char('c'), Modifiers::CTRL);
                    let encoded = self.encode_via_negotiated_protocol(pane, &event);
                    // Logged at info for the same reason as SendChar below:
                    // "Ctrl+C did not interrupt" has two completely different
                    // causes -- the binding never matched so this arm never
                    // ran, or it ran and the app ignored what we sent -- and
                    // they are indistinguishable to the user.
                    log::info!(
                        "CopySelectionOrInterrupt (no selection) on pane {}: keyboard encoding \
                         is {:?}, allow_win32_input_mode={}, enable_kitty_keyboard={}, sending {}",
                        pane.pane_id(),
                        pane.get_keyboard_encoding(),
                        self.config.allow_win32_input_mode,
                        self.config.enable_kitty_keyboard,
                        match &encoded {
                            Some(encoded) => format!("encoded {:?}", encoded),
                            None => "legacy \\x03".to_string(),
                        }
                    );
                    match encoded {
                        Some(encoded) => {
                            pane.writer().write_all(encoded.as_bytes()).ok();
                        }
                        None => {
                            pane.writer().write_all(b"\x03").ok();
                        }
                    }
                }
            }
            SendEnterOrNewline(mods) => {
                // See `CopySelectionOrInterrupt` above: route through
                // whatever keyboard protocol the app has negotiated so
                // eg. Codex CLI (which negotiates win32-input-mode via
                // DECSET, confirmed via live escape-sequence capture --
                // it never attempts kitty keyboard protocol) gets the
                // modified-Enter form it expects, instead of a hardcoded
                // '\n' unconditionally masking the chord from ever
                // reaching that negotiation. Only apps that haven't
                // negotiated such a protocol get the '\n' fallback, since
                // that's the best a plain/legacy app can do with this
                // chord.
                // An application that reads the byte stream instead of console
                // records cannot see this chord at all: conhost renders the
                // modified-Enter record down to a bare `\r`, identical to
                // plain Enter, so the keypress submits rather than inserting a
                // newline. Those applications recognise ESC CR for "newline,
                // not submit" -- see `shift_enter_esc_cr_processes`, and note
                // it must stay narrow, since Codex CLI resolves this chord by
                // virtual key and needs the faithful record.
                let esc_cr = mods.contains(Modifiers::SHIFT)
                    && !mods.contains(Modifiers::CTRL)
                    && self.shift_enter_esc_cr_for(pane);
                if esc_cr {
                    log::info!(
                        "SendEnterOrNewline({:?}) on pane {}: sending ESC CR for a \
                         byte-stream reader",
                        mods,
                        pane.pane_id(),
                    );
                    pane.writer().write_all(b"\x1b\r").ok();
                    return Ok(PerformAssignmentResult::Handled);
                }
                let event = synthetic_key_down(KeyCode::Char('\r'), *mods);
                let encoded = self.encode_via_negotiated_protocol(pane, &event);
                // Logged at info (not trace) deliberately: this chord is rare
                // and user-initiated, and which of the two branches below
                // fires -- plus what the pane claims to have negotiated -- is
                // the single most useful fact when a multi-line prompt (Codex
                // CLI and friends) refuses to insert a newline. The same
                // keypress can submit, do nothing, or insert a newline
                // depending entirely on this decision, and the pane's
                // encoding is now sourced from the *remote* terminal for
                // hosted tabs, so it cannot be inferred from local state.
                log::info!(
                    "SendEnterOrNewline({:?}) on pane {}: keyboard encoding is {:?}, \
                     allow_win32_input_mode={}, enable_kitty_keyboard={}, sending {}",
                    mods,
                    pane.pane_id(),
                    pane.get_keyboard_encoding(),
                    self.config.allow_win32_input_mode,
                    self.config.enable_kitty_keyboard,
                    match &encoded {
                        Some(seq) => format!("encoded {:?}", seq),
                        None => "a literal LF (no protocol negotiated)".to_string(),
                    }
                );
                match encoded {
                    Some(encoded) => {
                        pane.writer().write_all(encoded.as_bytes()).ok();
                    }
                    None => {
                        pane.writer().write_all(b"\n").ok();
                    }
                }
            }
            SendChar(mods, c) => {
                // See `SendEnterOrNewline` above: route through whatever
                // keyboard protocol the app has negotiated so a modified
                // chord like CTRL+j is distinguishable from a bare 'j' once
                // decoded by the app, instead of a hardcoded byte
                // unconditionally masking the chord from ever reaching that
                // negotiation. The modifiers are part of the synthetic
                // event -- dropping them here would silently turn eg.
                // CTRL+j into a plain 'j' keypress for any app that HAS
                // negotiated a protocol, which defeats the point of this
                // action.
                let event = synthetic_key_down(KeyCode::Char(*c), *mods);
                let encoded = self.encode_via_negotiated_protocol(pane, &event);
                // See `SendEnterOrNewline` above for why this is logged at
                // info: same rationale, same "which branch fired" question.
                log::info!(
                    "SendChar({:?}, {:?}) on pane {}: keyboard encoding is {:?}, \
                     allow_win32_input_mode={}, enable_kitty_keyboard={}, sending {}",
                    mods,
                    c,
                    pane.pane_id(),
                    pane.get_keyboard_encoding(),
                    self.config.allow_win32_input_mode,
                    self.config.enable_kitty_keyboard,
                    match &encoded {
                        Some(seq) => format!("encoded {:?}", seq),
                        None => "the raw/control-code fallback byte (no protocol negotiated)"
                            .to_string(),
                    }
                );
                match encoded {
                    Some(encoded) => {
                        pane.writer().write_all(encoded.as_bytes()).ok();
                    }
                    None => {
                        // No protocol negotiated: fall back to the standard
                        // ASCII control-code encoding when CTRL is held
                        // (eg. CTRL+j -> 0x0A), matching what a plain/legacy
                        // app would already get from this chord if nothing
                        // had bound it at all -- otherwise send the raw
                        // character byte.
                        let byte = if mods.contains(Modifiers::CTRL) {
                            ctrl_mapping(*c).map(|c| c as u8).unwrap_or(*c as u8)
                        } else {
                            *c as u8
                        };
                        pane.writer().write_all(&[byte]).ok();
                    }
                }
            }
            CopyTextTo { text, destination } => {
                self.copy_to_clipboard(*destination, text.clone());
            }
            PasteFrom(source) => {
                self.paste_from_clipboard(pane, *source);
            }
            ActivateTabRelative(n) => {
                self.activate_tab_relative(*n, true)?;
            }
            ActivateTabRelativeNoWrap(n) => {
                self.activate_tab_relative(*n, false)?;
            }
            ActivateLastTab => self.activate_last_tab()?,
            DecreaseFontSize => self.decrease_font_size(),
            IncreaseFontSize => self.increase_font_size(),
            ResetFontSize => self.reset_font_size(),
            ResetFontAndWindowSize => {
                if let Some(w) = window.as_ref() {
                    self.reset_font_and_window_size(w)?
                }
            }
            ActivateTab(n) => {
                self.activate_tab(*n)?;
            }
            ActivateWindow(n) => {
                self.activate_window(*n)?;
            }
            ActivateWindowRelative(n) => {
                self.activate_window_relative(*n, true)?;
            }
            ActivateWindowRelativeNoWrap(n) => {
                self.activate_window_relative(*n, false)?;
            }
            SendString(s) => pane.writer().write_all(s.as_bytes())?,
            SendKey(key) => {
                use keyevent::Key;
                let mods = key.mods;
                if let Key::Code(key) = self.win_key_code_to_termwiz_key_code(
                    &key.key.resolve(self.config.key_map_preference),
                ) {
                    pane.key_down(key, mods)?;
                }
            }
            Hide => {
                if let Some(w) = window.as_ref() {
                    w.hide();
                }
            }
            Show => {
                if let Some(w) = window.as_ref() {
                    w.show();
                }
            }
            CloseCurrentTab { confirm } => self.close_current_tab(*confirm),
            CloseCurrentPane { confirm } => self.close_current_pane(*confirm),
            Nop | DisableDefaultAssignment => {}
            ReloadConfiguration => onlyterm_config::reload(),
            MoveTab(n) => self.move_tab(*n)?,
            MoveTabRelative(n) => self.move_tab_relative(*n)?,
            RenameCurrentTab => self.rename_current_tab(),
            ScrollByPage(n) => self.scroll_by_page(**n, pane)?,
            ScrollByLine(n) => self.scroll_by_line(*n, pane)?,
            ScrollByCurrentEventWheelDelta => self.scroll_by_current_event_wheel_delta(pane)?,
            ScrollToPrompt(n) => self.scroll_to_prompt(*n, pane)?,
            ScrollToTop => self.scroll_to_top(pane),
            ScrollToBottom => self.scroll_to_bottom(pane),
            ShowTabNavigator => self.show_tab_navigator(),
            ShowDebugOverlay => self.show_debug_overlay(),
            ShowVersionOverlay => self.show_version_overlay(),
            ShowLauncherArgs(args) => {
                let title = args.title.clone().unwrap_or("Launcher".to_string());
                let args = LauncherActionArgs {
                    title: Some(title),
                    flags: args.flags,
                    help_text: args.help_text.clone(),
                    fuzzy_help_text: args.fuzzy_help_text.clone(),
                    alphabet: args.alphabet.clone(),
                };
                self.show_launcher_impl(args, 0);
            }
            HideApplication => {
                let con = Connection::get().expect("call on gui thread");
                con.hide_application();
            }
            // OnlyTerm: never prompt on quit - close-confirmation overlays
            // are removed entirely, not just defaulted off via config.
            QuitApplication => {
                log::info!("QuitApplication over here (window)");
                let con = Connection::get().expect("call on gui thread");
                con.terminate_message_loop();
            }
            SelectTextAtMouseCursor(mode) => self.select_text_at_mouse_cursor(*mode, pane),
            ExtendSelectionToMouseCursor(mode) => {
                self.extend_selection_at_mouse_cursor(*mode, pane)
            }
            ClearSelection => {
                self.clear_selection(pane);
            }
            StartWindowDrag => {
                self.window_drag_position = self.current_mouse_event.clone();
            }
            OpenLinkAtMouseCursor => {
                self.do_open_link_at_mouse_cursor(pane);
            }
            CopyLinkAtMouseCursor(destination) => {
                // Right-click's default binding. If there's a hyperlink
                // under the cursor, copy its URL (existing behavior).
                // Otherwise, if there's a text selection, copy it to the
                // clipboard and clear the selection - matching the same
                // copy-then-clear pattern as CTRL+C's
                // CopySelectionOrInterrupt. Unlike left-click's
                // CompleteSelectionOrOpenLinkAtMouseCursor, this is an
                // explicit action to end the selection, so clearing here
                // is correct.
                if self.current_highlight.is_some() {
                    self.do_copy_link_at_mouse_cursor(*destination);
                } else {
                    let text = self.selection_text(pane);
                    if !text.is_empty() {
                        self.copy_to_clipboard(*destination, text);
                        self.clear_selection(pane);
                        if let Some(window) = self.window.as_ref() {
                            window.invalidate();
                        }
                    }
                }
            }
            EmitEvent(name) => {
                self.emit_window_event(name, None);
            }
            CompleteSelectionOrOpenLinkAtMouseCursor(dest) => {
                // Releasing the mouse button after a drag-select must leave
                // the selection visible: it should only go away when the
                // user clicks elsewhere (handled by `begin()` resetting the
                // range on the next mouse-down), right-clicks it (copies and
                // clears, see CopyLinkAtMouseCursor above), or presses
                // Ctrl+C (CopySelectionOrInterrupt). So, unlike those two,
                // this handler must not clear the selection itself.
                let text = self.selection_text(pane);
                if !text.is_empty() {
                    self.copy_to_clipboard(*dest, text);
                    let window = self.window.as_ref().unwrap();
                    window.invalidate();
                } else {
                    self.do_open_link_at_mouse_cursor(pane);
                }
            }
            CompleteSelection(dest) => {
                let text = self.selection_text(pane);
                if !text.is_empty() {
                    self.copy_to_clipboard(*dest, text);
                    let window = self.window.as_ref().unwrap();
                    window.invalidate();
                }
            }
            ClearScrollback(erase_mode) => {
                pane.erase_scrollback(*erase_mode);
                let window = self.window.as_ref().unwrap();
                window.invalidate();
            }
            Search(pattern) => {
                if let Some(pane) = self.get_active_pane_or_overlay() {
                    let mut replace_current = false;
                    if let Some(existing) = pane.downcast_ref::<CopyOverlay>() {
                        let mut params = existing.get_params();
                        params.editing_search = true;
                        if !pattern.is_empty() {
                            params.pattern = self.resolve_search_pattern(pattern.clone(), &pane);
                        }
                        existing.apply_params(params);
                        replace_current = true;
                    } else {
                        let search = CopyOverlay::with_pane(
                            self,
                            &pane,
                            CopyModeParams {
                                pattern: self.resolve_search_pattern(pattern.clone(), &pane),
                                editing_search: true,
                            },
                        )?;
                        self.assign_overlay_for_pane(pane.pane_id(), search);
                    }
                    if let Some(overlay) = self.pane_state(pane.pane_id()).overlay.as_mut() {
                        overlay.key_table_state.activate(KeyTableArgs {
                            name: "search_mode",
                            timeout_milliseconds: None,
                            replace_current,
                            one_shot: false,
                            until_unknown: false,
                            prevent_fallback: false,
                        });
                    }
                }
            }
            QuickSelect => {
                if let Some(pane) = self.get_active_pane_no_overlay() {
                    let qa = QuickSelectOverlay::with_pane(
                        self,
                        &pane,
                        &QuickSelectArguments::default(),
                    );
                    self.assign_overlay_for_pane(pane.pane_id(), qa);
                }
            }
            QuickSelectArgs(args) => {
                if let Some(pane) = self.get_active_pane_no_overlay() {
                    let qa = QuickSelectOverlay::with_pane(self, &pane, args);
                    self.assign_overlay_for_pane(pane.pane_id(), qa);
                }
            }
            ActivateCopyMode => {
                if let Some(pane) = self.get_active_pane_or_overlay() {
                    let mut replace_current = false;
                    if let Some(existing) = pane.downcast_ref::<CopyOverlay>() {
                        let mut params = existing.get_params();
                        params.editing_search = false;
                        existing.apply_params(params);
                        replace_current = true;
                    } else {
                        let copy = CopyOverlay::with_pane(
                            self,
                            &pane,
                            CopyModeParams {
                                pattern: MuxPattern::default(),
                                editing_search: false,
                            },
                        )?;
                        self.assign_overlay_for_pane(pane.pane_id(), copy);
                    }
                    if let Some(overlay) = self.pane_state(pane.pane_id()).overlay.as_mut() {
                        overlay.key_table_state.activate(KeyTableArgs {
                            name: "copy_mode",
                            timeout_milliseconds: None,
                            replace_current,
                            one_shot: false,
                            until_unknown: false,
                            prevent_fallback: false,
                        });
                    }
                }
            }
            AdjustPaneSize(direction, amount) => {
                let mux = Mux::get();
                let tab = match mux.get_active_tab_for_window(self.mux_window_id) {
                    Some(tab) => tab,
                    None => return Ok(PerformAssignmentResult::Handled),
                };

                let tab_id = tab.tab_id();

                if self.tab_state(tab_id).overlay.is_none() {
                    tab.adjust_pane_size(*direction, *amount);
                }
            }
            ActivatePaneByIndex(index) => {
                let mux = Mux::get();
                let tab = match mux.get_active_tab_for_window(self.mux_window_id) {
                    Some(tab) => tab,
                    None => return Ok(PerformAssignmentResult::Handled),
                };

                let tab_id = tab.tab_id();

                if self.tab_state(tab_id).overlay.is_none() {
                    let panes = tab.iter_panes();
                    if panes.iter().position(|p| p.index == *index).is_some() {
                        tab.set_active_idx(*index);
                    }
                }
            }
            ActivatePaneDirection(direction) => {
                let mux = Mux::get();
                let tab = match mux.get_active_tab_for_window(self.mux_window_id) {
                    Some(tab) => tab,
                    None => return Ok(PerformAssignmentResult::Handled),
                };

                let tab_id = tab.tab_id();

                if self.tab_state(tab_id).overlay.is_none() {
                    tab.activate_pane_direction(*direction);
                }
            }
            TogglePaneZoomState => {
                let mux = Mux::get();
                let tab = match mux.get_active_tab_for_window(self.mux_window_id) {
                    Some(tab) => tab,
                    None => return Ok(PerformAssignmentResult::Handled),
                };
                tab.toggle_zoom();
            }
            SetPaneZoomState(zoomed) => {
                let mux = Mux::get();
                let tab = match mux.get_active_tab_for_window(self.mux_window_id) {
                    Some(tab) => tab,
                    None => return Ok(PerformAssignmentResult::Handled),
                };
                tab.set_zoomed(*zoomed);
            }
            SwitchWorkspaceRelative(delta) => {
                let mux = Mux::get();
                let workspace = mux.active_workspace();
                let workspaces = mux.iter_workspaces();
                let idx = workspaces.iter().position(|w| *w == workspace).unwrap_or(0);
                let new_idx = idx as isize + delta;
                let new_idx = if new_idx < 0 {
                    workspaces.len() as isize + new_idx
                } else {
                    new_idx
                };
                let new_idx = new_idx as usize % workspaces.len();
                if let Some(w) = workspaces.get(new_idx) {
                    front_end().switch_workspace(w);
                }
            }
            SwitchToWorkspace { name, spawn } => {
                let activity = crate::Activity::new();
                let mux = Mux::get();
                let name = name
                    .as_ref()
                    .map(|name| name.to_string())
                    .unwrap_or_else(|| mux.generate_workspace_name());
                let switcher = crate::frontend::WorkspaceSwitcher::new(&name);
                mux.set_active_workspace(&name);

                if mux.iter_windows_in_workspace(&name).is_empty() {
                    let spawn = spawn.clone().unwrap_or_default();
                    let size = self.terminal_size;
                    let term_config = Arc::new(TermConfig::with_config(self.config.clone()));
                    let src_window_id = self.mux_window_id;

                    onlyterm_promise::spawn::spawn(async move {
                        if let Err(err) = crate::spawn::spawn_command_internal(
                            spawn,
                            SpawnWhere::NewWindow,
                            size,
                            Some(src_window_id),
                            term_config,
                        )
                        .await
                        {
                            log::error!("Failed to spawn: {:#}", err);
                        }
                        switcher.do_switch();
                        drop(activity);
                    })
                    .detach();
                } else {
                    switcher.do_switch();
                }
            }
            DetachDomain(domain) => {
                let domain = Mux::get().resolve_spawn_tab_domain(Some(pane.pane_id()), domain)?;
                domain.detach()?;
            }
            AttachDomain(domain) => {
                let window = self.mux_window_id;
                let domain = domain.to_string();
                let dpi = self.dimensions.dpi as u32;

                onlyterm_promise::spawn::spawn(async move {
                    let mux = Mux::get();
                    let domain = mux
                        .get_domain_by_name(&domain)
                        .ok_or_else(|| anyhow!("{} is not a valid domain name", domain))?;
                    domain.attach(Some(window)).await?;

                    let have_panes_in_domain = mux
                        .iter_panes()
                        .iter()
                        .any(|p| p.domain_id() == domain.domain_id());

                    if !have_panes_in_domain {
                        let config = onlyterm_config::configuration();
                        let _tab = domain
                            .spawn(
                                config.initial_size(
                                    dpi,
                                    Some(crate::cell_pixel_dims(&config, dpi as f64)?),
                                ),
                                None,
                                None,
                                window,
                            )
                            .await?;
                    }

                    Result::<(), anyhow::Error>::Ok(())
                })
                .detach();
            }
            CopyMode(_) => {
                // NOP here; handled by the overlay directly
            }
            RotatePanes(direction) => {
                let mux = Mux::get();
                let tab = match mux.get_active_tab_for_window(self.mux_window_id) {
                    Some(tab) => tab,
                    None => return Ok(PerformAssignmentResult::Handled),
                };
                let tab_id = tab.tab_id();
                let direction = *direction;
                onlyterm_promise::spawn::spawn(async move {
                    let mux = Mux::get();
                    if let Err(err) = mux.rotate_panes(tab_id, direction).await {
                        log::error!("Unable to rotate panes: {:#}", err);
                    }
                })
                .detach()
            }
            SplitPane(split) => {
                log::trace!("SplitPane {:?}", split);
                self.spawn_command(
                    &split.command,
                    SpawnWhere::SplitPane(SplitRequest {
                        direction: match split.direction {
                            PaneDirection::Down | PaneDirection::Up => SplitDirection::Vertical,
                            PaneDirection::Left | PaneDirection::Right => {
                                SplitDirection::Horizontal
                            }
                            PaneDirection::Next | PaneDirection::Prev => {
                                log::error!(
                                    "Invalid direction {:?} for SplitPane",
                                    split.direction
                                );
                                return Ok(PerformAssignmentResult::Handled);
                            }
                        },
                        target_is_second: match split.direction {
                            PaneDirection::Down | PaneDirection::Right => true,
                            PaneDirection::Up | PaneDirection::Left => false,
                            PaneDirection::Next | PaneDirection::Prev => unreachable!(),
                        },
                        size: match split.size {
                            SplitSize::Percent(n) => MuxSplitSize::Percent(n),
                            SplitSize::Cells(n) => MuxSplitSize::Cells(n),
                        },
                        top_level: split.top_level,
                    }),
                );
            }
            PaneSelect(args) => {
                let modal = crate::termwindow::paneselect::PaneSelector::new(self, args);
                self.set_modal(Rc::new(modal));
            }
            CharSelect(args) => {
                let modal = crate::termwindow::charselect::CharSelector::new(self, args);
                self.set_modal(Rc::new(modal));
            }
            ResetTerminal => {
                pane.perform_actions(vec![termwiz::escape::Action::Esc(
                    termwiz::escape::Esc::Code(termwiz::escape::EscCode::FullReset),
                )]);
            }
            OpenUri(link) => {
                onlyterm_open_url::open_url(link);
            }
            OpenConfigFile => {
                if let Err(err) = open_config_file() {
                    log::error!("OpenConfigFile: {err:#}");
                    onlyterm_toast_notification::persistent_toast_notification(
                        "OnlyTerm",
                        &format!("Couldn't open the config file: {err:#}"),
                    );
                }
            }
            ActivateCommandPalette => {
                let modal = crate::termwindow::palette::CommandPalette::new(self);
                self.set_modal(Rc::new(modal));
            }
            ActivateNewTabOptions => {
                let modal = crate::termwindow::newtab_options::NewTabOptions::new();
                self.set_modal(Rc::new(modal));
            }
            ActivatePaneLayoutMenu => {
                if self.get_modal().is_none()
                    && Mux::get()
                        .get_active_tab_for_window(self.mux_window_id)
                        .is_some()
                {
                    let menu = crate::termwindow::pane_layout_menu::PaneLayoutMenu::new();
                    self.set_modal(Rc::new(menu));
                }
            }
            PromptInputLine(args) => self.show_prompt_input_line(args),
            InputSelector(args) => self.show_input_selector(args),
            Confirmation(args) => self.show_confirmation(args),
        };
        Ok(PerformAssignmentResult::Handled)
    }
}
