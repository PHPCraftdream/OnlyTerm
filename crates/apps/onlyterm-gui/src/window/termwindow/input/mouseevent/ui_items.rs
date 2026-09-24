use super::*;

impl super::super::TermWindow {
    /// `pane` is optional because a `--choose-tab` window has none until the
    /// user presses Run; only the scrollbar items genuinely need one, and they
    /// cannot be on screen in that state anyway.
    pub(super) fn mouse_event_ui_item(
        &mut self,
        item: UIItem,
        pane: Option<Arc<dyn Pane>>,
        _y: i64,
        event: MouseEvent,
        context: &dyn WindowOps,
    ) {
        self.last_ui_item.replace(item.clone());
        match item.item_type {
            UIItemType::TabBar(tab_bar_item) => {
                self.mouse_event_tab_bar(item, tab_bar_item, event, context);
            }
            UIItemType::AboveScrollThumb => {
                if let Some(pane) = pane {
                    self.mouse_event_above_scroll_thumb(item, pane, event, context);
                }
            }
            UIItemType::ScrollThumb => {
                if let Some(pane) = pane {
                    self.mouse_event_scroll_thumb(item, pane, event, context);
                }
            }
            UIItemType::BelowScrollThumb => {
                if let Some(pane) = pane {
                    self.mouse_event_below_scroll_thumb(item, pane, event, context);
                }
            }
            UIItemType::Split(split) => {
                self.mouse_event_split(item, split, event, context);
            }
            UIItemType::CloseTab(idx) => {
                self.mouse_event_close_tab(idx, event, context);
            }
            UIItemType::NewTabOptionRadio { group, choice } => {
                self.mouse_event_newtab_options_radio(item, group, choice, event, context);
            }
            UIItemType::NewTabOptionRun => {
                self.mouse_event_newtab_options_run(item, event, context);
            }
            UIItemType::NewTabOptionClose => {
                self.mouse_event_newtab_options_close(item, event, context);
            }
            UIItemType::PaneLayoutMenuItem(number) => {
                self.mouse_event_pane_layout_menu_item(number, event, context);
            }
        }
    }

    fn mouse_event_pane_layout_menu_item(
        &mut self,
        number: u8,
        event: MouseEvent,
        context: &dyn WindowOps,
    ) {
        if let WMEK::Press(MousePress::Left) = event.kind {
            let active = self.get_modal().is_some_and(|modal| {
                modal
                    .downcast_ref::<crate::termwindow::pane_layout_menu::PaneLayoutMenu>()
                    .is_some()
            });
            if active {
                if let Some(choice) =
                    crate::termwindow::pane_layout_menu::PaneLayoutChoice::from_number(number)
                {
                    self.perform_pane_layout_choice(choice);
                }
            }
        }
        context.set_cursor(Some(MouseCursor::Hand));
    }

    fn mouse_event_newtab_options_radio(
        &mut self,
        _item: UIItem,
        group: crate::termwindow::NewTabOptionGroup,
        choice: usize,
        event: MouseEvent,
        context: &dyn WindowOps,
    ) {
        log::trace!(
            "diag: mouse_event_newtab_options_radio group={:?} choice={} kind={:?}",
            group,
            choice,
            event.kind
        );
        if let WMEK::Press(MousePress::Left) = event.kind {
            use crate::termwindow::newtab_options::NewTabOptions;
            // Scope the RefCell borrow tightly: `invalidate_modal` below
            // needs `&mut self`, which can't be called while a `Ref`
            // derived from `self.modal.borrow()` is still alive.
            let handled = {
                let modal = self.modal.borrow();
                match modal
                    .as_ref()
                    .and_then(|m| m.downcast_ref::<NewTabOptions>())
                {
                    Some(newtab) => {
                        newtab.handle_selection(group, choice);
                        true
                    }
                    None => false,
                }
            };
            if handled {
                // `context.invalidate()` alone repaints with the *cached*
                // computed_element and would silently not show the new
                // selection; `invalidate_modal` is what actually clears
                // that cache (via `Modal::reconfigure`) before repainting.
                self.invalidate_modal();
            } else {
                log::trace!("diag: no active NewTabOptions modal to handle radio click");
            }
        }
        context.set_cursor(Some(MouseCursor::Hand));
    }

