use super::*;

impl super::super::TermWindow {
    pub(super) fn resolve_ui_item(&self, event: &MouseEvent) -> Option<UIItem> {
        let x = event.coords.x;
        let y = event.coords.y;
        let guard = self.ui_items.load();
        guard.iter().rev().find(|item| item.hit_test(x, y)).cloned()
    }

    fn leave_ui_item(&mut self, item: &UIItem) {
        match item.item_type {
            UIItemType::TabBar(_) => {
                self.update_title_post_status();
            }
            UIItemType::CloseTab(_)
            | UIItemType::AboveScrollThumb
            | UIItemType::BelowScrollThumb
            | UIItemType::ScrollThumb
            | UIItemType::Split(_)
            | UIItemType::NewTabOptionRadio { .. }
            | UIItemType::NewTabOptionRun
            | UIItemType::NewTabOptionClose => {}
        }
    }

    fn enter_ui_item(&mut self, item: &UIItem) {
        match item.item_type {
            UIItemType::TabBar(_) => {}
            UIItemType::CloseTab(_)
            | UIItemType::AboveScrollThumb
            | UIItemType::BelowScrollThumb
            | UIItemType::ScrollThumb
            | UIItemType::Split(_)
            | UIItemType::NewTabOptionRadio { .. }
            | UIItemType::NewTabOptionRun
            | UIItemType::NewTabOptionClose => {}
        }
    }

