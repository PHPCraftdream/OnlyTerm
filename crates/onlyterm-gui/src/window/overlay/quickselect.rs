use crate::selection::{SelectionCoordinate, SelectionRange};
use crate::termwindow::{TermWindow, TermWindowNotif};
use config::keyassignment::{ClipboardCopyDestination, QuickSelectArguments, ScrollbackEraseMode};
use config::ConfigHandle;
use mux::domain::DomainId;
use mux::pane::{
    CachePolicy, ForEachPaneLogicalLine, LogicalLine, Pane, PaneId, Pattern, SearchResult,
    WithPaneLines,
};
use mux::renderable::*;
use onlyterm_term::color::ColorPalette;
use onlyterm_term::{
    Clipboard, Intensity, KeyCode, KeyModifiers, Line, MouseEvent, StableRowIndex, TerminalSize,
};
use parking_lot::{MappedMutexGuard, Mutex};
use rangeset::RangeSet;
use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;
use termwiz::cell::{Cell, CellAttributes};
use termwiz::color::AnsiColor;
use termwiz::surface::SequenceNo;
use url::Url;
use window::WindowOps;

const PATTERNS: [&str; 14] = [
    // markdown_url
    r"\[[^]]*\]\(([^)]+)\)",
    // url
    r"(?:https?://|git@|git://|ssh://|ftp://|file://)\S+",
    // diff_a
    r"--- a/(\S+)",
    // diff_b
    r"\+\+\+ b/(\S+)",
    // docker
    r"sha256:([0-9a-f]{64})",
    // path
    r"(?:[.\w\-@~]+)?(?:/+[.\w\-@]+)+",
    // color
    r"#[0-9a-fA-F]{6}",
    // uuid
    r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}",
    // ipfs
    r"Qm[0-9a-zA-Z]{44}",
    // sha
    r"[0-9a-f]{7,40}",
    // ip
    r"\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}",
    // ipv6
    r"[A-f0-9:]+:+[A-f0-9:]+[%\w\d]+",
    // address
    r"0x[0-9a-fA-F]+",
    // number
    r"[0-9]{4,}",
];
mod labels;
mod render_lines;
use labels::label_matches_selection;
pub use labels::{compute_labels_for_alphabet, compute_labels_for_alphabet_with_preserved_case};
use render_lines::{
    decorate_quickselect_line, MatchResult, QuickSelectMatchHighlights, SearchBarInfo,
};

pub struct QuickSelectOverlay {
    renderer: Mutex<QuickSelectRenderable>,
    delegate: Arc<dyn Pane>,
}

struct QuickSelectRenderable {
    delegate: Arc<dyn Pane>,
    /// The text that the user entered
    pattern: Pattern,
    /// The most recently queried set of matches
    results: Vec<SearchResult>,
    by_line: HashMap<StableRowIndex, Vec<MatchResult>>,
    by_label: HashMap<String, usize>,
    selection: String,

    viewport: Option<StableRowIndex>,
    last_bar_pos: Option<StableRowIndex>,

    dirty_results: RangeSet<StableRowIndex>,
    result_pos: Option<usize>,
    width: usize,
    height: usize,

    /// We use this to cancel ourselves later
    window: ::window::Window,

    config: ConfigHandle,
    args: QuickSelectArguments,