    fn mouse_event_newtab_options_run(
        &mut self,
        _item: UIItem,
        event: MouseEvent,
        context: &dyn WindowOps,
    ) {
        if let WMEK::Press(MousePress::Left) = event.kind {
            use crate::termwindow::newtab_options::{execute_new_tab_run_request, NewTabOptions};
            // Scoped the same way as the radio handler above: `run()`
            // only reads the current selections into an owned request,
            // so the `Ref` from `self.modal.borrow()` can drop before
            // `execute_new_tab_run_request` needs `&mut self`.
            let request = {
                let modal = self.modal.borrow();
                modal
                    .as_ref()
                    .and_then(|m| m.downcast_ref::<NewTabOptions>())
                    .map(|newtab| newtab.run())
            };
            if let Some(request) = request {
                self.cancel_modal();
                execute_new_tab_run_request(self, request);
            }
        }
        context.set_cursor(Some(MouseCursor::Hand));
    }

    /// The dialog's close cross. Deliberately routes through the same
    /// `perform_dismiss` that Esc uses, so the two dismissal paths cannot
    /// drift apart -- notably, neither of them starts a tab, and either may
    /// end the process when the dialog was opened by `--choose-tab`.
    fn mouse_event_newtab_options_close(
        &mut self,
        _item: UIItem,
        event: MouseEvent,
        context: &dyn WindowOps,
    ) {
        if let WMEK::Press(MousePress::Left) = event.kind {
            use crate::termwindow::newtab_options::{perform_dismiss, NewTabOptions, OnCancel};
            // Read the choice and release the borrow before dispatching:
            // `perform_dismiss` may call `cancel_modal`, which takes
            // `borrow_mut()` on this very `RefCell`. Holding the `Ref` across
            // that call is a guaranteed `BorrowMutError`, not a rare race.
            let on_cancel = {
                let modal = self.modal.borrow();
                modal
                    .as_ref()
                    .and_then(|m| m.downcast_ref::<NewTabOptions>())
                    .map(|newtab| newtab.on_cancel())
            };
            // No dialog to ask means nothing to quit for: dismiss, as before.
            perform_dismiss(on_cancel.unwrap_or(OnCancel::Dismiss), self);
        }
        context.set_cursor(Some(MouseCursor::Hand));
    }

    pub fn mouse_event_close_tab(
        &mut self,
        idx: usize,
        event: MouseEvent,
        context: &dyn WindowOps,
    ) {
        if let WMEK::Press(MousePress::Left) = event.kind {
            log::debug!("Should close tab {}", idx);
            self.close_specific_tab(idx, true);
        }
        context.set_cursor(Some(MouseCursor::Arrow));
    }

    /// The `new-tab-button-click` event used to be dispatched to a rhai
    /// handler registered via `onlyterm.on("new-tab-button-click", ...)`,
    /// which could return `false` to suppress `action` (the built-in
    /// default: spawn a new tab in the same domain on left-click, nothing
    /// on middle-click). With the scripting layer removed there is no
    /// handler left to suppress it, so the default action now always
    /// runs. Right-click used to show the full command launcher here,
    /// but that overlay's actual "new tab" content (the current domain,
    /// since SSH/WSL domains were removed from this fork) was a single
    /// duplicate of what left-click already does, buried under ~70
    /// unrelated app-wide commands -- removed as pure noise earlier this
    /// session. Right-click now opens the purpose-built "New Tab
    /// Options" dialog (shell/elevation/priority) instead, reusing the
    /// gesture for something that's actually about starting a new tab.
    fn do_new_tab_button_click(&mut self, button: MousePress) {
        let pane = match self.get_active_pane_or_overlay() {
            Some(pane) => pane,
            None => return,
        };
        let action = match button {
            MousePress::Left => Some(KeyAssignment::SpawnTab(SpawnTabDomain::CurrentPaneDomain)),
            MousePress::Right => Some(KeyAssignment::ActivateNewTabOptions),
            MousePress::Middle => None,
        };

        if let Some(assignment) = action {
            let window = GuiWin::new(self);
            let pane = MuxPane(pane.pane_id());
            window.window.notify(TermWindowNotif::PerformAssignment {
                pane_id: pane.0,
                assignment,
                tx: None,
            });
        }
    }

