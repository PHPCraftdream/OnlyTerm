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

/// Plain-data view of a search-bar render for a single line, used by
/// [`decorate_copy_line`]. Kept as owned/borrowed data (no `&self`) so the
/// decoration logic can be exercised directly against a real `Line` from a
/// unit test.
struct CopySearchBarInfo<'a> {
    cols: usize,
    pattern: &'a Pattern,
    result_pos: Option<usize>,
    num_results: usize,
    /// Extra "searching N lines" suffix shown while an async search is
    /// still in flight. `with_lines_mut` includes this; `get_lines`
    /// historically does not -- preserve that pre-existing asymmetry by
    /// passing `None` from `get_lines`.
    searching_remain: Option<StableRowIndex>,
}

/// The search matches (if any) that fall on the line being decorated, plus
/// which result (by index) is the "active" one, used by
/// [`decorate_copy_line`] to choose active-vs-inactive highlight colors.
struct CopyMatchHighlights<'a> {
    matches: Option<&'a [MatchResult]>,
    active_result_index: Option<usize>,
}

/// Decorates a single cloned pane `Line` for one CopyMode render pass:
/// either replacing it with the search UI bar, or highlighting any search
/// matches that fall on this row. This is the exact per-line logic used by
/// both `Pane::with_lines_mut` and `Pane::get_lines` for `CopyOverlay` --
/// factored out so it can be unit tested against a real `Line` without a
/// live `window::Window`.
///
/// `render_seqno` must be used for every mutating call so that downstream
/// seqno-keyed caches (e.g. the GUI's shape_hash_cache) see each render pass
/// as a distinct version of the line; see `CopyRenderable::render_seqno`.
fn decorate_copy_line(
    line: &mut Line,
    render_seqno: SequenceNo,
    is_search_row: bool,
    search_bar: &CopySearchBarInfo,
    highlights: CopyMatchHighlights,
    colors: &config::Palette,
    // `with_lines_mut` operates on clones of lines that may carry forward
    // stale cached appdata, so it explicitly clears it after mutating.
    // `get_lines` historically does not do this. Preserve that pre-existing
    // asymmetry via this flag rather than changing behavior as part of this
    // refactor.
    clear_appdata: bool,
) {
    if is_search_row {
        // Replace with search UI
        let rev = CellAttributes::default().set_reverse(true).clone();
        line.fill_range(
            0..search_bar.cols,
            &Cell::new(' ', rev.clone()),
            render_seqno,
        );
        let mode = &match search_bar.pattern {
            Pattern::CaseSensitiveString(_) => "case-sensitive",
            Pattern::CaseInSensitiveString(_) => "ignore-case",
            Pattern::Regex(_) => "regex",
        };

        let remain = match search_bar.searching_remain {
            Some(remain) => format!(" searching {remain} lines"),
            None => String::new(),
        };

        line.overlay_text_with_attribute(
            0,
            &format!(
                "Search: {} ({}/{} matches. {}{remain})",
                **search_bar.pattern,
                search_bar.result_pos.map(|x| x + 1).unwrap_or(0),
                search_bar.num_results,
                mode
            ),
            rev,
            render_seqno,
        );
        if clear_appdata {
            line.clear_appdata();
        }
        return;
    }

    let Some(matches) = highlights.matches else {
        return;
    };

    for m in matches {
        // highlight
        for cell_idx in m.range.clone() {
            if let Some(cell) = line.cells_mut_for_attr_changes_only().get_mut(cell_idx) {
                if Some(m.result_index) == highlights.active_result_index {
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
    // cells_mut_for_attr_changes_only() mutates cells directly without
    // bumping the line's seqno; do it explicitly so downstream seqno-keyed
    // caches see this pass as a new version of the line.
    line.update_last_change_seqno(render_seqno);
    if clear_appdata {
        line.clear_appdata();
    }
}

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
                    let is_search_row = stable_idx == self.search_row
                        && (self.renderer.editing_search || !pattern.is_empty());
                    if is_search_row {
                        self.renderer.last_bar_pos = Some(self.search_row);
                    }
                    let searching_remain = self
                        .renderer
                        .searching
                        .as_ref()
                        .map(|Searching { remain, .. }| *remain);
                    let search_bar = CopySearchBarInfo {
                        cols: self.dims.cols,
                        pattern: &pattern,
                        result_pos: self.renderer.result_pos,
                        num_results: self.renderer.results.len(),
                        searching_remain,
                    };
                    decorate_copy_line(
                        &mut line,
                        render_seqno,
                        is_search_row,
                        &search_bar,
                        CopyMatchHighlights {
                            matches: self.renderer.by_line.get(&stable_idx).map(Vec::as_slice),
                            active_result_index: self.renderer.result_pos,
                        },
                        colors,
                        /* clear_appdata */ true,
                    );
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
            let is_search_row =
                stable_idx == search_row && (renderer.editing_search || !pattern.is_empty());
            if is_search_row {
                renderer.last_bar_pos = Some(search_row);
            }
            let search_bar = CopySearchBarInfo {
                cols: dims.cols,
                pattern: &pattern,
                result_pos: renderer.result_pos,
                num_results: renderer.results.len(),
                // get_lines historically does not show the "searching N
                // lines" suffix; see decorate_copy_line's doc comment.
                searching_remain: None,
            };
            decorate_copy_line(
                line,
                render_seqno,
                is_search_row,
                &search_bar,
                CopyMatchHighlights {
                    matches: renderer.by_line.get(&stable_idx).map(Vec::as_slice),
                    active_result_index: renderer.result_pos,
                },
                colors,
                // get_lines historically does not clear_appdata; see
                // decorate_copy_line's doc comment.
                /* clear_appdata */
                false,
            );
        }

        (top, lines)
    }

    fn get_dimensions(&self) -> RenderableDimensions {
        self.delegate.get_dimensions()
    }
}

/// Regression tests for the overlay-render-seqno fix (facb0646e).
///
/// `CopyOverlay` mutates a *clone* of the delegate pane's `Line` on every
/// render pass (search bar text, match highlighting). Those clones inherit
/// whatever seqno the delegate's real line already has. Before the fix,
/// the overlay tagged its own mutations with `SEQ_ZERO`; since
/// `Line::update_last_change_seqno` only ever increases a line's seqno
/// (`self.seqno = self.seqno.max(seqno)`), tagging with SEQ_ZERO on a line
/// whose seqno was already > 0 was a silent no-op. Downstream per-frame
/// caches keyed on (pane_id, stable_row, seqno) -- e.g. the GUI's
/// shape_hash_cache -- would then treat the overlay's mutated frame as
/// identical to a stale pre-overlay frame and keep serving the old shape,
/// so the search bar / highlights could silently fail to appear.
///
/// These tests call the real `decorate_copy_line` function used by
/// `CopyOverlay::with_lines_mut`/`get_lines` directly against a real `Line`,
/// without needing a live TermWindow/GUI (that function takes no
/// `&self`/`Window`, only plain data). This means reverting
/// `decorate_copy_line` (or its callers) back to tagging mutations with
/// `SEQ_ZERO` makes these tests fail.
#[cfg(test)]
mod render_seqno_test {
    use super::*;
    use termwiz::surface::SEQ_ZERO;

    /// A line that already has server/terminal content at a real seqno,
    /// simulating a static (unchanging) pane that has been rendered once
    /// before an overlay was activated.
    fn make_line_with_seqno(seqno: usize) -> Line {
        let mut line = Line::with_width(10, SEQ_ZERO);
        line.fill_range(0..10, &Cell::new(' ', CellAttributes::default()), seqno);
        line
    }

    #[test]
    fn seq_zero_mutation_is_a_no_op_on_nonzero_line() {
        // This documents *why* the bug existed: update_last_change_seqno
        // never decreases the seqno, so re-tagging with SEQ_ZERO after a
        // real mutation leaves current_seqno() unchanged.
        let mut line = make_line_with_seqno(5);
        assert_eq!(line.current_seqno(), 5);

        line.fill_range(0..10, &Cell::new('x', CellAttributes::default()), SEQ_ZERO);
        assert_eq!(
            line.current_seqno(),
            5,
            "SEQ_ZERO must not appear to change a line that already has a higher seqno"
        );
    }

    /// Two successive real render passes (via `decorate_copy_line`) over
    /// the SAME underlying static delegate line (same seqno both times, as
    /// happens while the user types into the copy-mode search bar but the
    /// underlying pane content does not change) must produce distinct
    /// `current_seqno()` values on the decorated clones, and those values
    /// must exceed the delegate's real seqno. If `decorate_copy_line` were
    /// reverted to stamp mutations with `SEQ_ZERO` instead of
    /// `render_seqno`, both assertions below would fail.
    #[test]
    fn copy_search_bar_render_bumps_seqno_past_delegate_and_across_passes() {
        let delegate_seqno = 5;
        let pattern_1 = Pattern::CaseSensitiveString("a".to_string());
        let pattern_2 = Pattern::CaseSensitiveString("ab".to_string());

        let search_bar_1 = CopySearchBarInfo {
            cols: 10,
            pattern: &pattern_1,
            result_pos: None,
            num_results: 0,
            searching_remain: None,
        };
        let search_bar_2 = CopySearchBarInfo {
            cols: 10,
            pattern: &pattern_2,
            result_pos: None,
            num_results: 0,
            searching_remain: None,
        };

        let mut line1 = make_line_with_seqno(delegate_seqno);
        let pass1_seqno = usize::MAX / 2 + 1;
        decorate_copy_line(
            &mut line1,
            pass1_seqno,
            true, // is_search_row
            &search_bar_1,
            CopyMatchHighlights {
                matches: None,
                active_result_index: None,
            },
            &config::Palette::default(),
            true,
        );

        let mut line2 = make_line_with_seqno(delegate_seqno);
        let pass2_seqno = usize::MAX / 2 + 2;
        decorate_copy_line(
            &mut line2,
            pass2_seqno,
            true, // is_search_row
            &search_bar_2,
            CopyMatchHighlights {
                matches: None,
                active_result_index: None,
            },
            &config::Palette::default(),
            true,
        );

        assert_eq!(line1.current_seqno(), pass1_seqno);
        assert_eq!(line2.current_seqno(), pass2_seqno);
        assert_ne!(
            line1.current_seqno(),
            line2.current_seqno(),
            "two different overlay render passes over the same underlying \
             line must produce different current_seqno() values, so a \
             (pane_id, stable_row, seqno)-keyed cache warmed by pass 1 is \
             correctly invalidated for pass 2's different content"
        );
        assert!(line1.current_seqno() > delegate_seqno);
        assert!(line2.current_seqno() > delegate_seqno);

        // Content sanity: the search bar text was actually written.
        let text: String = (0..10)
            .filter_map(|i| line1.get_cell(i).map(|c| c.str().to_string()))
            .collect();
        assert!(text.starts_with("Search"));
    }

    /// The match-highlight branch of `decorate_copy_line` uses
    /// `cells_mut_for_attr_changes_only()`, which does NOT itself bump the
    /// line's seqno tracking -- the function must call
    /// `update_last_change_seqno` explicitly afterwards. This exercises
    /// that branch directly.
    #[test]
    fn copy_match_highlight_requires_explicit_seqno_bump() {
        let delegate_seqno = 5;
        let mut line = make_line_with_seqno(delegate_seqno);
        assert_eq!(line.current_seqno(), delegate_seqno);

        let matches = vec![MatchResult {
            range: 0..1,
            result_index: 0,
        }];
        let pattern = Pattern::default();
        let search_bar = CopySearchBarInfo {
            cols: 10,
            pattern: &pattern,
            result_pos: None,
            num_results: 0,
            searching_remain: None,
        };
        let render_seqno = usize::MAX / 2 + 1;
        decorate_copy_line(
            &mut line,
            render_seqno,
            false, // not the search row: takes the match-highlight branch
            &search_bar,
            CopyMatchHighlights {
                matches: Some(&matches),
                active_result_index: Some(0),
            },
            &config::Palette::default(),
            true,
        );

        assert_eq!(
            line.current_seqno(),
            render_seqno,
            "decorate_copy_line's match-highlight branch mutates cells \
             directly via cells_mut_for_attr_changes_only(), which does not \
             itself bump the seqno -- an explicit \
             update_last_change_seqno(render_seqno) call is required, and \
             tagging with SEQ_ZERO here would be a silent no-op on this \
             already-nonzero-seqno line"
        );
        assert!(line.current_seqno() > delegate_seqno);
    }
}