    pub fn mouse_event_impl(&mut self, event: MouseEvent, context: &dyn WindowOps) {
        log::trace!("{:?}", event);
        // Any button press or wheel tick breaks a Ctrl tap in progress -- the
        // Ctrl+click-on-a-link gesture must not arm the pass-through mode. An
        // already-armed mode is deliberately untouched.
        if matches!(
            event.kind,
            WMEK::Press(_) | WMEK::VertWheel(_) | WMEK::HorzWheel(_)
        ) {
            self.pass_through.mouse_input();
        }
        // A window can legitimately have no pane at all: `--choose-tab` opens
        // one whose only content is the New Tab Options modal, and the first
        // tab does not exist until the user presses Run. Returning here in
        // that state made the dialog completely unclickable, because this is
        // upstream of all UI-item hit testing. So the pane is optional from
        // here on, and only the paths that genuinely need one bail out.
        let pane = self.get_active_pane_or_overlay();
        if pane.is_none() && self.modal.borrow().is_none() {
            // No pane and no modal: nothing on screen that could want a click.
            return;
        }

        self.current_mouse_event.replace(event.clone());

        let border = self.get_os_border();

        let first_line_offset = if self.show_tab_bar && !self.config.tab_bar_at_bottom {
            self.tab_bar_pixel_height().unwrap_or(0.) as isize
        } else {
            0
        } + border.top.get() as isize;

        let (padding_left, padding_top) = self.padding_left_top();

        let y = (event
            .coords
            .y
            .sub(padding_top as isize)
            .sub(first_line_offset)
            .max(0)
            / self.render_metrics.cell_size.height) as i64;

        let x = (event
            .coords
            .x
            .sub((padding_left + border.left.get() as f32) as isize)
            .max(0) as f32)
            / self.render_metrics.cell_size.width as f32;
        let x = if !pane.as_ref().is_some_and(|p| p.is_mouse_grabbed()) {
            // Round the x coordinate so that we're a bit more forgiving of
            // the horizontal position when selecting cells
            x.round()
        } else {
            x
        }
        .trunc() as usize;

        let mut y_pixel_offset = event
            .coords
            .y
            .sub(padding_top as isize)
            .sub(first_line_offset);
        if y > 0 {
            y_pixel_offset = y_pixel_offset.max(0) % self.render_metrics.cell_size.height;
        }

        let mut x_pixel_offset = event
            .coords
            .x
            .sub((padding_left + border.left.get() as f32) as isize);
        if x > 0 {
            x_pixel_offset = x_pixel_offset.max(0) % self.render_metrics.cell_size.width;
        }

        self.last_mouse_coords = (x, y);

        let mut capture_mouse = false;

        match event.kind {
            WMEK::Release(ref press) => {
                self.current_mouse_capture = None;
                self.current_mouse_buttons.retain(|p| p != press);
                if press == &MousePress::Left && self.window_drag_position.take().is_some() {
                    // Completed a window drag
                    return;
                }
                if press == &MousePress::Left {
                    if let Some((item, _)) = self.dragging.take() {
                        // Completed a drag. A tab reorder drag swaps the
                        // pointer to the hand cursor for as long as the tab
                        // is held (see `drag_tab`), so put it back now that
                        // it isn't. Only for the tab bar: a split-resize drag
                        // leaves its own sizing cursor in place, and clobbering
                        // that with an arrow would flicker until the next Move
                        // recomputed it.
                        if matches!(item.item_type, UIItemType::TabBar(TabBarItem::Tab { .. })) {
                            context.set_cursor(Some(MouseCursor::Arrow));
                        }
                        return;
                    }
                }
            }

            WMEK::Press(ref press) => {
                capture_mouse = true;

                // Perform click counting
                let button = mouse_press_to_tmb(press);

                let click_position = ClickPosition {
                    column: x,
                    row: y,
                    x_pixel_offset,
                    y_pixel_offset,
                };

                let click = match self.last_mouse_click.take() {
                    None => LastMouseClick::new(button, click_position),
                    Some(click) => click.add(button, click_position),
                };
                self.last_mouse_click = Some(click);
                self.current_mouse_buttons.retain(|p| p != press);
                self.current_mouse_buttons.push(*press);

                // If this press arrives while the window is still within its
                // just-focused grace period, arm the same-position Move
                // suppression below: some window managers (observed on
                // Windows; see #2414 and #5309) synthesize a spurious
                // WM_MOUSEMOVE at the same coordinates immediately after the
                // activating click, which would otherwise be misreported to
                // mouse-aware programs (e.g. tmux) as a real drag.
                self.suppress_move_after_focus_click = if should_arm_focus_click_move_suppression(
                    self.focused.as_ref().map(Instant::elapsed),
                ) {
                    Some((event.coords.x, event.coords.y))
                } else {
                    None
                };
            }

            WMEK::Move => {
                if let Some(start) = self.window_drag_position.as_ref() {
                    // Dragging the window
                    // Compute the distance since the initial event
                    let delta_x = start.screen_coords.x - event.screen_coords.x;
                    let delta_y = start.screen_coords.y - event.screen_coords.y;

                    // Now compute a new window position.
                    // We don't have a direct way to get the position,
                    // but we can infer it by comparing the mouse coords
                    // with the screen coords in the initial event.
                    // This computes the original top_left position,
                    // and applies the total drag delta to it.
                    let top_left = ::window::ScreenPoint::new(
                        (start.screen_coords.x - start.coords.x) - delta_x,
                        (start.screen_coords.y - start.coords.y) - delta_y,
                    );
                    // and now tell the window to go there
                    context.set_window_position(top_left);
                    return;
                }

                if let Some((item, start_event)) = self.dragging.take() {
                    self.drag_ui_item(item, start_event, x, y, event, context);
                    return;
                }

                if let Some(armed) = self.suppress_move_after_focus_click.take() {
                    if armed == (event.coords.x, event.coords.y) {
                        // Zero-motion Move immediately following the click that
                        // (re)focused this window: this is the synthetic event
                        // described in #2414/#5309, not real mouse motion.
                        // Swallow it rather than forwarding a spurious drag to
                        // the pane's mouse reporting.
                        log::trace!(
                            "swallowing zero-motion Move immediately after focus-click at {:?}",
                            event.coords
                        );
                        return;
                    }
                }
            }
            _ => {
                self.suppress_move_after_focus_click = None;
            }
        }

        let prior_ui_item = self.last_ui_item.clone();

        let ui_item = if matches!(self.current_mouse_capture, None | Some(MouseCapture::UI)) {
            let ui_item = self.resolve_ui_item(&event);
            if matches!(event.kind, WMEK::Press(_)) {
                log::trace!(
                    "diag: mouse press at {:?} resolve_ui_item -> {:?} (modal_active={})",
                    event.coords,
                    ui_item.as_ref().map(|i| &i.item_type),
                    self.modal.borrow().is_some(),
                );
            }

            match (self.last_ui_item.take(), &ui_item) {
                (Some(prior), Some(item)) => {
                    if prior != *item || !self.config.use_fancy_tab_bar {
                        self.leave_ui_item(&prior);
                        self.enter_ui_item(item);
                        context.invalidate();
                    }
                }
                (Some(prior), None) => {
                    self.leave_ui_item(&prior);
                    context.invalidate();
                }
                (None, Some(item)) => {
                    self.enter_ui_item(item);
                    context.invalidate();
                }
                (None, None) => {}
            }

            ui_item
        } else {
            None
        };

        if let Some(item) = ui_item.clone() {
            if capture_mouse {
                self.current_mouse_capture = Some(MouseCapture::UI);
            }
            self.mouse_event_ui_item(item, pane, y, event, context);
        } else if let Some(pane) = pane {
            if matches!(
                self.current_mouse_capture,
                None | Some(MouseCapture::TerminalPane(_))
            ) {
                self.mouse_event_terminal(
                    pane,
                    ClickPosition {
                        column: x,
                        row: y,
                        x_pixel_offset,
                        y_pixel_offset,
                    },
                    event,
                    context,
                    capture_mouse,
                );
            }
        }

        if prior_ui_item != ui_item {
            self.update_title_post_status();
        }
    }
}