    /// Synthetic sequence number used to mark the overlay's own mutations
    /// (search bar text, match/label highlighting) to the cloned lines it
    /// hands back for rendering. These mutations happen on a clone of the
    /// delegate's line, so they must never be tagged with the delegate's
    /// real seqno space (SEQ_ZERO would be a no-op if the underlying line
    /// already has a higher seqno, since `update_last_change_seqno` only
    /// ever increases). Seeded far above any plausible real terminal seqno
    /// so it can never collide, and incremented once per render pass so
    /// that caches keyed on (pane_id, stable_row, seqno) -- e.g. the GUI's
    /// shape_hash_cache -- correctly treat each render pass as distinct.
    render_seqno: SequenceNo,
}
impl QuickSelectOverlay {
    pub fn with_pane(
        term_window: &TermWindow,
        pane: &Arc<dyn Pane>,
        args: &QuickSelectArguments,
    ) -> Arc<dyn Pane> {
        let viewport = term_window.get_viewport(pane.pane_id());
        let dims = pane.get_dimensions();

        let config = term_window.config.clone();

        let mut pattern = "(?m)(".to_string();
        let mut have_patterns = false;
        if !args.patterns.is_empty() {
            for p in &args.patterns {
                if have_patterns {
                    pattern.push('|');
                }
                pattern.push_str(p);
                have_patterns = true;
            }
        } else {
            // User-provided patterns take precedence over built-ins
            for p in &config.quick_select_patterns {
                if have_patterns {
                    pattern.push('|');
                }
                pattern.push_str(p);
                have_patterns = true;
            }
            if !config.disable_default_quick_select_patterns {
                for p in &PATTERNS {
                    if have_patterns {
                        pattern.push('|');
                    }
                    pattern.push_str(p);
                    have_patterns = true;
                }
            }
        }
        pattern.push(')');

        let pattern = Pattern::Regex(pattern);

        let window = term_window.window.clone().unwrap();
        let mut renderer = QuickSelectRenderable {
            delegate: Arc::clone(pane),
            pattern,
            selection: "".to_string(),
            results: vec![],
            by_line: HashMap::new(),
            by_label: HashMap::new(),
            dirty_results: RangeSet::default(),
            viewport,
            last_bar_pos: None,
            window,
            result_pos: None,
            width: dims.cols,
            height: dims.viewport_rows,
            config,
            args: args.clone(),
            // Seeded far above any plausible real terminal seqno (which
            // starts near 0/1 and increments once per actual content
            // mutation) so it can never collide. See field doc comment.
            render_seqno: usize::MAX / 2,
        };

        let search_row = renderer.compute_search_row();
        renderer.dirty_results.add(search_row);
        renderer.update_search(true);

        Arc::new(QuickSelectOverlay {
            renderer: Mutex::new(renderer),
            delegate: Arc::clone(pane),
        })
    }

    pub fn viewport_changed(&self, viewport: Option<StableRowIndex>) {
        let mut render = self.renderer.lock();
        if render.viewport != viewport {
            if let Some(last) = render.last_bar_pos.take() {
                render.dirty_results.add(last);
            }
            if let Some(pos) = viewport.as_ref() {
                render.dirty_results.add(*pos);
            }
            render.viewport = viewport;
        }
    }
}

impl Pane for QuickSelectOverlay {
    fn pane_id(&self) -> PaneId {
        self.delegate.pane_id()
    }

    fn get_title(&self) -> String {
        self.delegate.get_title()
    }

    fn send_paste(&self, _text: &str) -> anyhow::Result<()> {
        // Ignore
        Ok(())
    }

    fn reader(&self) -> anyhow::Result<Option<Box<dyn std::io::Read + Send>>> {
        Ok(None)
    }

