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

/// This function computes a set of labels for a given alphabet.
/// It is derived from https://github.com/fcsonline/tmux-thumbs/blob/master/src/alphabets.rs
/// which is Copyright (c) 2019 Ferran Basora and provided under the MIT license
pub fn compute_labels_for_alphabet(alphabet: &str, num_matches: usize) -> Vec<String> {
    compute_labels_for_alphabet_impl(alphabet, num_matches, true)
}

pub fn compute_labels_for_alphabet_with_preserved_case(
    alphabet: &str,
    num_matches: usize,
) -> Vec<String> {
    compute_labels_for_alphabet_impl(alphabet, num_matches, false)
}

fn compute_labels_for_alphabet_impl(
    alphabet: &str,
    num_matches: usize,
    make_lowercase: bool,
) -> Vec<String> {
    let alphabet = if make_lowercase {
        alphabet
            .chars()
            .map(|c| c.to_lowercase().to_string())
            .collect::<Vec<String>>()
    } else {
        alphabet
            .chars()
            .map(|c| c.to_string())
            .collect::<Vec<String>>()
    };
    // Prefer to use single character matches to represent everything
    let mut primary = alphabet.clone();
    let mut secondary = vec![];

    loop {
        if primary.len() + secondary.len() >= num_matches {
            break;
        }

        // We have more matches than can be represented by alphabet,
        // so steal one of the single character options from the end
        // of the alphabet and use it to generate a two character
        // label
        let prefix = match primary.pop() {
            Some(p) => p,
            None => break,
        };

        // Generate a two character label for each of the alphabet
        // characters.  This ignores later alphabet characters;
        // since we popped our prefix from the end of alphabet,
        // length limiting this iteration ensures that we don't
        // end up with a duplicate letters in the result.
        let prefixed: Vec<String> = alphabet
            .iter()
            .take(num_matches - primary.len() - secondary.len())
            .map(|s| format!("{}{}", prefix, s))
            .collect();

        secondary.splice(0..0, prefixed);
    }

    let len = secondary.len();

    primary
        .drain(0..)
        .take(num_matches - len)
        .chain(secondary.drain(0..))
        .collect()
}

/// Returns true if a label should be displayed given a selection prefix.
fn label_matches_selection(label: &str, lowered_prefix: &str) -> bool {
    lowered_prefix.is_empty() || label.starts_with(lowered_prefix)
}

#[cfg(test)]
mod alphabet_test {
    use super::*;

    #[test]
    fn simple_alphabet() {
        assert_eq!(compute_labels_for_alphabet("abcd", 3), vec!["a", "b", "c"]);
    }

    #[test]
    fn more_matches_than_alphabet_can_represent() {
        assert_eq!(
            compute_labels_for_alphabet("asdfqwerzxcvjklmiuopghtybn", 792).len(),
            676
        );
    }

    #[test]
    fn composed_single() {
        assert_eq!(
            compute_labels_for_alphabet("abcd", 6),
            vec!["a", "b", "c", "da", "db", "dc"]
        );
    }

    #[test]
    fn composed_multiple() {
        assert_eq!(
            compute_labels_for_alphabet("abcd", 8),
            vec!["a", "b", "ca", "cb", "da", "db", "dc", "dd"]
        );
    }

    #[test]
    fn composed_max() {
        // The number of chars in the alphabet limits the potential matches to fewer
        // than the number of matches that we requested
        assert_eq!(
            compute_labels_for_alphabet("ab", 5),
            vec!["aa", "ab", "ba", "bb"]
        );
    }

    #[test]
    fn composed_capital() {
        assert_eq!(
            compute_labels_for_alphabet_with_preserved_case("AB", 4),
            vec!["AA", "AB", "BA", "BB"]
        );
    }

    #[test]
    fn composed_mixed() {
        assert_eq!(
            compute_labels_for_alphabet_with_preserved_case("aA", 4),
            vec!["aa", "aA", "Aa", "AA"]
        );
    }

