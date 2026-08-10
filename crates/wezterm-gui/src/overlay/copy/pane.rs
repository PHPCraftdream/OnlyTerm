use super::*;

use config::keyassignment::{CopyModeAssignment, KeyAssignment, ScrollbackEraseMode};
use mux::domain::DomainId;
use mux::pane::{
    CachePolicy, ForEachPaneLogicalLine, LogicalLine, Pane, PaneId, Pattern,
    PerformAssignmentResult, WithPaneLines,
};
use parking_lot::{MappedMutexGuard, MutexGuard};
use rangeset::RangeSet;
use std::ops::Range;
use std::sync::Arc;
use termwiz::cell::{Cell, CellAttributes};
use termwiz::color::AnsiColor;
use termwiz::lineedit::Movement;
use termwiz::surface::SequenceNo;
use url::Url;
use wezterm_term::color::ColorPalette;
use wezterm_term::{
    unicode_column_width, Clipboard, KeyCode, KeyModifiers, Line, MouseEvent, StableRowIndex,
    TerminalSize,
};

impl Pane for CopyOverlay {
    fn pane_id(&self) -> PaneId {
        self.delegate.pane_id()
    }

    fn get_title(&self) -> String {
        format!("Copy mode: {}", self.delegate.get_title())
    }

    fn send_paste(&self, text: &str) -> anyhow::Result<()> {
        // paste into the search bar
        let mut r = self.render.lock();
        r.search_line.insert_text(text);
        r.schedule_update_search();
        Ok(())
    }

    fn reader(&self) -> anyhow::Result<Option<Box<dyn std::io::Read + Send>>> {
        Ok(None)
    }