    fn writer(&self) -> MappedMutexGuard<'_, dyn std::io::Write> {
        self.delegate.writer()
    }

    fn resize(&self, size: TerminalSize) -> anyhow::Result<()> {
        self.delegate.resize(size)
    }

    fn key_up(&self, _key: KeyCode, _mods: KeyModifiers) -> anyhow::Result<()> {
        Ok(())
    }

    fn key_down(&self, key: KeyCode, mods: KeyModifiers) -> anyhow::Result<()> {
        let mods = mods.remove_positional_mods();
        match (key, mods) {
            (KeyCode::Escape, KeyModifiers::NONE) => self.renderer.lock().close(),
            (KeyCode::UpArrow, KeyModifiers::NONE)
            | (KeyCode::Enter, KeyModifiers::NONE)
            | (KeyCode::Char('p'), KeyModifiers::CTRL) => {
                // Move to prior match
                let mut r = self.renderer.lock();
                if let Some(cur) = r.result_pos.as_ref() {
                    let prior = if *cur > 0 {
                        cur - 1
                    } else {
                        r.results.len() - 1
                    };
                    r.activate_match_number(prior);
                }
            }
            (KeyCode::PageUp, KeyModifiers::NONE) => {
                // Skip this page of matches and move up to the first match from
                // the prior page.
                let dims = self.delegate.get_dimensions();
                let mut r = self.renderer.lock();
                if let Some(cur) = r.result_pos {
                    let top = r.viewport.unwrap_or(dims.physical_top);
                    let prior = top - dims.viewport_rows as isize;
                    if let Some(pos) = r
                        .results
                        .iter()
                        .position(|res| res.start_y > prior && res.start_y < top)
                    {
                        r.activate_match_number(pos);
                    } else {
                        r.activate_match_number(cur.saturating_sub(1));
                    }
                }
            }
            (KeyCode::PageDown, KeyModifiers::NONE) => {
                // Skip this page of matches and move down to the first match from
                // the next page.
                let dims = self.delegate.get_dimensions();
                let mut r = self.renderer.lock();
                if let Some(cur) = r.result_pos {
                    let top = r.viewport.unwrap_or(dims.physical_top);
                    let bottom = top + dims.viewport_rows as isize;
                    if let Some(pos) = r.results.iter().position(|res| res.start_y >= bottom) {
                        r.activate_match_number(pos);
                    } else {
                        let len = r.results.len().saturating_sub(1);
                        r.activate_match_number(cur.min(len));
                    }
                }
            }
            (KeyCode::DownArrow, KeyModifiers::NONE) | (KeyCode::Char('n'), KeyModifiers::CTRL) => {
                // Move to next match
                let mut r = self.renderer.lock();
                if let Some(cur) = r.result_pos.as_ref() {
                    let next = if *cur + 1 >= r.results.len() {
                        0
                    } else {
                        *cur + 1
                    };
                    r.activate_match_number(next);
                }
            }
            (KeyCode::Char(c), KeyModifiers::NONE) | (KeyCode::Char(c), KeyModifiers::SHIFT) => {
                // Type to add to the selection
                let mut r = self.renderer.lock();
                r.selection.push(c);
                let lowered = r.selection.to_lowercase();
                let paste = lowered != r.selection;
                if let Some(result_index) = r.by_label.get(&lowered).cloned() {
                    r.select_and_copy_match_number(result_index, paste);
                    r.close();
                } else {
                    r.recompute_results();
                }
            }
            (KeyCode::Backspace, KeyModifiers::NONE) => {
                // Backspace to edit the selection
                let mut r = self.renderer.lock();
                r.selection.pop();
                r.recompute_results();
            }
            (KeyCode::Char('u'), KeyModifiers::CTRL) => {
                // CTRL-u to clear the selection
                let mut r = self.renderer.lock();
                r.selection.clear();
                r.recompute_results();
            }
            _ => {}
        }
        Ok(())
    }

    fn mouse_event(&self, event: MouseEvent) -> anyhow::Result<()> {
        self.delegate.mouse_event(event)
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
        // move to the search box
        let renderer = self.renderer.lock();
        StableCursorPosition {
            x: 8 + onlyterm_term::unicode_column_width(&renderer.selection, None),
            y: renderer.compute_search_row(),
            shape: termwiz::surface::CursorShape::SteadyBlock,
            visibility: termwiz::surface::CursorVisibility::Visible,
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
        let mut dirty = self.delegate.get_changed_since(lines.clone(), seqno);
        dirty.add_set(&self.renderer.lock().dirty_results);
        dirty.intersection_with_range(lines)
    }

    fn for_each_logical_line_in_stable_range_mut(
        &self,
        lines: Range<StableRowIndex>,
        for_line: &mut dyn ForEachPaneLogicalLine,
    ) {
        self.delegate
            .for_each_logical_line_in_stable_range_mut(lines, for_line);
    }

    fn get_logical_lines(&self, lines: Range<StableRowIndex>) -> Vec<LogicalLine> {
        self.delegate.get_logical_lines(lines)
    }

    fn with_lines_mut(&self, lines: Range<StableRowIndex>, with_lines: &mut dyn WithPaneLines) {
        let mut renderer = self.renderer.lock();
        // Take care to access self.delegate methods here before we get into
        // calling into its own with_lines_mut to avoid a runtime
        // borrow erro!
        renderer.check_for_resize();
        let dims = self.get_dimensions();
        let search_row = renderer.compute_search_row();

        struct OverlayLines<'a> {
            with_lines: &'a mut dyn WithPaneLines,
            dims: RenderableDimensions,
            search_row: StableRowIndex,
            renderer: &'a mut QuickSelectRenderable,
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

                let config = &self.renderer.config;
                let colors = config.resolved_palette.clone();
                let disable_attr = config.quick_select_remove_styling;

                // Bump once per render pass (not per line/cell) so that
                // every mutation made below to the cloned lines in this
                // pass is tagged with a seqno strictly greater than any
                // prior pass. See QuickSelectRenderable::render_seqno doc
                // comment.
                self.renderer.render_seqno += 1;
                let render_seqno = self.renderer.render_seqno;

                // Process the lines; for the search row we want to render instead
                // the search UI.
                // For rows with search results, we want to highlight the matching ranges

                let lowered_prefix = self.renderer.selection.to_lowercase();
                for (idx, line) in lines.iter_mut().enumerate() {
                    let mut line: Line = line.clone();
                    let stable_idx = idx as StableRowIndex + first_row;
                    self.renderer.dirty_results.remove(stable_idx);
                    let is_search_row = stable_idx == self.search_row;
                    if is_search_row {
                        self.renderer.last_bar_pos = Some(self.search_row);
                    }
                    let search_bar = SearchBarInfo {
                        cols: self.dims.cols,
                        selection: &self.renderer.selection,
                        label: &self.renderer.args.label,
                    };
                    decorate_quickselect_line(
                        &mut line,
                        render_seqno,
                        disable_attr,
                        is_search_row,
                        &search_bar,
                        QuickSelectMatchHighlights {
                            matches: self.renderer.by_line.get(&stable_idx).map(Vec::as_slice),
                            filter_by_selection: Some(&lowered_prefix),
                        },
                        &colors,
                    );
                    overlay_lines.push(line);
                }

                let mut overlay_refs: Vec<&mut Line> = overlay_lines.iter_mut().collect();
                self.with_lines.with_lines_mut(first_row, &mut overlay_refs);
            }
        }
    }

    fn get_lines(&self, lines: Range<StableRowIndex>) -> (StableRowIndex, Vec<Line>) {
        let mut renderer = self.renderer.lock();
        renderer.check_for_resize();
        let dims = self.get_dimensions();

        let (top, mut lines) = self.delegate.get_lines(lines);
        let colors = renderer.config.resolved_palette.clone();
        let disable_attr = renderer.config.quick_select_remove_styling;

        // Bump once per render pass (not per line/cell); see
        // QuickSelectRenderable::render_seqno doc comment.
        renderer.render_seqno += 1;
        let render_seqno = renderer.render_seqno;

        // Process the lines; for the search row we want to render instead
        // the search UI.
        // For rows with search results, we want to highlight the matching ranges
        let search_row = renderer.compute_search_row();
        for (idx, line) in lines.iter_mut().enumerate() {
            let stable_idx = idx as StableRowIndex + top;
            renderer.dirty_results.remove(stable_idx);
            let is_search_row = stable_idx == search_row;
            if is_search_row {
                renderer.last_bar_pos = Some(search_row);
            }
            let search_bar = SearchBarInfo {
                cols: dims.cols,
                selection: &renderer.selection,
                label: &renderer.args.label,
            };
            decorate_quickselect_line(
                line,
                render_seqno,
                disable_attr,
                is_search_row,
                &search_bar,
                QuickSelectMatchHighlights {
                    matches: renderer.by_line.get(&stable_idx).map(Vec::as_slice),
                    // get_lines historically does not filter by selection
                    // prefix; see decorate_quickselect_line's doc comment.
                    filter_by_selection: None,
                },
                &colors,
            );
        }

        (top, lines)
    }

    fn get_dimensions(&self) -> RenderableDimensions {
        self.delegate.get_dimensions()
    }
}

