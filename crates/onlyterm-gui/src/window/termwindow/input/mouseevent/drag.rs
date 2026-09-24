use super::*;

impl super::super::TermWindow {
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

    pub(super) fn drag_ui_item(
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
}