    #[test]
    fn lowercase_alphabet_equal() {
        assert_eq!(
            compute_labels_for_alphabet_with_preserved_case("abc123", 12),
            compute_labels_for_alphabet("abc123", 12)
        );
    }
}

#[cfg(test)]
mod label_filter_test {
    use super::*;

    #[test]
    fn empty_prefix_matches_all() {
        let labels = ["a", "fq", "db", "av", "ac"];
        assert!(labels.iter().all(|l| label_matches_selection(l, "")));
    }

    #[test]
    fn single_char_prefix_filters_non_matching() {
        // Pressing 'a' should keep labels starting with 'a' and remove others
        let labels = ["ai", "fq", "db", "av", "ac"];
        let visible: Vec<&str> = labels
            .iter()
            .copied()
            .filter(|l| label_matches_selection(l, "a"))
            .collect();
        assert_eq!(visible, ["ai", "av", "ac"]);
    }

    #[test]
    fn full_prefix_matches_exact_label() {
        assert!(label_matches_selection("ai", "ai"));
        assert!(!label_matches_selection("ac", "ai"));
    }

    #[test]
    fn prefix_longer_than_label() {
        assert!(!label_matches_selection("a", "ab"));
        assert!(!label_matches_selection("", "a"));
    }

    #[test]
    fn two_char_labels_narrowed_by_first_char() {
        // With more matches than alphabet, two-char labels are generated
        let labels = compute_labels_for_alphabet("abcd", 6);
        assert_eq!(labels, ["a", "b", "c", "da", "db", "dc"]);

        // Typing 'd' should keep "da", "db", "dc" and remove "a", "b", "c"
        let visible: Vec<&str> = labels
            .iter()
            .map(String::as_str)
            .filter(|l| label_matches_selection(l, "d"))
            .collect();
        assert_eq!(visible, ["da", "db", "dc"]);
    }

    #[test]
    fn no_labels_match_unknown_prefix() {
        let labels = ["ai", "fq", "db"];
        assert!(!labels.iter().any(|l| label_matches_selection(l, "z")));
    }
}

pub struct QuickSelectOverlay {
    renderer: Mutex<QuickSelectRenderable>,
    delegate: Arc<dyn Pane>,
}

#[derive(Debug)]
struct MatchResult {
    range: Range<usize>,
    label: String,
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

/// Plain-data view of a search-bar render for a single line, used by
/// [`decorate_quickselect_line`]. Kept as owned/borrowed data (no `&self`,
/// no `Window`) so the decoration logic can be exercised directly against a
/// real `Line` from a unit test.
struct SearchBarInfo<'a> {
    cols: usize,
    selection: &'a str,
    label: &'a str,
}

/// The matches (if any) that fall on the line being decorated, plus the
/// selection-prefix filter used by [`decorate_quickselect_line`] to decide
/// which labels are currently visible.
struct QuickSelectMatchHighlights<'a> {
    matches: Option<&'a [MatchResult]>,
    // `with_lines_mut` filters out labels that don't match the current
    // selection prefix (so unmatched labels visually disappear as the user
    // types); `get_lines` historically does not apply this filter. Preserve
    // that pre-existing asymmetry via this flag rather than changing
    // behavior as part of this refactor.
    filter_by_selection: Option<&'a str>,
}