impl QuickSelectRenderable {
    fn compute_search_row(&self) -> StableRowIndex {
        let dims = self.delegate.get_dimensions();
        let top = self.viewport.unwrap_or(dims.physical_top);

        (top + dims.viewport_rows as StableRowIndex).saturating_sub(1)
    }

    fn close(&self) {
        TermWindow::schedule_cancel_overlay_for_pane(self.window.clone(), self.delegate.pane_id());
    }

    fn set_viewport(&self, row: Option<StableRowIndex>) {
        let dims = self.delegate.get_dimensions();
        let pane_id = self.delegate.pane_id();
        self.window
            .notify(TermWindowNotif::Apply(Box::new(move |term_window| {
                term_window.set_viewport(pane_id, row, dims);
            })));
    }

    fn check_for_resize(&mut self) {
        let dims = self.delegate.get_dimensions();
        if dims.cols == self.width && dims.viewport_rows == self.height {
            return;
        }

        self.width = dims.cols;
        self.height = dims.viewport_rows;

        let pos = self.result_pos;
        self.update_search(false);
        self.result_pos = pos;
    }

    fn recompute_results(&mut self) {
        /// Produce the sorted seq of unique match_ids from the results
        fn compute_uniq_results(results: &[SearchResult]) -> Vec<usize> {
            let mut ids: Vec<usize> = results.iter().map(|sr| sr.match_id).collect();
            ids.sort();
            ids.dedup();
            ids
        }

        let uniq_results = compute_uniq_results(&self.results);

        // Label each unique result
        let labels = compute_labels_for_alphabet(
            if !self.args.alphabet.is_empty() {
                &self.args.alphabet
            } else {
                &self.config.quick_select_alphabet
            },
            uniq_results.len(),
        );
        self.by_label.clear();

        // Keep track of match_id -> label
        let mut assigned_labels: HashMap<usize, usize> = HashMap::new();

        // Work through the results in reverse order, so that we assign eg: `a` to the
        // bottom-right-most result first and so on
        for (result_index, res) in self.results.iter().enumerate().rev() {
            // Figure out which label to use based on the match_id
            let label_index = match assigned_labels.get(&res.match_id).copied() {
                Some(idx) => idx,
                None => {
                    let idx = assigned_labels.len();
                    assigned_labels.insert(res.match_id, idx);
                    idx
                }
            };
            let label = match labels.get(label_index) {
                Some(l) => l,
                None => {
                    // There are more result candidates than the alphabet
                    // can support, so we skip this one and keep looking:
                    // we may still have matches that have an assigned
                    // label, so we keep going rather than breaking
                    // out of the loop.
                    continue;
                }
            };

            self.by_label.entry(label.clone()).or_insert(result_index);
            for idx in res.start_y..=res.end_y {
                let range = if idx == res.start_y && idx == res.end_y {
                    // Range on same line
                    res.start_x..res.end_x
                } else if idx == res.end_y {
                    // final line of multi-line
                    0..res.end_x
                } else if idx == res.start_y {
                    // first line of multi-line
                    res.start_x..self.width
                } else {
                    // a middle line
                    0..self.width
                };

                let result = MatchResult {
                    range,
                    label: label.clone(),
                };

                let matches = self.by_line.entry(idx).or_default();
                matches.push(result);

                self.dirty_results.add(idx);
            }
        }
    }

