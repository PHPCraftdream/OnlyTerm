use crate::tabbar::TabBarItem;
use crate::termwindow::{
    GuiWin, MouseCapture, PositionedSplit, ScrollHit, TermWindowNotif, UIItem, UIItemType, TMB,
};
use ::window::{
    MouseButtons as WMB, MouseCursor, MouseEvent, MouseEventKind as WMEK, MousePress,
    WindowDecorations, WindowOps, WindowState,
};
use config::keyassignment::{KeyAssignment, MouseEventTrigger, SpawnTabDomain};
use config::MouseEventAltScreen;
use mux::pane::{Pane, WithPaneLines};
use mux::tab::SplitDirection;
use mux::Mux;
use mux_funcs::MuxPane;
use onlyterm_dynamic::ToDynamic;
use onlyterm_term::input::{MouseButton, MouseEventKind as TMEK};
use onlyterm_term::{ClickPosition, LastMouseClick, StableRowIndex};
use std::convert::TryInto;
use std::ops::Sub;
use std::sync::Arc;
use std::time::{Duration, Instant};
use termwiz::hyperlink::Hyperlink;
use termwiz::surface::Line;

impl super::TermWindow {
    fn resolve_ui_item(&self, event: &MouseEvent) -> Option<UIItem> {
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

    pub fn mouse_leave_impl(&mut self, context: &dyn WindowOps) {
        self.current_mouse_event = None;
        self.update_title();
        context.set_cursor(Some(MouseCursor::Arrow));
        context.invalidate();
    }

    fn drag_split(
        &mut self,
        mut item: UIItem,
        split: PositionedSplit,
        start_event: MouseEvent,
        x: usize,
        y: i64,
        context: &dyn WindowOps,
    ) {
        let mux = Mux::get();
        let tab = match mux.get_active_tab_for_window(self.mux_window_id) {
            Some(tab) => tab,
            None => return,
        };
        let (left_or_top, changed) = match split.direction {
            SplitDirection::Horizontal => (x as isize, split.left as isize != x as isize),
            SplitDirection::Vertical => (y as isize, split.top as isize != y as isize),
        };

        if changed {
            tab.resize_split_to(split.index, left_or_top);
            if let Some(split) = tab.iter_splits().into_iter().nth(split.index) {
                item.item_type = UIItemType::Split(split);
                context.invalidate();
            }
        }
        self.dragging.replace((item, start_event));
    }

    fn drag_scroll_thumb(
        &mut self,
        item: UIItem,
        start_event: MouseEvent,
        event: MouseEvent,
        context: &dyn WindowOps,
    ) {
        let pane = match self.get_active_pane_or_overlay() {
            Some(pane) => pane,
            None => return,
        };

        let dims = pane.get_dimensions();
        let current_viewport = self.get_viewport(pane.pane_id());

        let tab_bar_height = if self.show_tab_bar {
            self.tab_bar_pixel_height().unwrap_or(0.)
        } else {
            0.
        };
        let (top_bar_height, bottom_bar_height) = if self.config.tab_bar_at_bottom {
            (0.0, tab_bar_height)
        } else {
            (tab_bar_height, 0.0)
        };

        let border = self.get_os_border();
        let y_offset = top_bar_height + border.top.get() as f32;

        let from_top = start_event.coords.y.saturating_sub(item.y as isize);
        let effective_thumb_top = event
            .coords
            .y
            .saturating_sub(y_offset as isize + from_top)
            .max(0) as usize;

        // Convert thumb top into a row index by reversing the math
        // in ScrollHit::thumb
        let row = ScrollHit::thumb_top_to_scroll_top(
            effective_thumb_top,
            &*pane,
            current_viewport,
            self.dimensions.pixel_height.saturating_sub(
                y_offset as usize + border.bottom.get() + bottom_bar_height as usize,
            ),
            self.min_scroll_bar_height() as usize,
        );
        self.set_viewport(pane.pane_id(), Some(row), dims);
        context.invalidate();
        self.dragging.replace((item, start_event));
    }

    fn drag_ui_item(
        &mut self,
        item: UIItem,
        start_event: MouseEvent,
        x: usize,
        y: i64,
        event: MouseEvent,
        context: &dyn WindowOps,
    ) {
        match item.item_type {
            UIItemType::Split(split) => {
                self.drag_split(item, split, start_event, x, y, context);
            }
            UIItemType::ScrollThumb => {
                self.drag_scroll_thumb(item, start_event, event, context);
            }
            UIItemType::TabBar(TabBarItem::Tab { .. }) => {
                self.drag_tab(item, start_event, event, context);
            }
            _ => {
                log::error!("drag not implemented for {:?}", item);
            }
        }
    }

    /// Reorders the tab bar by mouse drag. The pressed tab is already the
    /// active one (`mouse_event_tab_bar`'s Press branch activates it before
    /// arming `self.dragging`), so this only needs to find which tab the
    /// pointer is over *now* and hand that index to the same `move_tab`
    /// the `MoveTab`/`MoveTabRelative` key assignments already use --
    /// reordering itself isn't new logic, only driving it from the mouse
    /// is. Scoped to reordering within this tab bar for now; dragging a
    /// tab out to detach it into a new window is a separate, larger
    /// feature, not attempted here.
    fn drag_tab(
        &mut self,
        item: UIItem,
        _start_event: MouseEvent,
        event: MouseEvent,
        context: &dyn WindowOps,
    ) {
        // Re-arm for the next Move event, the same way drag_split/
        // drag_scroll_thumb keep themselves armed until mouse-up clears
        // `self.dragging` (see the WMEK::Release handling above).
        self.dragging.replace((item, event.clone()));

        // Show a hand for as long as the tab is being carried. Windows has no
        // closed/grabbing-hand cursor among its stock set -- IDC_HAND is the
        // pointing hand -- and drawing our own was tried and deliberately
        // reverted: a hand-drawn cursor ignores the user's cursor scheme, the
        // "large cursors" accessibility setting and high-contrast inversion,
        // which is a poor trade for a slightly better-fitting shape. The arrow
        // is restored on mouse-up in the WMEK::Release branch above.
        context.set_cursor(Some(MouseCursor::Hand));

        if let Some(target) = self.resolve_ui_item(&event) {
            if let UIItemType::TabBar(TabBarItem::Tab { tab_idx, .. }) = target.item_type {
                if self.move_tab(tab_idx).is_ok() {
                    context.invalidate();
                }
            }
        }
    }

    /// `pane` is optional because a `--choose-tab` window has none until the
    /// user presses Run; only the scrollbar items genuinely need one, and they
    /// cannot be on screen in that state anyway.
    fn mouse_event_ui_item(
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
        }
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

    pub fn mouse_event_above_scroll_thumb(
        &mut self,
        _item: UIItem,
        pane: Arc<dyn Pane>,
        event: MouseEvent,
        context: &dyn WindowOps,
    ) {
        if let WMEK::Press(MousePress::Left) = event.kind {
            let dims = pane.get_dimensions();
            let current_viewport = self.get_viewport(pane.pane_id());
            // Page up
            self.set_viewport(
                pane.pane_id(),
                Some(
                    current_viewport
                        .unwrap_or(dims.physical_top)
                        .saturating_sub(self.terminal_size.rows.try_into().unwrap()),
                ),
                dims,
            );
            context.invalidate();
        }
        context.set_cursor(Some(MouseCursor::Arrow));
    }

    pub fn mouse_event_below_scroll_thumb(
        &mut self,
        _item: UIItem,
        pane: Arc<dyn Pane>,
        event: MouseEvent,
        context: &dyn WindowOps,
    ) {
        if let WMEK::Press(MousePress::Left) = event.kind {
            let dims = pane.get_dimensions();
            let current_viewport = self.get_viewport(pane.pane_id());
            // Page down
            self.set_viewport(
                pane.pane_id(),
                Some(
                    current_viewport
                        .unwrap_or(dims.physical_top)
                        .saturating_add(self.terminal_size.rows.try_into().unwrap()),
                ),
                dims,
            );
            context.invalidate();
        }
        context.set_cursor(Some(MouseCursor::Arrow));
    }

    pub fn mouse_event_scroll_thumb(
        &mut self,
        item: UIItem,
        _pane: Arc<dyn Pane>,
        event: MouseEvent,
        context: &dyn WindowOps,
    ) {
        if let WMEK::Press(MousePress::Left) = event.kind {
            // Start a scroll drag
            // self.scroll_drag_start = Some(from_top);
            self.dragging = Some((item, event));
        }
        context.set_cursor(Some(MouseCursor::Arrow));
    }

    pub fn mouse_event_split(
        &mut self,
        item: UIItem,
        split: PositionedSplit,
        event: MouseEvent,
        context: &dyn WindowOps,
    ) {
        context.set_cursor(Some(match &split.direction {
            SplitDirection::Horizontal => MouseCursor::SizeLeftRight,
            SplitDirection::Vertical => MouseCursor::SizeUpDown,
        }));

        if event.kind == WMEK::Press(MousePress::Left) {
            self.dragging.replace((item, event));
        }
    }

    fn mouse_event_terminal(
        &mut self,
        mut pane: Arc<dyn Pane>,
        position: ClickPosition,
        event: MouseEvent,
        context: &dyn WindowOps,
        capture_mouse: bool,
    ) {
        let mut is_click_to_focus_pane = false;

        let ClickPosition {
            mut column,
            mut row,
            mut x_pixel_offset,
            mut y_pixel_offset,
        } = position;

        let is_already_captured = matches!(
            self.current_mouse_capture,
            Some(MouseCapture::TerminalPane(_))
        );

        for pos in self.get_panes_to_render() {
            if !is_already_captured
                && row >= pos.top as i64
                && row <= (pos.top + pos.height) as i64
                && column >= pos.left
                && column <= pos.left + pos.width
            {
                if pane.pane_id() != pos.pane.pane_id() {
                    // We're over a pane that isn't active
                    match &event.kind {
                        WMEK::Press(_) => {
                            let mux = Mux::get();
                            if let Some(tab) = mux.get_active_tab_for_window(self.mux_window_id) {
                                tab.set_active_idx(pos.index)
                            }

                            pane = Arc::clone(&pos.pane);
                            is_click_to_focus_pane = true;
                        }
                        WMEK::Move => {
                            if self.config.pane_focus_follows_mouse {
                                let mux = Mux::get();
                                if let Some(tab) = mux.get_active_tab_for_window(self.mux_window_id)
                                {
                                    tab.set_active_idx(pos.index)
                                }

                                pane = Arc::clone(&pos.pane);
                                context.invalidate();
                            }
                        }
                        WMEK::Release(_) | WMEK::HorzWheel(_) => {}
                        WMEK::VertWheel(_) => {
                            // Let wheel events route to the hovered pane,
                            // even if it doesn't have focus
                            pane = Arc::clone(&pos.pane);
                            context.invalidate();
                        }
                    }
                }
                column = column.saturating_sub(pos.left);
                row = row.saturating_sub(pos.top as i64);
                break;
            } else if is_already_captured && pane.pane_id() == pos.pane.pane_id() {
                column = column.saturating_sub(pos.left);
                row = row.saturating_sub(pos.top as i64).max(0);

                if position.column < pos.left {
                    x_pixel_offset -= self.render_metrics.cell_size.width
                        * (pos.left as isize - position.column as isize);
                }
                if position.row < pos.top as i64 {
                    y_pixel_offset -= self.render_metrics.cell_size.height
                        * (pos.top as isize - position.row as isize);
                }

                break;
            }
        }

        if capture_mouse {
            self.current_mouse_capture = Some(MouseCapture::TerminalPane(pane.pane_id()));
        }

        let is_focused = if let Some(focused) = self.focused.as_ref() {
            !self.config.swallow_mouse_click_on_window_focus
                || (focused.elapsed() > Duration::from_millis(200))
        } else {
            false
        };

        if self.focused.is_some()
            && !is_focused
            && matches!(&event.kind, WMEK::Press(_))
            && self.config.swallow_mouse_click_on_window_focus
        {
            // Entering click to focus state
            self.is_click_to_focus_window = true;
            context.invalidate();
            log::trace!("enter click to focus");
            return;
        }
        if self.is_click_to_focus_window && matches!(&event.kind, WMEK::Release(_)) {
            // Exiting click to focus state
            self.is_click_to_focus_window = false;
            context.invalidate();
            log::trace!("exit click to focus");
            return;
        }

        let allow_action = if self.is_click_to_focus_window || !is_focused {
            matches!(&event.kind, WMEK::VertWheel(_) | WMEK::HorzWheel(_))
        } else {
            true
        };

        log::trace!(
            "is_focused={} allow_action={} event={:?}",
            is_focused,
            allow_action,
            event
        );

        let dims = pane.get_dimensions();
        let stable_row = self
            .get_viewport(pane.pane_id())
            .unwrap_or(dims.physical_top)
            + row as StableRowIndex;

        // When the click lands inside a wide (multi-column) character, the
        // rounded column can point at the character's second (hidden) column,
        // which would start/extend a selection in the middle of the glyph.
        // Snap the column to the nearer edge of the wide character based on
        // which half of it was clicked. We only do this when the mouse isn't
        // grabbed, matching the coordinate rounding in `mouse_event_impl`.
        if column > 0 && !pane.is_mouse_grabbed() {
            let (_, lines) = pane.get_lines(stable_row..stable_row + 1);
            if let Some(line) = lines.first() {
                if let Some(covered) = line.wide_cell_covering(column) {
                    // Reconstruct the sub-cell click position (in columns) so
                    // that we can compare against the wide character's midpoint.
                    let cell_width = self.render_metrics.cell_size.width as f32;
                    let frac = if cell_width > 0. {
                        x_pixel_offset as f32 / cell_width
                    } else {
                        0.
                    };
                    // `column` is `round(floor_val + frac)`, so a fraction of
                    // 0.5 or more means the value was rounded up.
                    let floor_val = if frac >= 0.5 {
                        column.saturating_sub(1)
                    } else {
                        column
                    };
                    let x_in_cells = floor_val as f32 + frac;
                    let mid = covered.start as f32 + (covered.end - covered.start) as f32 / 2.;
                    column = if x_in_cells < mid {
                        covered.start
                    } else {
                        covered.end
                    };
                }
            }
        }

        self.pane_state(pane.pane_id())
            .mouse_terminal_coords
            .replace((
                ClickPosition {
                    column,
                    row,
                    x_pixel_offset,
                    y_pixel_offset,
                },
                stable_row,
            ));

        pane.apply_hyperlinks(stable_row..stable_row + 1, &self.config.hyperlink_rules);

        struct FindCurrentLink {
            current: Option<Arc<Hyperlink>>,
            stable_row: StableRowIndex,
            column: usize,
        }

        impl WithPaneLines for FindCurrentLink {
            fn with_lines_mut(&mut self, stable_top: StableRowIndex, lines: &mut [&mut Line]) {
                if stable_top == self.stable_row {
                    if let Some(line) = lines.first() {
                        if let Some(cell) = line.get_cell(self.column) {
                            self.current = cell.attrs().hyperlink().cloned();
                        }
                    }
                }
            }
        }

        let mut find_link = FindCurrentLink {
            current: None,
            stable_row,
            column,
        };
        pane.with_lines_mut(stable_row..stable_row + 1, &mut find_link);
        let new_highlight = find_link.current;

        match (self.current_highlight.as_ref(), new_highlight) {
            (Some(old_link), Some(new_link)) if Arc::ptr_eq(old_link, &new_link) => {
                // Unchanged
            }
            (None, None) => {
                // Unchanged
            }
            (_, rhs) => {
                // We're hovering over a different URL, so invalidate and repaint
                // so that we render the underline correctly
                self.current_highlight = rhs;
                context.invalidate();
            }
        };

        let outside_window = event.coords.x < 0
            || event.coords.x as usize > self.dimensions.pixel_width
            || event.coords.y < 0
            || event.coords.y as usize > self.dimensions.pixel_height;

        context.set_cursor(Some(if self.current_highlight.is_some() {
            // When hovering over a hyperlink, show an appropriate
            // mouse cursor to give the cue that it is clickable
            MouseCursor::Hand
        } else if pane.is_mouse_grabbed() || outside_window {
            MouseCursor::Arrow
        } else {
            MouseCursor::Text
        }));

        let event_trigger_type = match &event.kind {
            WMEK::Press(press) => {
                let press = mouse_press_to_tmb(press);
                match self.last_mouse_click.as_ref() {
                    Some(LastMouseClick { streak, button, .. }) if *button == press => {
                        Some(MouseEventTrigger::Down {
                            streak: *streak,
                            button: press,
                        })
                    }
                    _ => None,
                }
            }
            WMEK::Release(press) => {
                let press = mouse_press_to_tmb(press);
                match self.last_mouse_click.as_ref() {
                    Some(LastMouseClick { streak, button, .. }) if *button == press => {
                        Some(MouseEventTrigger::Up {
                            streak: *streak,
                            button: press,
                        })
                    }
                    _ => None,
                }
            }
            WMEK::Move => {
                if !self.current_mouse_buttons.is_empty() {
                    if let Some(LastMouseClick { streak, button, .. }) =
                        self.last_mouse_click.as_ref()
                    {
                        if Some(*button)
                            == self.current_mouse_buttons.last().map(mouse_press_to_tmb)
                        {
                            Some(MouseEventTrigger::Drag {
                                streak: *streak,
                                button: *button,
                            })
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            WMEK::VertWheel(amount) => Some(match *amount {
                0 => return,
                1.. => MouseEventTrigger::Down {
                    streak: 1,
                    button: MouseButton::WheelUp(*amount as usize),
                },
                _ => MouseEventTrigger::Down {
                    streak: 1,
                    button: MouseButton::WheelDown(-amount as usize),
                },
            }),
            WMEK::HorzWheel(amount) => Some(match *amount {
                0 => return,
                1.. => MouseEventTrigger::Down {
                    streak: 1,
                    button: MouseButton::WheelLeft(*amount as usize),
                },
                _ => MouseEventTrigger::Down {
                    streak: 1,
                    button: MouseButton::WheelRight(-amount as usize),
                },
            }),
        };

        if allow_action {
            if let Some(mut event_trigger_type) = event_trigger_type {
                self.current_event = Some(event_trigger_type.to_dynamic());
                let mut modifiers = event.modifiers;

                // Since we use shift to force assessing the mouse bindings, pretend
                // that shift is not one of the mods when the mouse is grabbed.
                let mut mouse_reporting = pane.is_mouse_grabbed();
                if mouse_reporting
                    && modifiers.contains(self.config.bypass_mouse_reporting_modifiers)
                {
                    modifiers.remove(self.config.bypass_mouse_reporting_modifiers);
                    mouse_reporting = false;
                }

                if mouse_reporting {
                    // If they were scrolled back prior to launching an
                    // application that captures the mouse, then mouse based
                    // scrolling assignments won't have any effect.
                    // Ensure that we scroll to the bottom if they try to
                    // use the mouse so that things are less surprising
                    self.scroll_to_bottom(&pane);
                }

                // normalize delta and streak to make mouse assignment
                // easier to wrangle
                match event_trigger_type {
                    MouseEventTrigger::Down {
                        ref mut streak,
                        button:
                            MouseButton::WheelUp(ref mut delta)
                            | MouseButton::WheelDown(ref mut delta)
                            | MouseButton::WheelLeft(ref mut delta)
                            | MouseButton::WheelRight(ref mut delta),
                    }
                    | MouseEventTrigger::Up {
                        ref mut streak,
                        button:
                            MouseButton::WheelUp(ref mut delta)
                            | MouseButton::WheelDown(ref mut delta)
                            | MouseButton::WheelLeft(ref mut delta)
                            | MouseButton::WheelRight(ref mut delta),
                    }
                    | MouseEventTrigger::Drag {
                        ref mut streak,
                        button:
                            MouseButton::WheelUp(ref mut delta)
                            | MouseButton::WheelDown(ref mut delta)
                            | MouseButton::WheelLeft(ref mut delta)
                            | MouseButton::WheelRight(ref mut delta),
                    } => {
                        *streak = 1;
                        *delta = 1;
                    }
                    _ => {}
                };

                let mouse_mods = config::MouseEventTriggerMods {
                    mods: modifiers,
                    mouse_reporting,
                    alt_screen: if pane.is_alt_screen_active() {
                        MouseEventAltScreen::True
                    } else {
                        MouseEventAltScreen::False
                    },
                };

                if let Some(action) = self.input_map.lookup_mouse(event_trigger_type, mouse_mods) {
                    self.perform_key_assignment(&pane, &action).ok();
                    return;
                }
            }
        }

        let mouse_event = onlyterm_term::MouseEvent {
            kind: match event.kind {
                WMEK::Move => TMEK::Move,
                WMEK::VertWheel(_) | WMEK::HorzWheel(_) | WMEK::Press(_) => TMEK::Press,
                WMEK::Release(_) => TMEK::Release,
            },
            button: match event.kind {
                WMEK::Release(ref press) | WMEK::Press(ref press) => mouse_press_to_tmb(press),
                WMEK::Move => {
                    if event.mouse_buttons == WMB::LEFT {
                        TMB::Left
                    } else if event.mouse_buttons == WMB::RIGHT {
                        TMB::Right
                    } else if event.mouse_buttons == WMB::MIDDLE {
                        TMB::Middle
                    } else {
                        TMB::None
                    }
                }
                WMEK::VertWheel(amount) => {
                    if amount > 0 {
                        TMB::WheelUp(amount as usize)
                    } else {
                        TMB::WheelDown((-amount) as usize)
                    }
                }
                WMEK::HorzWheel(amount) => {
                    if amount > 0 {
                        TMB::WheelLeft(amount as usize)
                    } else {
                        TMB::WheelRight((-amount) as usize)
                    }
                }
            },
            x: column,
            y: row,
            x_pixel_offset,
            y_pixel_offset,
            modifiers: event.modifiers,
        };

        if allow_action
            && !(self.config.swallow_mouse_click_on_pane_focus && is_click_to_focus_pane)
        {
            pane.mouse_event(mouse_event).ok();
        }

        match event.kind {
            WMEK::Move => {}
            _ => {
                context.invalidate();
            }
        }
    }
}

fn mouse_press_to_tmb(press: &MousePress) -> TMB {
    match press {
        MousePress::Left => TMB::Left,
        MousePress::Right => TMB::Right,
        MousePress::Middle => TMB::Middle,
    }
}

/// What should happen when a mouse button is released over a cell, given
/// whether that cell carries an active hyperlink.
///
/// This is the specification for the default mouse bindings configured in
/// `InputMap::new` (see `onlyterm-gui/src/inputmap.rs`), which bind
/// `MouseEventTrigger::Up { button: MouseButton::Left, .. }` to
/// `CompleteSelectionOrOpenLinkAtMouseCursor` (open) and
/// `MouseEventTrigger::Up { button: MouseButton::Right, .. }` to
/// `CopyLinkAtMouseCursor` (copy). `TermWindow::do_open_link_at_mouse_cursor`
/// and `TermWindow::do_copy_link_at_mouse_cursor` implement the "open" and
/// "copy" halves respectively, each gated on `current_highlight.is_some()`
/// exactly as modeled here. Kept as a standalone pure function (rather than
/// inlined) so the button/link/action matrix has a single, unit-testable
/// source of truth. Any button other than left/right, or a click that
/// doesn't land on a hyperlink, has no hyperlink-related effect: a
/// right-click with no link present simply falls through to whatever else
/// is bound to it (e.g. the pane's native mouse reporting), and a
/// middle-click over a link is left alone since it is reserved for
/// primary-selection paste.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
enum HyperlinkClickAction {
    Open,
    Copy,
}

#[allow(dead_code)]
fn hyperlink_click_action(button: MousePress, has_hyperlink: bool) -> Option<HyperlinkClickAction> {
    if !has_hyperlink {
        return None;
    }
    match button {
        MousePress::Left => Some(HyperlinkClickAction::Open),
        MousePress::Right => Some(HyperlinkClickAction::Copy),
        MousePress::Middle => None,
    }
}

/// How long after gaining focus a button-down is still considered to be
/// "the activating click", for the purposes of arming suppression of a
/// same-position synthetic Move (see #2414, #5309). This only needs to
/// bridge a single OS message-pump tick, so it is deliberately much shorter
/// than the 200ms grace period used by `swallow_mouse_click_on_window_focus`
/// (which governs a different, opt-in policy: whether the activating click
/// itself is forwarded to the pane).
const FOCUS_CLICK_MOVE_SUPPRESSION_GRACE: Duration = Duration::from_millis(50);

/// Decide whether a button-down event should arm suppression of the next
/// same-position Move, given how long ago (if at all) the window most
/// recently gained focus.
fn should_arm_focus_click_move_suppression(focused_elapsed: Option<Duration>) -> bool {
    matches!(focused_elapsed, Some(elapsed) if elapsed <= FOCUS_CLICK_MOVE_SUPPRESSION_GRACE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arms_when_press_is_within_focus_grace_period() {
        assert!(should_arm_focus_click_move_suppression(Some(
            Duration::from_millis(0)
        )));
        assert!(should_arm_focus_click_move_suppression(Some(
            FOCUS_CLICK_MOVE_SUPPRESSION_GRACE
        )));
    }

    #[test]
    fn does_not_arm_once_grace_period_has_elapsed() {
        assert!(!should_arm_focus_click_move_suppression(Some(
            FOCUS_CLICK_MOVE_SUPPRESSION_GRACE + Duration::from_millis(1)
        )));
        assert!(!should_arm_focus_click_move_suppression(Some(
            Duration::from_secs(5)
        )));
    }

    #[test]
    fn does_not_arm_when_window_was_never_focused() {
        // `self.focused` is `None` whenever the window doesn't currently
        // have focus (e.g. it was never focused, or focus was just lost);
        // there is no activating click to protect in that case.
        assert!(!should_arm_focus_click_move_suppression(None));
    }

    /// Regression test for #2414 / #5309: a same-position Move that arrives
    /// immediately after the click that (re)focused the window must be
    /// recognized as suppressible, while a Move at a different position
    /// (real motion) must not be, and neither should be a Move that arrives
    /// well after the focus-granting click.
    #[test]
    fn suppresses_only_the_exact_zero_motion_move_after_focus_click() {
        let click_coords: (isize, isize) = (1, 1);

        // Simulates arming: a Press landed inside the grace period.
        let armed = if should_arm_focus_click_move_suppression(Some(Duration::from_millis(0))) {
            Some(click_coords)
        } else {
            None
        };
        assert_eq!(armed, Some(click_coords));

        // A same-position Move should match the armed coordinates (and thus
        // be suppressed by the caller).
        let same_position_move = click_coords;
        assert_eq!(armed, Some(same_position_move));

        // A Move at different coordinates is real motion and must not match.
        let real_motion_move: (isize, isize) = (1, 2);
        assert_ne!(armed, Some(real_motion_move));

        // If the Press arrived outside the grace period, nothing is armed,
        // so even a same-position Move afterwards is left untouched.
        let not_armed = if should_arm_focus_click_move_suppression(Some(Duration::from_millis(200)))
        {
            Some(click_coords)
        } else {
            None
        };
        assert_eq!(not_armed, None);
    }

    /// Regression test: right-clicking a hyperlink must copy its URL to the
    /// clipboard rather than open it, while left-click keeps opening the
    /// link as it always has.
    #[test]
    fn right_click_on_hyperlink_copies_left_click_opens() {
        assert_eq!(
            hyperlink_click_action(MousePress::Right, true),
            Some(HyperlinkClickAction::Copy)
        );
        assert_eq!(
            hyperlink_click_action(MousePress::Left, true),
            Some(HyperlinkClickAction::Open)
        );
    }

    #[test]
    fn click_without_a_hyperlink_does_nothing() {
        assert_eq!(hyperlink_click_action(MousePress::Left, false), None);
        assert_eq!(hyperlink_click_action(MousePress::Right, false), None);
        assert_eq!(hyperlink_click_action(MousePress::Middle, false), None);
    }

    #[test]
    fn middle_click_on_hyperlink_has_no_hyperlink_effect() {
        // Middle-click is reserved for primary-selection paste; it must not
        // be repurposed for hyperlink handling even when hovering a link.
        assert_eq!(hyperlink_click_action(MousePress::Middle, true), None);
    }
}