/// Decorates a single cloned pane `Line` for one QuickSelect render pass:
/// either replacing it with the search UI bar, or highlighting any matches/
/// labels that fall on this row. This is the exact per-line logic used by
/// both `Pane::with_lines_mut` and `Pane::get_lines` for `QuickSelectOverlay`
/// -- factored out so it can be unit tested against a real `Line` without a
/// live `window::Window`.
///
/// `render_seqno` must be used for every mutating call so that downstream
/// seqno-keyed caches (e.g. the GUI's shape_hash_cache) see each render pass
/// as a distinct version of the line; see `QuickSelectRenderable::render_seqno`.
fn decorate_quickselect_line(
    line: &mut Line,
    render_seqno: SequenceNo,
    disable_attr: bool,
    is_search_row: bool,
    search_bar: &SearchBarInfo,
    highlights: QuickSelectMatchHighlights,
    colors: &config::Palette,
) {
    if disable_attr {
        line.cells_mut_for_attr_changes_only()
            .iter_mut()
            .for_each(|cell| cell.attrs_mut().clear());
        line.update_last_change_seqno(render_seqno);
        line.clear_appdata();
    }

    if is_search_row {
        // Replace with search UI
        let rev = CellAttributes::default().set_reverse(true).clone();
        line.fill_range(
            0..search_bar.cols,
            &Cell::new(' ', rev.clone()),
            render_seqno,
        );
        line.overlay_text_with_attribute(
            0,
            &format!(
                "Select: {}  (type highlighted prefix to {}, uppercase pastes, ESC to cancel)",
                search_bar.selection,
                if search_bar.label.is_empty() {
                    "copy"
                } else {
                    search_bar.label
                },
            ),
            rev,
            render_seqno,
        );
        line.clear_appdata();
        return;
    }

    let Some(matches) = highlights.matches else {
        return;
    };

    for m in matches {
        if let Some(lowered_prefix) = highlights.filter_by_selection {
            if !label_matches_selection(&m.label, lowered_prefix) {
                // Skip displaying this label, it doesn't match the current filter.
                continue;
            }
        }
        // highlight
        for cell_idx in m.range.clone() {
            if let Some(cell) = line.cells_mut_for_attr_changes_only().get_mut(cell_idx) {
                cell.attrs_mut()
                    .set_background(
                        colors
                            .quick_select_match_bg
                            .unwrap_or(AnsiColor::Black.into()),
                    )
                    .set_foreground(
                        colors
                            .quick_select_match_fg
                            .unwrap_or(AnsiColor::Green.into()),
                    )
                    .set_reverse(false)
                    .set_intensity(Intensity::Bold);
            }
        }
        for (idx, c) in m.label.chars().enumerate() {
            let mut attr = line
                .get_cell(idx)
                .map(|cell| cell.attrs().clone())
                .unwrap_or_default();
            attr.set_background(
                colors
                    .quick_select_label_bg
                    .unwrap_or(AnsiColor::Black.into()),
            )
            .set_foreground(
                colors
                    .quick_select_label_fg
                    .unwrap_or(AnsiColor::Olive.into()),
            )
            .set_reverse(false)
            .set_intensity(Intensity::Bold);
            line.set_cell(m.range.start + idx, Cell::new(c, attr), render_seqno);
        }
    }
    // cells_mut_for_attr_changes_only() above mutates cells directly
    // without bumping the line's seqno; do it explicitly so downstream
    // seqno-keyed caches see this pass as a new version of the line.
    line.update_last_change_seqno(render_seqno);
    line.clear_appdata();
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

/// Regression tests for the overlay-render-seqno fix.
///
/// Both CopyOverlay and QuickSelectOverlay mutate a *clone* of the
/// delegate pane's `Line` on every render pass (search bar text, match
/// highlighting, quickselect labels). Those clones inherit whatever seqno
/// the delegate's real line already has. Before this fix the overlays
/// tagged their own mutations with `SEQ_ZERO`; since
/// `Line::update_last_change_seqno` only ever increases a line's seqno
/// (`self.seqno = self.seqno.max(seqno)`), tagging with SEQ_ZERO on a line
/// whose seqno was already > 0 was a silent no-op. Downstream per-frame
/// caches keyed on (pane_id, stable_row, seqno) -- e.g. the GUI's
/// shape_hash_cache -- would then treat the overlay's mutated frame as
/// identical to a stale pre-overlay frame and keep serving the old shape,
/// so the search bar / highlights / labels could silently fail to appear.
///
/// These tests call the real `decorate_quickselect_line` function used by
/// `QuickSelectOverlay::with_lines_mut`/`get_lines` directly against a real
/// `Line`, without needing a live TermWindow/GUI (that function takes no
/// `&self`/`Window`, only plain data). This means reverting
/// `decorate_quickselect_line` (or its callers) back to tagging mutations
/// with `SEQ_ZERO` -- the bug fixed by facb0646e -- makes these tests fail,
/// unlike the old tests in this module which reimplemented the seqno-bump
/// counter locally and so passed regardless of what the production code did.
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

    /// Two successive real render passes (via `decorate_quickselect_line`)
    /// over the SAME underlying static delegate line (same seqno both
    /// times, as happens while the user types into the search bar but the
    /// underlying pane content does not change) must produce distinct
    /// `current_seqno()` values on the decorated clones, and those values
    /// must exceed the delegate's real seqno. If `decorate_quickselect_line`
    /// were reverted to stamp mutations with `SEQ_ZERO` instead of
    /// `render_seqno`, both assertions below would fail.
    #[test]
    fn quickselect_search_bar_render_bumps_seqno_past_delegate_and_across_passes() {
        let delegate_seqno = 5;
        let search_bar_1 = SearchBarInfo {
            cols: 10,
            selection: "a",
            label: "",
        };
        let search_bar_2 = SearchBarInfo {
            cols: 10,
            selection: "ab",
            label: "",
        };

        let mut line1 = make_line_with_seqno(delegate_seqno);
        let pass1_seqno = usize::MAX / 2 + 1;
        decorate_quickselect_line(
            &mut line1,
            pass1_seqno,
            false,
            true, // is_search_row
            &search_bar_1,
            QuickSelectMatchHighlights {
                matches: None,
                filter_by_selection: None,
            },
            &config::Palette::default(),
        );

        let mut line2 = make_line_with_seqno(delegate_seqno);
        let pass2_seqno = usize::MAX / 2 + 2;
        decorate_quickselect_line(
            &mut line2,
            pass2_seqno,
            false,
            true, // is_search_row
            &search_bar_2,
            QuickSelectMatchHighlights {
                matches: None,
                filter_by_selection: None,
            },
            &config::Palette::default(),
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
        assert!(text.starts_with("Select"));
    }

    /// The match/label-highlight branch of `decorate_quickselect_line` uses
    /// `cells_mut_for_attr_changes_only()`, which does NOT itself bump the
    /// line's seqno tracking -- the function must call
    /// `update_last_change_seqno` explicitly afterwards. This exercises
    /// that branch directly.
    #[test]
    fn quickselect_match_highlight_requires_explicit_seqno_bump() {
        let delegate_seqno = 5;
        let mut line = make_line_with_seqno(delegate_seqno);
        assert_eq!(line.current_seqno(), delegate_seqno);

        let matches = vec![MatchResult {
            range: 0..1,
            label: "a".to_string(),
        }];
        let search_bar = SearchBarInfo {
            cols: 10,
            selection: "",
            label: "",
        };
        let render_seqno = usize::MAX / 2 + 1;
        decorate_quickselect_line(
            &mut line,
            render_seqno,
            false,
            false, // not the search row: takes the match-highlight branch
            &search_bar,
            QuickSelectMatchHighlights {
                matches: Some(&matches),
                filter_by_selection: None,
            },
            &config::Palette::default(),
        );

        assert_eq!(
            line.current_seqno(),
            render_seqno,
            "decorate_quickselect_line's match-highlight branch mutates \
             cells directly via cells_mut_for_attr_changes_only(), which \
             does not itself bump the seqno -- an explicit \
             update_last_change_seqno(render_seqno) call is required, and \
             tagging with SEQ_ZERO here would be a silent no-op on this \
             already-nonzero-seqno line"
        );
        assert!(line.current_seqno() > delegate_seqno);
    }
}
