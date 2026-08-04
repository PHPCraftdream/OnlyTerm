use crate::selection::{SelectionCoordinate, SelectionRange};
use crate::termwindow::TermWindow;
use config::keyassignment::SelectionMode;
use mux::pane::{
    Pane, Pattern, PatternType, SearchResult,
};
use mux::renderable::*;
use mux::tab::TabId;
use parking_lot::Mutex;
use rangeset::RangeSet;
use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;
use termwiz::lineedit::LineEditBuffer;
use termwiz::surface::{CursorVisibility, SequenceNo, SEQ_ZERO};
use unicode_segmentation::*;
use wezterm_term::StableRowIndex;

lazy_static::lazy_static! {
    static ref SAVED_PATTERN: Mutex<HashMap<TabId, Pattern>> = Mutex::new(HashMap::new());
}

const SEARCH_CHUNK_SIZE: StableRowIndex = 1000;

pub struct CopyOverlay {
    delegate: Arc<dyn Pane>,
    render: Arc<Mutex<CopyRenderable>>,
    writer: Mutex<SearchOverlayPatternWriter>,
}

#[derive(Copy, Clone, Debug)]
struct PendingJump {
    forward: bool,
    prev_char: bool,
}

#[derive(Copy, Clone, Debug)]
struct Jump {
    forward: bool,
    prev_char: bool,
    target: char,
}

struct CopyRenderable {
    cursor: StableCursorPosition,
    delegate: Arc<dyn Pane>,
    start: Option<SelectionCoordinate>,
    selection_mode: SelectionMode,
    /// The selection range computed by the most recent CopyMode action.
    /// Stored synchronously so that selection readers (CopyTo, etc.) within
    /// the same synchronous dispatch pass see the up-to-date range without
    /// waiting for the deferred TermWindow mirror update via window.notify.
    /// See issue #3302.
    selection_range: Option<SelectionRange>,
    selection_rectangular: bool,
    viewport: Option<StableRowIndex>,
    /// We use this to cancel ourselves later
    window: ::window::Window,

    /// The text that the user entered
    pattern_type: PatternType,
    search_line: LineEditBuffer,
    /// The most recently queried set of matches
    results: Vec<SearchResult>,
    by_line: HashMap<StableRowIndex, Vec<MatchResult>>,
    last_result_seqno: SequenceNo,
    last_bar_pos: Option<StableRowIndex>,
    dirty_results: RangeSet<StableRowIndex>,
    width: usize,
    height: usize,
    editing_search: bool,
    result_pos: Option<usize>,
    tab_id: TabId,
    /// Used to debounce queries while the user is typing
    typing_cookie: usize,
    searching: Option<Searching>,
    pending_jump: Option<PendingJump>,
    last_jump: Option<Jump>,
}

struct Searching {
    remain: StableRowIndex,
}

#[derive(Debug)]
struct MatchResult {
    range: Range<usize>,
    result_index: usize,
}

struct Dimensions {
    vertical_gap: isize,
    dims: RenderableDimensions,
    top: StableRowIndex,
}

#[derive(Debug)]
pub struct CopyModeParams {
    pub pattern: Pattern,
    pub editing_search: bool,
}

impl CopyOverlay {
    pub fn with_pane(
        term_window: &TermWindow,
        pane: &Arc<dyn Pane>,
        params: CopyModeParams,
    ) -> anyhow::Result<Arc<dyn Pane>> {
        let mut cursor = pane.get_cursor_position();
        cursor.shape = termwiz::surface::CursorShape::SteadyBlock;
        cursor.visibility = CursorVisibility::Visible;

        let (_domain, _window, tab_id) = mux::Mux::get()
            .resolve_pane_id(pane.pane_id())
            .ok_or_else(|| anyhow::anyhow!("no tab contains the current pane"))?;

        let window = term_window
            .window
            .clone()
            .ok_or_else(|| anyhow::anyhow!("failed to clone window handle"))?;
        let dims = pane.get_dimensions();
        let pattern = if params.pattern.is_empty() {
            SAVED_PATTERN
                .lock()
                .get(&tab_id)
                .cloned()
                .unwrap_or(params.pattern)
        } else {
            params.pattern
        };
        let search_line = LineEditBuffer::new(&pattern, pattern.len());

        let mut render = CopyRenderable {
            cursor,
            window,
            delegate: Arc::clone(pane),
            start: None,
            viewport: term_window.get_viewport(pane.pane_id()),
            results: vec![],
            by_line: HashMap::new(),
            dirty_results: RangeSet::default(),
            width: dims.cols,
            height: dims.viewport_rows,
            last_result_seqno: SEQ_ZERO,
            last_bar_pos: None,
            tab_id,
            pattern_type: PatternType::from(&pattern),
            search_line,
            editing_search: params.editing_search,
            result_pos: None,
            selection_mode: SelectionMode::Cell,
            selection_range: None,
            selection_rectangular: false,
            typing_cookie: 0,
            searching: None,
            pending_jump: None,
            last_jump: None,
        };

        let search_row = render.compute_search_row();
        render.dirty_results.add(search_row);
        render.update_search();

        let shared_render = Arc::new(Mutex::new(render));
        let writer = SearchOverlayPatternWriter {
            render: Arc::clone(&shared_render),
        };

        Ok(Arc::new(CopyOverlay {
            delegate: Arc::clone(pane),
            render: shared_render,
            writer: Mutex::new(writer),
        }))
    }

    pub fn get_params(&self) -> CopyModeParams {
        let render = self.render.lock();
        CopyModeParams {
            pattern: render.get_pattern(),
            editing_search: render.editing_search,
        }
    }

    pub fn apply_params(&self, params: CopyModeParams) {
        let mut render = self.render.lock();
        render.editing_search = params.editing_search;
        if render.get_pattern() != params.pattern {
            render.pattern_type = PatternType::from(&params.pattern);
            render
                .search_line
                .set_line_and_cursor(&params.pattern, params.pattern.len());
            render.schedule_update_search();
        }
        let search_row = render.compute_search_row();
        render.dirty_results.add(search_row);
    }

    pub fn viewport_changed(&self, viewport: Option<StableRowIndex>) {
        let mut render = self.render.lock();
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

    /// Returns the authoritative selection range and rectangular flag set by
    /// the most recent CopyMode action. Unlike the TermWindow's selection
    /// mirror (updated asynchronously via window.notify), this reflects
    /// selection changes immediately, so it is correct even within the same
    /// synchronous dispatch pass — e.g. when CopyTo runs after CopyMode
    /// actions inside action.Multiple. See issue #3302.
    pub fn current_selection(&self) -> Option<(SelectionRange, bool)> {
        let render = self.render.lock();
        Some((
            *render.selection_range.as_ref()?,
            render.selection_rectangular,
        ))
    }
}

pub struct SearchOverlayPatternWriter {
    render: Arc<Mutex<CopyRenderable>>,
}

impl std::io::Write for SearchOverlayPatternWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut render = self.render.lock();
        let s = std::str::from_utf8(buf)
            .map_err(|err| std::io::Error::other(format!("invalid UTF-8: {err:#}")))?;
        render.search_line.insert_text(s);
        render.schedule_update_search();
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

mod key_tables;
mod pane;
mod render;

pub use key_tables::{copy_key_table, search_key_table};