    fn update_search(&mut self, is_initial_run: bool) {
        for idx in self.by_line.keys() {
            self.dirty_results.add(*idx);
        }
        if let Some(idx) = self.last_bar_pos.as_ref() {
            self.dirty_results.add(*idx);
        }

        self.results.clear();
        self.by_line.clear();
        self.result_pos.take();

        let bar_pos = self.compute_search_row();
        self.dirty_results.add(bar_pos);

        if !self.pattern.is_empty() {
            let pane: Arc<dyn Pane> = self.delegate.clone();
            let window = self.window.clone();
            let pattern = self.pattern.clone();
            let scope = self.args.scope_lines;
            let viewport = self.viewport;
            promise::spawn::spawn(async move {
                let dims = pane.get_dimensions();
                let scope = scope.unwrap_or(1000).max(dims.viewport_rows);
                let top = viewport.unwrap_or(dims.physical_top);
                let range = top.saturating_sub(scope as StableRowIndex)
                    ..top + (dims.viewport_rows + scope) as StableRowIndex;
                let limit = None;
                let mut results = pane.search(pattern, range, limit).await?;
                results.sort();

                let pane_id = pane.pane_id();
                let mut results = Some(results);
                window.notify(TermWindowNotif::Apply(Box::new(move |term_window| {
                    let state = term_window.pane_state(pane_id);
                    if let Some(overlay) = state.overlay.as_ref() {
                        if let Some(search_overlay) =
                            overlay.pane.downcast_ref::<QuickSelectOverlay>()
                        {
                            let mut r = search_overlay.renderer.lock();
                            r.results = results.take().unwrap();
                            r.recompute_results();
                            let num_results = r.results.len();

                            if !r.results.is_empty() {
                                match &r.viewport {
                                    Some(y) if is_initial_run => {
                                        r.result_pos = r
                                            .results
                                            .iter()
                                            .position(|result| result.start_y >= *y);
                                    }
                                    _ => {
                                        r.activate_match_number(num_results - 1);
                                    }
                                }
                            } else {
                                if !is_initial_run {
                                    r.set_viewport(None);
                                }
                                r.clear_selection();
                            }
                        }
                    }
                })));
                anyhow::Result::<()>::Ok(())
            })
            .detach();
        } else {
            if !is_initial_run {
                self.set_viewport(None);
            }
            self.clear_selection();
        }
    }