    fn writer(&self) -> MappedMutexGuard<'_, dyn std::io::Write> {
        MutexGuard::map(self.writer.lock(), |writer| {
            let w: &mut dyn std::io::Write = writer;
            w
        })
    }

    fn resize(&self, size: TerminalSize) -> anyhow::Result<()> {
        self.delegate.resize(size)
    }

    fn key_up(&self, _key: KeyCode, _mods: KeyModifiers) -> anyhow::Result<()> {
        Ok(())
    }

    fn key_down(&self, key: KeyCode, mods: KeyModifiers) -> anyhow::Result<()> {
        let mut render = self.render.lock();
        let mods = mods.remove_positional_mods();
        if let Some(jump) = render.pending_jump.take() {
            match (key, mods) {
                (KeyCode::Char(c), KeyModifiers::NONE)
                | (KeyCode::Char(c), KeyModifiers::SHIFT) => {
                    let jump = Jump {
                        forward: jump.forward,
                        prev_char: jump.prev_char,
                        target: c,
                    };
                    render.last_jump.replace(jump);
                    render.perform_jump(jump, false);
                }
                _ => {
                    self.delegate
                        .perform_actions(vec![termwiz::escape::Action::Control(
                            termwiz::escape::ControlCode::Bell,
                        )]);
                }
            }
            return Ok(());
        }

        if render.editing_search {
            match (key, mods) {
                (KeyCode::Char(c), KeyModifiers::NONE)
                | (KeyCode::Char(c), KeyModifiers::SHIFT) => {
                    // Type to add to the pattern
                    render.search_line.insert_char(c);

                    render.schedule_update_search();
                }
                (KeyCode::Char('H'), KeyModifiers::CTRL)
                | (KeyCode::Backspace, KeyModifiers::NONE) => {
                    render
                        .search_line
                        .kill_text(Movement::BackwardChar(1), Movement::BackwardChar(1));

                    render.schedule_update_search();
                }
                (KeyCode::Delete, KeyModifiers::NONE) => {
                    render
                        .search_line
                        .kill_text(Movement::ForwardChar(1), Movement::None);

                    render.schedule_update_search();
                }
                (KeyCode::Backspace, KeyModifiers::ALT)
                | (KeyCode::Char('W'), KeyModifiers::CTRL) => {
                    render
                        .search_line
                        .kill_text(Movement::BackwardWord(1), Movement::BackwardWord(1));

                    render.schedule_update_search();
                }
                (KeyCode::Backspace, KeyModifiers::SUPER) => {
                    render
                        .search_line
                        .kill_text(Movement::StartOfLine, Movement::StartOfLine);

                    render.schedule_update_search();
                }
                (KeyCode::Char('K'), KeyModifiers::CTRL) => {
                    render
                        .search_line
                        .kill_text(Movement::EndOfLine, Movement::EndOfLine);

                    render.schedule_update_search();
                }
                (KeyCode::Char('B'), KeyModifiers::CTRL)
                | (KeyCode::ApplicationLeftArrow, KeyModifiers::NONE)
                | (KeyCode::LeftArrow, KeyModifiers::NONE) => {
                    render.search_line.exec_movement(Movement::BackwardChar(1));
                }
                (KeyCode::Char('F'), KeyModifiers::CTRL)
                | (KeyCode::ApplicationRightArrow, KeyModifiers::NONE)
                | (KeyCode::RightArrow, KeyModifiers::NONE) => {
                    render.search_line.exec_movement(Movement::ForwardChar(1));
                }
                (KeyCode::ApplicationLeftArrow, KeyModifiers::CTRL)
                | (KeyCode::LeftArrow, KeyModifiers::CTRL) => {
                    render.search_line.exec_movement(Movement::BackwardWord(1));
                }
                (KeyCode::ApplicationRightArrow, KeyModifiers::CTRL)
                | (KeyCode::RightArrow, KeyModifiers::CTRL) => {
                    render.search_line.exec_movement(Movement::ForwardWord(1));
                }
                (KeyCode::Char('A'), KeyModifiers::CTRL) | (KeyCode::Home, KeyModifiers::NONE) => {
                    render.search_line.exec_movement(Movement::StartOfLine);
                }
                (KeyCode::Char('E'), KeyModifiers::CTRL) | (KeyCode::End, KeyModifiers::NONE) => {
                    render.search_line.exec_movement(Movement::EndOfLine);
                }
                _ => {}
            }
        }

        Ok(())
    }

    fn perform_assignment(&self, assignment: &KeyAssignment) -> PerformAssignmentResult {
        use CopyModeAssignment::*;
        let mut render = self.render.lock();
        if render.pending_jump.is_some() {
            // Block key assignments until key_down is called
            // and resolves the next state
            return PerformAssignmentResult::BlockAssignmentAndRouteToKeyDown;
        }
        match assignment {
            KeyAssignment::CopyMode(assignment) => {
                match assignment {
                    MoveToViewportBottom => render.move_to_viewport_bottom(),
                    MoveToViewportTop => render.move_to_viewport_top(),
                    MoveToViewportMiddle => render.move_to_viewport_middle(),
                    MoveToScrollbackTop => render.move_to_top(),
                    MoveToScrollbackBottom => render.move_to_bottom(),
                    MoveToStartOfLineContent => render.move_to_start_of_line_content(),
                    MoveToEndOfLineContent => render.move_to_end_of_line_content(),
                    MoveToStartOfLine => render.move_to_start_of_line(),
                    MoveToStartOfNextLine => render.move_to_start_of_next_line(),
                    MoveToSelectionOtherEnd => render.move_to_selection_other_end(),
                    MoveToSelectionOtherEndHoriz => render.move_to_selection_other_end_horiz(),
                    MoveBackwardWord => render.move_backward_one_word(),
                    MoveForwardWord => render.move_forward_one_word(),
                    MoveForwardWordEnd => render.move_to_end_of_word(),
                    MoveRight => render.move_right_single_cell(),
                    MoveLeft => render.move_left_single_cell(),
                    MoveUp => render.move_up_single_row(),
                    MoveDown => render.move_down_single_row(),
                    MoveByPage(n) => render.move_by_page(**n),
                    PageUp => render.move_by_page(-1.0),
                    PageDown => render.move_by_page(1.0),
                    Close => render.close(),
                    PriorMatch => render.prior_match(),
                    NextMatch => render.next_match(),
                    PriorMatchPage => render.prior_match_page(),
                    NextMatchPage => render.next_match_page(),
                    CycleMatchType => render.cycle_match_type(),
                    ClearPattern => render.clear_pattern(),
                    EditPattern => render.edit_pattern(),
                    AcceptPattern => render.accept_pattern(),
                    SetSelectionMode(mode) => render.set_selection_mode(mode),
                    ClearSelectionMode => render.clear_selection_mode(),
                    MoveBackwardSemanticZone => render.move_by_zone(-1, None),
                    MoveForwardSemanticZone => render.move_by_zone(1, None),
                    MoveBackwardZoneOfType(zone_type) => render.move_by_zone(-1, Some(*zone_type)),
                    MoveForwardZoneOfType(zone_type) => render.move_by_zone(1, Some(*zone_type)),
                    JumpForward { prev_char } => render.jump(true, *prev_char),
                    JumpBackward { prev_char } => render.jump(false, *prev_char),
                    JumpAgain => render.jump_again(false),
                    JumpReverse => render.jump_again(true),
                }
                PerformAssignmentResult::Handled
            }
            _ => PerformAssignmentResult::Unhandled,
        }
    }

    fn mouse_event(&self, _event: MouseEvent) -> anyhow::Result<()> {
        anyhow::bail!("ignoring mouse while copying");
    }

    fn perform_actions(&self, actions: Vec<termwiz::escape::Action>) {
        self.delegate.perform_actions(actions)
    }

    fn is_dead(&self) -> bool {
        self.delegate.is_dead()
    }

    fn palette(&self) -> ColorPalette {
        self.delegate.palette()
    }

    fn domain_id(&self) -> DomainId {
        self.delegate.domain_id()
    }

    fn erase_scrollback(&self, erase_mode: ScrollbackEraseMode) {
        self.delegate.erase_scrollback(erase_mode)
    }

    fn is_mouse_grabbed(&self) -> bool {
        // Force grabbing off while we're searching
        false
    }

    fn is_alt_screen_active(&self) -> bool {
        false
    }

    fn set_clipboard(&self, clipboard: &Arc<dyn Clipboard>) {
        self.delegate.set_clipboard(clipboard)
    }

    fn get_current_working_dir(&self, policy: CachePolicy) -> Option<Url> {
        self.delegate.get_current_working_dir(policy)
    }

    fn get_cursor_position(&self) -> StableCursorPosition {
        let renderer = self.render.lock();
        if renderer.editing_search {
            // place in the search box
            // Padding between the start of the editable line and the left side of the terminal
            const SEARCH_CURSOR_PADDING: usize = 8;
            let cursor = unicode_column_width(
                &renderer.search_line.get_line()[0..renderer.search_line.get_cursor()],
                None,
            );
            StableCursorPosition {
                x: SEARCH_CURSOR_PADDING + cursor,
                y: renderer.compute_search_row(),
                shape: termwiz::surface::CursorShape::SteadyBlock,
                visibility: termwiz::surface::CursorVisibility::Visible,
            }
        } else {
            renderer.cursor
        }
    }

    fn get_current_seqno(&self) -> SequenceNo {
        self.delegate.get_current_seqno()
    }

    fn get_changed_since(
        &self,
        lines: Range<StableRowIndex>,
        seqno: SequenceNo,
    ) -> RangeSet<StableRowIndex> {
        self.delegate.get_changed_since(lines, seqno)
    }

    fn get_logical_lines(&self, lines: Range<StableRowIndex>) -> Vec<LogicalLine> {
        self.delegate.get_logical_lines(lines)
    }

    fn for_each_logical_line_in_stable_range_mut(
        &self,
        lines: Range<StableRowIndex>,
        for_line: &mut dyn ForEachPaneLogicalLine,
    ) {
        self.delegate
            .for_each_logical_line_in_stable_range_mut(lines, for_line);
    }

    fn with_lines_mut(&self, lines: Range<StableRowIndex>, with_lines: &mut dyn WithPaneLines) {
        // Take care to access self.delegate methods here before we get into
        // calling into its own with_lines_mut to avoid a runtime
        // lock erro!
        let mut renderer = self.render.lock();
        if self.delegate.get_current_seqno() > renderer.last_result_seqno {
            renderer.update_search();
        }
        renderer.check_for_resize();
        let dims = self.get_dimensions();
        let search_row = renderer.compute_search_row();

        struct OverlayLines<'a> {
            with_lines: &'a mut dyn WithPaneLines,
            dims: RenderableDimensions,
            search_row: StableRowIndex,
            renderer: &'a mut CopyRenderable,
        }

        self.delegate.with_lines_mut(
            lines,
            &mut OverlayLines {
                with_lines,
                dims,
                search_row,
                renderer: &mut renderer,
            },
        );

        impl<'a> WithPaneLines for OverlayLines<'a> {
            fn with_lines_mut(&mut self, first_row: StableRowIndex, lines: &mut [&mut Line]) {
                let mut overlay_lines = vec![];
                let config = config::configuration();
                let colors = &config.resolved_palette;

                // Bump once per render pass (not per line/cell) so that
                // every mutation made below to the cloned lines in this
                // pass is tagged with a seqno strictly greater than any
                // prior pass. See CopyRenderable::render_seqno doc comment.
                self.renderer.render_seqno += 1;
                let render_seqno = self.renderer.render_seqno;

                for (idx, line) in lines.iter_mut().enumerate() {
                    let mut line: Line = line.clone();

                    let stable_idx = idx as StableRowIndex + first_row;
                    self.renderer.dirty_results.remove(stable_idx);
                    let pattern = self.renderer.get_pattern();
                    if stable_idx == self.search_row
                        && (self.renderer.editing_search || !pattern.is_empty())
                    {
                        // Replace with search UI
                        let rev = CellAttributes::default().set_reverse(true).clone();
                        line.fill_range(
                            0..self.dims.cols,
                            &Cell::new(' ', rev.clone()),
                            render_seqno,
                        );
                        let mode = &match pattern {
                            Pattern::CaseSensitiveString(_) => "case-sensitive",
                            Pattern::CaseInSensitiveString(_) => "ignore-case",
                            Pattern::Regex(_) => "regex",
                        };

                        let remain = match &self.renderer.searching {
                            Some(Searching { remain, .. }) => {
                                format!(" searching {remain} lines")
                            }
                            None => String::new(),
                        };

                        line.overlay_text_with_attribute(
                            0,
                            &format!(
                                "Search: {} ({}/{} matches. {}{remain})",
                                *pattern,
                                self.renderer.result_pos.map(|x| x + 1).unwrap_or(0),
                                self.renderer.results.len(),
                                mode
                            ),
                            rev,
                            render_seqno,
                        );
                        self.renderer.last_bar_pos = Some(self.search_row);
                        line.clear_appdata();
                    } else if let Some(matches) = self.renderer.by_line.get(&stable_idx) {
                        for m in matches {
                            // highlight
                            for cell_idx in m.range.clone() {
                                if let Some(cell) =
                                    line.cells_mut_for_attr_changes_only().get_mut(cell_idx)
                                {
                                    if Some(m.result_index) == self.renderer.result_pos {
                                        cell.attrs_mut()
                                            .set_background(
                                                colors
                                                    .copy_mode_active_highlight_bg
                                                    .unwrap_or(AnsiColor::Yellow.into()),
                                            )
                                            .set_foreground(
                                                colors
                                                    .copy_mode_active_highlight_fg
                                                    .unwrap_or(AnsiColor::Black.into()),
                                            )
                                            .set_reverse(false);
                                    } else {
                                        cell.attrs_mut()
                                            .set_background(
                                                colors
                                                    .copy_mode_inactive_highlight_bg
                                                    .unwrap_or(AnsiColor::Fuchsia.into()),
                                            )
                                            .set_foreground(
                                                colors
                                                    .copy_mode_inactive_highlight_fg
                                                    .unwrap_or(AnsiColor::Black.into()),
                                            )
                                            .set_reverse(false);
                                    }
                                }
                            }
                        }
                        // cells_mut_for_attr_changes_only() mutates cells
                        // directly without bumping the line's seqno; do it
                        // explicitly so downstream seqno-keyed caches see
                        // this pass as a new version of the line.
                        line.update_last_change_seqno(render_seqno);
                        line.clear_appdata();
                    }
                    overlay_lines.push(line);
                }

                let mut overlay_refs: Vec<&mut Line> = overlay_lines.iter_mut().collect();
                self.with_lines.with_lines_mut(first_row, &mut overlay_refs);
            }
        }
    }

    fn get_lines(&self, lines: Range<StableRowIndex>) -> (StableRowIndex, Vec<Line>) {
        let mut renderer = self.render.lock();
        if self.delegate.get_current_seqno() > renderer.last_result_seqno {
            renderer.update_search();
        }

        renderer.check_for_resize();
        let dims = self.get_dimensions();

        let (top, mut lines) = self.delegate.get_lines(lines);

        let config = config::configuration();
        let colors = &config.resolved_palette;

        // Bump once per render pass (not per line/cell); see
        // CopyRenderable::render_seqno doc comment.
        renderer.render_seqno += 1;
        let render_seqno = renderer.render_seqno;

        // Process the lines; for the search row we want to render instead
        // the search UI.
        // For rows with search results, we want to highlight the matching ranges
        let search_row = renderer.compute_search_row();
        for (idx, line) in lines.iter_mut().enumerate() {
            let stable_idx = idx as StableRowIndex + top;
            renderer.dirty_results.remove(stable_idx);
            let pattern = renderer.get_pattern();
            if stable_idx == search_row && (renderer.editing_search || !pattern.is_empty()) {
                // Replace with search UI
                let rev = CellAttributes::default().set_reverse(true).clone();
                line.fill_range(0..dims.cols, &Cell::new(' ', rev.clone()), render_seqno);
                let mode = &match pattern {
                    Pattern::CaseSensitiveString(_) => "case-sensitive",
                    Pattern::CaseInSensitiveString(_) => "ignore-case",
                    Pattern::Regex(_) => "regex",
                };
                line.overlay_text_with_attribute(
                    0,
                    &format!(
                        "Search: {} ({}/{} matches. {})",
                        *pattern,
                        renderer.result_pos.map(|x| x + 1).unwrap_or(0),
                        renderer.results.len(),
                        mode
                    ),
                    rev,
                    render_seqno,
                );
                renderer.last_bar_pos = Some(search_row);
            } else if let Some(matches) = renderer.by_line.get(&stable_idx) {
                for m in matches {
                    // highlight
                    for cell_idx in m.range.clone() {
                        if let Some(cell) = line.cells_mut_for_attr_changes_only().get_mut(cell_idx)
                        {
                            if Some(m.result_index) == renderer.result_pos {
                                cell.attrs_mut()
                                    .set_background(
                                        colors
                                            .copy_mode_active_highlight_bg
                                            .unwrap_or(AnsiColor::Yellow.into()),
                                    )
                                    .set_foreground(
                                        colors
                                            .copy_mode_active_highlight_fg
                                            .unwrap_or(AnsiColor::Black.into()),
                                    )
                                    .set_reverse(false);
                            } else {
                                cell.attrs_mut()
                                    .set_background(
                                        colors
                                            .copy_mode_inactive_highlight_bg
                                            .unwrap_or(AnsiColor::Fuchsia.into()),
                                    )
                                    .set_foreground(
                                        colors
                                            .copy_mode_inactive_highlight_fg
                                            .unwrap_or(AnsiColor::Black.into()),
                                    )
                                    .set_reverse(false);
                            }
                        }
                    }
                }
                // cells_mut_for_attr_changes_only() mutates cells directly
                // without bumping the line's seqno; do it explicitly so
                // downstream seqno-keyed caches see this pass as a new
                // version of the line.
                line.update_last_change_seqno(render_seqno);
            }
        }

        (top, lines)
    }

    fn get_dimensions(&self) -> RenderableDimensions {
        self.delegate.get_dimensions()
    }
}