    pub fn mouse_event_tab_bar(
        &mut self,
        ui_item: UIItem,
        item: TabBarItem,
        event: MouseEvent,
        context: &dyn WindowOps,
    ) {
        match event.kind {
            WMEK::Press(MousePress::Left) => match item {
                TabBarItem::Tab { tab_idx, .. } => {
                    self.activate_tab(tab_idx as isize).ok();
                    if self.last_mouse_click.as_ref().map(|c| c.streak) == Some(2) {
                        // Double-click: same rename prompt as the F2
                        // keybinding (task #430), not a drag. Skip arming
                        // `self.dragging` below so the second press of the
                        // double-click doesn't also start (and immediately
                        // no-op) a reorder drag.
                        self.rename_current_tab();
                    } else {
                        // Arm a potential drag-to-reorder: if the next Move
                        // event before mouse-up lands over a different tab,
                        // drag_ui_item's UIItemType::TabBar branch moves this
                        // (now active) tab there via the same move_tab used by
                        // the MoveTab/MoveTabRelative key assignments. A plain
                        // click (press immediately followed by release, no
                        // intervening Move past this tab) never reaches
                        // drag_tab, so it's a no-op beyond the activation above.
                        self.dragging = Some((ui_item, event));
                    }
                }
                TabBarItem::NewTabButton => {
                    self.do_new_tab_button_click(MousePress::Left);
                }
                TabBarItem::None | TabBarItem::LeftStatus | TabBarItem::RightStatus => {
                    let maximized = self
                        .window_state
                        .intersects(WindowState::MAXIMIZED | WindowState::FULL_SCREEN);
                    if let Some(ref window) = self.window {
                        if self.config.window_decorations
                            == WindowDecorations::INTEGRATED_BUTTONS | WindowDecorations::RESIZE
                            && self.last_mouse_click.as_ref().map(|c| c.streak) == Some(2)
                        {
                            if maximized {
                                window.restore();
                            } else {
                                window.maximize();
                            }
                        }
                    }
                    // Potentially starting a drag by the tab bar
                    if !maximized {
                        self.window_drag_position.replace(event.clone());
                    }
                    context.request_drag_move();
                }
                TabBarItem::WindowButton(button) => {
                    use window::IntegratedTitleButton as Button;
                    if let Some(ref window) = self.window {
                        match button {
                            Button::Hide => window.hide(),
                            Button::Maximize => {
                                let maximized = self
                                    .window_state
                                    .intersects(WindowState::MAXIMIZED | WindowState::FULL_SCREEN);
                                if maximized {
                                    window.restore();
                                } else {
                                    window.maximize();
                                }
                            }
                            Button::Close => self.close_requested(&window.clone()),
                        }
                    }
                }
            },
            WMEK::Press(MousePress::Middle) => match item {
                TabBarItem::Tab { tab_idx, .. } => {
                    self.close_specific_tab(tab_idx, true);
                }
                TabBarItem::NewTabButton => {
                    self.do_new_tab_button_click(MousePress::Middle);
                }
                TabBarItem::None
                | TabBarItem::LeftStatus
                | TabBarItem::RightStatus
                | TabBarItem::WindowButton(_) => {}
            },
            WMEK::Press(MousePress::Right) => match item {
                TabBarItem::Tab { .. } => {
                    self.show_tab_navigator();
                }
                TabBarItem::NewTabButton => {
                    self.do_new_tab_button_click(MousePress::Right);
                }
                TabBarItem::None
                | TabBarItem::LeftStatus
                | TabBarItem::RightStatus
                | TabBarItem::WindowButton(_) => {}
            },
            WMEK::Move => match item {
                TabBarItem::None | TabBarItem::LeftStatus | TabBarItem::RightStatus => {
                    context.set_window_drag_position(event.screen_coords);
                }
                TabBarItem::WindowButton(window::IntegratedTitleButton::Maximize) => {
                    let item = self.last_ui_item.clone().unwrap();
                    let bounds: ::window::ScreenRect = euclid::rect(
                        item.x as isize - (event.coords.x - event.screen_coords.x),
                        item.y as isize - (event.coords.y - event.screen_coords.y),
                        item.width as isize,
                        item.height as isize,
                    );
                    context.set_maximize_button_position(bounds);
                }
                TabBarItem::WindowButton(_) | TabBarItem::Tab { .. } | TabBarItem::NewTabButton => {
                }
            },
            WMEK::VertWheel(n) if self.config.mouse_wheel_scrolls_tabs => {
                self.activate_tab_relative(if n < 1 { 1 } else { -1 }, true)
                    .ok();
            }
            _ => {}
        }
        context.set_cursor(Some(MouseCursor::Arrow));
    }
}