    fn clear_selection(&mut self) {
        let pane_id = self.delegate.pane_id();
        self.window
            .notify(TermWindowNotif::Apply(Box::new(move |term_window| {
                let mut selection = term_window.selection(pane_id);
                selection.origin.take();
                selection.range.take();
            })));
    }

    fn select_and_copy_match_number(&mut self, n: usize, paste: bool) {
        let result = self.results[n];

        let pane_id = self.delegate.pane_id();
        let action = self.args.action.clone();
        let skip_action_on_paste = self.args.skip_action_on_paste;
        self.window
            .notify(TermWindowNotif::Apply(Box::new(move |term_window| {
                let mux = mux::Mux::get();
                if let Some(pane) = mux.get_pane(pane_id) {
                    {
                        let mut selection = term_window.selection(pane_id);
                        let start = SelectionCoordinate::x_y(result.start_x, result.start_y);
                        selection.origin = Some(start);
                        selection.range = Some(SelectionRange {
                            start,
                            // inclusive range for selection, but the result
                            // range is exclusive
                            end: SelectionCoordinate::x_y(
                                result.end_x.saturating_sub(1),
                                result.end_y,
                            ),
                        });
                        // Ensure that selection doesn't get invalidated when
                        // the overlay is closed
                        selection.seqno = pane.get_current_seqno();
                    }

                    let text = term_window.selection_text(&pane);
                    if !text.is_empty() {
                        if paste {
                            let _ = pane.send_paste(&text);
                        }
                        if let Some(action) = action {
                            if !paste || !skip_action_on_paste {
                                let _ = term_window.perform_key_assignment(&pane, &action);
                            }
                        } else {
                            term_window.copy_to_clipboard(
                                ClipboardCopyDestination::ClipboardAndPrimarySelection,
                                text,
                            );
                        }
                    }
                }
            })));
    }

    fn activate_match_number(&mut self, n: usize) {
        self.result_pos.replace(n);
        let result = self.results[n];
        self.set_viewport(Some(result.start_y));
    }
}
