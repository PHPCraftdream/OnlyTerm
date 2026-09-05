use crate::domain::DomainId;
use crate::renderable::*;
use crate::ExitBehavior;
use async_trait::async_trait;
use config::keyassignment::{KeyAssignment, ScrollbackEraseMode};
use downcast_rs::{impl_downcast, Downcast};
use onlyterm_dynamic::Value;
use onlyterm_term::color::ColorPalette;
use onlyterm_term::{
    Clipboard, DownloadHandler, KeyCode, KeyModifiers, MouseEvent, Progress, SemanticZone,
    StableRowIndex, TerminalConfiguration, TerminalSize,
};
use parking_lot::MappedMutexGuard;
use rangeset::RangeSet;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;
use termwiz::hyperlink::Rule;
use termwiz::input::KeyboardEncoding;
use termwiz::surface::{Line, SequenceNo};
use url::Url;

static PANE_ID: ::std::sync::atomic::AtomicUsize = ::std::sync::atomic::AtomicUsize::new(0);
pub type PaneId = usize;

pub fn alloc_pane_id() -> PaneId {
    PANE_ID.fetch_add(1, ::std::sync::atomic::Ordering::Relaxed)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PerformAssignmentResult {
    /// Continue search for handler
    Unhandled,
    /// Found handler and acted upon the action
    Handled,
    /// Do not perform assignment, but instead treat the key event
    /// as though there was no assignment and run it as a key_down
    /// event.
    BlockAssignmentAndRouteToKeyDown,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SearchResult {
    pub start_y: StableRowIndex,
    /// The cell index into the line of the start of the match
    pub start_x: usize,
    pub end_y: StableRowIndex,
    /// The cell index into the line of the end of the match
    pub end_x: usize,
    /// An identifier that can be used to group results that have
    /// the same textual content
    pub match_id: usize,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub enum Pattern {
    CaseSensitiveString(String),
    CaseInSensitiveString(String),
    Regex(String),
}

impl Default for Pattern {
    fn default() -> Self {
        Self::CaseSensitiveString("".to_string())
    }
}

impl std::ops::Deref for Pattern {
    type Target = String;
    fn deref(&self) -> &String {
        match self {
            Pattern::CaseSensitiveString(s) => s,
            Pattern::CaseInSensitiveString(s) => s,
            Pattern::Regex(s) => s,
        }
    }
}

impl std::ops::DerefMut for Pattern {
    fn deref_mut(&mut self) -> &mut String {
        match self {
            Pattern::CaseSensitiveString(s) => s,
            Pattern::CaseInSensitiveString(s) => s,
            Pattern::Regex(s) => s,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub enum PatternType {
    CaseSensitiveString,
    CaseInSensitiveString,
    Regex,
}

impl From<&Pattern> for PatternType {
    fn from(value: &Pattern) -> Self {
        match value {
            Pattern::CaseSensitiveString(_) => PatternType::CaseSensitiveString,
            Pattern::CaseInSensitiveString(_) => PatternType::CaseInSensitiveString,
            Pattern::Regex(_) => PatternType::Regex,
        }
    }
}

/// Why a close request is being made
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CloseReason {
    /// The containing window is being closed
    Window,
    /// The containing tab is being close
    Tab,
    /// Just this tab is being closed
    Pane,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LogicalLine {
    pub physical_lines: Vec<Line>,
    pub logical: Line,
    pub first_row: StableRowIndex,
}

impl LogicalLine {
    pub fn contains_y(&self, y: StableRowIndex) -> bool {
        y >= self.first_row && y < self.first_row + self.physical_lines.len() as StableRowIndex
    }

    pub fn xy_to_logical_x(&self, x: usize, y: StableRowIndex) -> usize {
        let mut offset = 0;
        for (idx, line) in self.physical_lines.iter().enumerate() {
            let phys_y = self.first_row + idx as StableRowIndex;
            if y < phys_y {
                // Eg: trying to drag off the top of the viewport.
                // Their y coordinate precedes our first line, so
                // the only logical x we can return is 0
                return 0;
            }
            if phys_y == y {
                return offset + x;
            }
            offset += line.len();
        }
        // Allow selecting off the end of the line
        offset + x
    }

    pub fn logical_x_to_physical_coord(&self, x: usize) -> (StableRowIndex, usize) {
        let mut y = self.first_row;
        let mut idx = 0;
        for line in &self.physical_lines {
            let x_off = x - idx;
            let line_len = line.len();
            if x_off < line_len {
                return (y, x_off);
            }
            y += 1;
            idx += line_len;
        }
        (y - 1, x - idx + self.physical_lines.last().unwrap().len())
    }
}

/// A Pane represents a view on a terminal
#[async_trait(?Send)]
pub trait Pane: Downcast + Send + Sync {
    fn pane_id(&self) -> PaneId;

    /// Returns the 0-based cursor position relative to the top left of
    /// the visible screen
    fn get_cursor_position(&self) -> StableCursorPosition;

    fn get_current_seqno(&self) -> SequenceNo;

    /// Returns misc metadata that is pane-specific
    fn get_metadata(&self) -> Value {
        Value::Null
    }

    /// Given a range of lines, return the subset of those lines that
    /// have changed since the supplied sequence no.
    fn get_changed_since(
        &self,
        lines: Range<StableRowIndex>,
        seqno: SequenceNo,
    ) -> RangeSet<StableRowIndex>;

    /// Returns a set of lines from the scrollback or visible portion of
    /// the display.  The lines are indexed using StableRowIndex, which
    /// can be invalidated if the scrollback is busy, or when switching
    /// to the alternate screen.
    /// To deal with this, this function will adjust the input so that
    /// a range that has been scrolled off the top will return the top
    /// n rows of the scrollback (where n is the size of the input range),
    /// or the bottom n rows of the scrollback when switching to the alt
    /// screen and the index would go off the bottom.
    /// Because of this, we also return the adjusted StableRowIndex for
    /// the first row in the range.
    fn get_lines(&self, lines: Range<StableRowIndex>) -> (StableRowIndex, Vec<Line>);

    fn with_lines_mut(&self, lines: Range<StableRowIndex>, with_lines: &mut dyn WithPaneLines);

    fn for_each_logical_line_in_stable_range_mut(
        &self,
        lines: Range<StableRowIndex>,
        for_line: &mut dyn ForEachPaneLogicalLine,
    );

    fn get_logical_lines(&self, lines: Range<StableRowIndex>) -> Vec<LogicalLine>;

    fn apply_hyperlinks(&self, lines: Range<StableRowIndex>, rules: &[Rule]) {
        struct ApplyHyperLinks<'a> {
            rules: &'a [Rule],
        }
        impl<'a> ForEachPaneLogicalLine for ApplyHyperLinks<'a> {
            fn with_logical_line_mut(
                &mut self,
                _: Range<StableRowIndex>,
                lines: &mut [&mut Line],
            ) -> bool {
                Line::apply_hyperlink_rules(self.rules, lines);

                true
            }
        }

        self.for_each_logical_line_in_stable_range_mut(lines, &mut ApplyHyperLinks { rules });
    }

    /// Returns a render snapshot -- cursor position, renderable dimensions,
    /// and the cloned lines for the pane's visible range -- as one logical
    /// read (see `PaneRenderSnapshot`).
    ///
    /// `viewport` is the stable row index the GUI is currently scrolled to
    /// (the same value `get_lines` would be called with as range start),
    /// or `None` to use the pane's own `physical_top`.
    ///
    /// The default implementation composes this from the same separate
    /// getters the renderer historically used (`get_cursor_position`,
    /// `get_dimensions`, `apply_hyperlinks`, `get_lines`), so panes without
    /// a single-lock implementation keep their exact prior behavior --
    /// including the windows between acquisitions where a pty parser
    /// thread can apply output mid-read (investigation
    /// `2026-08-25-render-and-resource-bug-hunt` section 1.3, bug B: a
    /// paint could combine a cursor position from moment t0 with line
    /// contents from t2, drawing the cursor block on the old prompt row
    /// while the input box has already visually moved to a new row).
    ///
    /// Implementations backed by a single terminal mutex (`LocalPane`)
    /// override this to capture everything under one short lock
    /// acquisition, which removes those windows
    /// (ghost-cursor-fix-plan Phase C). The lock is still short: one
    /// viewport's worth of line clones, not held across shaping/GPU work.
    fn get_render_snapshot(
        &self,
        viewport: Option<StableRowIndex>,
        hyperlink_rules: &[Rule],
    ) -> PaneRenderSnapshot {
        let cursor = self.get_cursor_position();
        let dims = self.get_dimensions();
        let top = viewport.unwrap_or(dims.physical_top);
        let lines = top..top + dims.viewport_rows as StableRowIndex;
        self.apply_hyperlinks(lines.clone(), hyperlink_rules);
        let (stable_top, lines) = self.get_lines(lines);
        PaneRenderSnapshot {
            cursor,
            dims,
            stable_top,
            lines,
        }
    }

    /// Returns render related dimensions
    fn get_dimensions(&self) -> RenderableDimensions;

    fn get_title(&self) -> String;
    fn get_progress(&self) -> Progress {
        Progress::None
    }
    fn send_paste(&self, text: &str) -> anyhow::Result<()>;
    fn reader(&self) -> anyhow::Result<Option<Box<dyn std::io::Read + Send>>>;
    fn writer(&self) -> MappedMutexGuard<'_, dyn std::io::Write>;
    fn resize(&self, size: TerminalSize) -> anyhow::Result<()>;
    /// Called as a hint that the pane is being resized as part of
    /// a zoom-to-fill-all-the-tab-space operation.
    fn set_zoomed(&self, _zoomed: bool) {}
    fn key_down(&self, key: KeyCode, mods: KeyModifiers) -> anyhow::Result<()>;
    fn key_up(&self, key: KeyCode, mods: KeyModifiers) -> anyhow::Result<()>;
    fn perform_assignment(&self, _assignment: &KeyAssignment) -> PerformAssignmentResult {
        PerformAssignmentResult::Unhandled
    }
    fn mouse_event(&self, event: MouseEvent) -> anyhow::Result<()>;
    fn perform_actions(&self, _actions: Vec<termwiz::escape::Action>) {}
    fn is_dead(&self) -> bool;
    fn kill(&self) {}
    fn palette(&self) -> ColorPalette;
    fn domain_id(&self) -> DomainId;

    /// The keyboard encoding protocol (win32-input-mode, kitty, CSI-u) that
    /// the application running in this pane has negotiated, if any. The GUI
    /// uses this to decide how to encode key events, so an implementation
    /// that reports the default `Xterm` when a protocol *was* in fact
    /// negotiated will silently downgrade every keystroke to legacy bytes.
    ///
    /// `LocalPane` answers from its own `TerminalState`; `ClientPane`
    /// answers from the value mirrored out of
    /// `GetPaneRenderChangesResponse::keyboard_encoding`. The default here is
    /// only correct for panes that have no terminal of their own at all
    /// (overlays and the like).
    fn get_keyboard_encoding(&self) -> KeyboardEncoding {
        KeyboardEncoding::Xterm
    }

    fn copy_user_vars(&self) -> HashMap<String, String> {
        HashMap::new()
    }

    fn erase_scrollback(&self, _erase_mode: ScrollbackEraseMode) {}

    /// Called to advise on whether this pane has focus
    fn focus_changed(&self, _focused: bool) {}

    /// Called to advise remote mux that this is the active pane
    /// for the current identity
    fn advise_focus(&self) {}

    fn has_unseen_output(&self) -> bool {
        false
    }

    /// Task #248: true if a recent GUI-thread-reachable accessor
    /// (`get_title()`, `get_progress()`, `copy_user_vars()`,
    /// `get_current_working_dir()`) gave up waiting on this pane's
    /// underlying terminal lock and served stale data instead (see
    /// `try_lock_terminal_for` in `crates/mux/src/localpane.rs`, task
    /// #246). Lets callers (tab-title formatters, future auto-recovery
    /// logic) observe "this pane may be wedged" instead of the timeout
    /// only showing up as a silent metrics counter. Defaults to `false`
    /// so pane implementations that have no such lock to wedge on (e.g.
    /// `Pane` impls that don't wrap a `Mutex<Terminal>`) never need to
    /// think about this.
    ///
    /// Task #269: this reflects ONLY the lock-timeout signal above, OR'd
    /// together (by implementations such as `LocalPane`) with the
    /// separate, independently-written `render_budget_exceeded` signal
    /// (see `set_render_budget_exceeded()`). The two are tracked in
    /// separate cells precisely so that the per-frame render-budget
    /// producer -- which writes `false` far more often than `true`, once
    /// per painted pane per frame -- can never clobber a genuine,
    /// still-active lock-timeout `true` written by the unrelated
    /// producer in `try_lock_terminal_for`.
    fn is_unresponsive(&self) -> bool {
        false
    }

    /// Task #251/#269: lets a caller outside of this pane's own
    /// lock-timeout machinery (specifically, the GUI's per-frame
    /// content-build budget in `paint_tab_content`/`paint_pane`) report
    /// "this pane is currently too slow to render fully". This is
    /// deliberately a SEPARATE signal from the one `is_unresponsive()`'s
    /// lock-timeout half is built on: the render-budget path writes here
    /// unconditionally every frame (both `true` and `false`), which would
    /// otherwise race with and clobber a wedged-lock `true` written
    /// concurrently by `try_lock_terminal_for` for the same pane (task
    /// #269). `is_unresponsive()` reports the OR of both signals, so
    /// implementations should combine this cell with their own
    /// lock-timeout cell rather than reusing a single shared flag.
    /// Defaults to a no-op so `Pane` impls that don't back
    /// `is_unresponsive()` with real storage aren't forced to implement
    /// storage for a signal they never read back.
    fn set_render_budget_exceeded(&self, _exceeded: bool) {}

    /// Certain panes are OK to be closed with impunity (no prompts)
    fn can_close_without_prompting(&self, _reason: CloseReason) -> bool {
        false
    }

    /// Performs a search bounded to the specified range.
    /// If the result is empty then there are no matches.
    /// Otherwise, if limit.is_none(), the result shall contain all possible
    /// matches.
    /// If limit.is_some(), then the maximum number of results that will be
    /// returned is limited to the specified number, and the
    /// SearchResult::start_y of the last item
    /// in the result can be used as the start of the next region to search.
    /// You can tell that you have reached the end of the results if the number
    /// of results is smaller than the limit you set.
    async fn search(
        &self,
        _pattern: Pattern,
        _range: Range<StableRowIndex>,
        _limit: Option<u32>,
    ) -> anyhow::Result<Vec<SearchResult>> {
        Ok(vec![])
    }

    /// Retrieve the set of semantic zones
    fn get_semantic_zones(&self) -> anyhow::Result<Vec<SemanticZone>> {
        Ok(vec![])
    }

    /// Returns true if the terminal has grabbed the mouse and wants to
    /// give the embedded application a chance to process events.
    /// In practice this controls whether the gui will perform local
    /// handling of clicks.
    fn is_mouse_grabbed(&self) -> bool;
    fn is_alt_screen_active(&self) -> bool;

    fn set_clipboard(&self, _clipboard: &Arc<dyn Clipboard>) {}
    fn set_download_handler(&self, _handler: &Arc<dyn DownloadHandler>) {}
    fn set_config(&self, _config: Arc<dyn TerminalConfiguration>) {}
    fn get_config(&self) -> Option<Arc<dyn TerminalConfiguration>> {
        None
    }

    fn get_current_working_dir(&self, policy: CachePolicy) -> Option<Url>;
    fn get_foreground_process_name(&self, _policy: CachePolicy) -> Option<String> {
        None
    }
    fn get_foreground_process_info(
        &self,
        _policy: CachePolicy,
    ) -> Option<procinfo::LocalProcessInfo> {
        None
    }

    /// Executable base names of EVERY process in this pane's tree, not just
    /// the one `get_foreground_process_name` picks.
    ///
    /// The foreground heuristic returns a single process -- on Windows, the
    /// most recently started one sharing the console -- which is the right
    /// answer for "what is the user interacting with" but the wrong one for
    /// "is program X running in this pane". A program launched through a
    /// wrapper hides behind whichever link of the chain happens to be
    /// youngest: Codex CLI is `codex.cmd` -> node -> `codex.exe`, and the
    /// foreground call was measured to return `node_repl.exe`, so matching
    /// on it alone silently never fires.
    fn get_process_tree_exe_names(
        &self,
        _policy: CachePolicy,
    ) -> Option<std::collections::HashSet<String>> {
        log::info!(
            "diag: key-compat pane={} type={} process tree unsupported",
            self.pane_id(),
            std::any::type_name::<Self>(),
        );
        None
    }

    fn tty_name(&self) -> Option<String> {
        None
    }

    fn exit_behavior(&self) -> Option<ExitBehavior> {
        None
    }
}
impl_downcast!(Pane);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CachePolicy {
    FetchImmediate,
    AllowStale,
}

/// This trait is used to implement/provide a callback that is used together
/// with the Pane::with_lines_mut method.
/// Ideally we'd simply pass an FnMut with the same signature as the trait
/// method defined here, but doing so results in Pane not being object-safe.
pub trait WithPaneLines {
    /// The `first_row` parameter is set to the StableRowIndex of the resolved
    /// first row from the Pane::with_lines_mut method. It will usually be
    /// the start of the lines range, but in case that row is no longer in
    /// a valid range (scrolled out of scrollback), it may be revised.
    ///
    /// `lines` is a mutable slice of the mutable lines in the requested
    /// stable range.
    fn with_lines_mut(&mut self, first_row: StableRowIndex, lines: &mut [&mut Line]);
}

/// This trait is used to implement/provide a callback that is used together
/// with the Pane::for_each_logical_line_in_stable_range_mut method.
/// Ideally we'd simply pass an FnMut with the same signature as the trait
/// method defined here, but doing so results in Pane not being object-safe.
pub trait ForEachPaneLogicalLine {
    /// The `stable_range` parameter is set to the range of physical lines
    /// that comprise the current logical line.
    ///
    /// `lines` is a mutable slice of the mutable physical lines that comprise
    /// the current logical line.
    ///
    /// Return `true` to continue with the next logical line in the requested
    /// range, or `false` to cease iteration.
    fn with_logical_line_mut(
        &mut self,
        stable_range: Range<StableRowIndex>,
        lines: &mut [&mut Line],
    ) -> bool;
}

/// A helper that allows you to implement Pane::with_lines_mut in terms
/// of your existing Pane::get_lines method.
///
/// The mutability is really a lie: while `with_lines` is passed something
/// that is mutable, it is operating on a copy the lines that won't persist
/// beyond the call to Pane::with_lines_mut.
pub fn impl_with_lines_via_get_lines<P: Pane + ?Sized>(
    pane: &P,
    lines: Range<StableRowIndex>,
    with_lines: &mut dyn WithPaneLines,
) {
    let (first, mut lines) = pane.get_lines(lines);
    let mut line_refs = vec![];
    for line in lines.iter_mut() {
        line_refs.push(line);
    }
    with_lines.with_lines_mut(first, &mut line_refs);
}

/// A helper that allows you to implement Pane::for_each_logical_line_in_stable_range_mut
/// in terms of your existing Pane::get_logical_lines method.
///
/// The mutability is really a lie: while `with_lines` is passed something
/// that is mutable, it is operating on a copy the lines that won't persist
/// beyond the call to Pane::with_lines_mut.
pub fn impl_for_each_logical_line_via_get_logical_lines<P: Pane + ?Sized>(
    pane: &P,
    lines: Range<StableRowIndex>,
    for_line: &mut dyn ForEachPaneLogicalLine,
) {
    let mut logical = pane.get_logical_lines(lines);

    for line in &mut logical {
        let num_lines = line.physical_lines.len() as StableRowIndex;
        let mut line_refs = vec![];
        for phys in line.physical_lines.iter_mut() {
            line_refs.push(phys);
        }
        let should_continue = for_line
            .with_logical_line_mut(line.first_row..line.first_row + num_lines, &mut line_refs);
        if !should_continue {
            break;
        }
    }
}

/// A helper that allows you to implement Pane::get_logical_lines in terms of
/// your Pane::get_lines method.
pub fn impl_get_logical_lines_via_get_lines<P: Pane + ?Sized>(
    pane: &P,
    lines: Range<StableRowIndex>,
) -> Vec<LogicalLine> {
    let (mut first, mut phys) = pane.get_lines(lines);

    // Avoid pathological cases where we have eg: a really long logical line
    // (such as 1.5MB of json) that we previously wrapped.  We don't want to
    // un-wrap, scan, and re-wrap that thing.
    // This is an imperfect length constraint to partially manage the cost.
    const MAX_LOGICAL_LINE_LEN: usize = 1024;
    let mut back_len = 0;

    // Look backwards to find the start of the first logical line
    while first > 0 {
        let (prior, back) = pane.get_lines(first - 1..first);
        if prior == first {
            break;
        }
        if !back[0].last_cell_was_wrapped() {
            break;
        }
        if back[0].len() + back_len > MAX_LOGICAL_LINE_LEN {
            break;
        }
        back_len += back[0].len();
        first = prior;
        for (idx, line) in back.into_iter().enumerate() {
            phys.insert(idx, line);
        }
    }

    // Look forwards to find the end of the last logical line
    while let Some(last) = phys.last() {
        if !last.last_cell_was_wrapped() {
            break;
        }
        if last.len() > MAX_LOGICAL_LINE_LEN {
            break;
        }

        let next_row = first + phys.len() as StableRowIndex;
        let (last_row, mut ahead) = pane.get_lines(next_row..next_row + 1);
        if last_row != next_row {
            break;
        }
        phys.append(&mut ahead);
    }

    // Now process this stuff into logical lines
    let mut lines = vec![];
    for (idx, line) in phys.into_iter().enumerate() {
        // The `if` here has an `else` branch, so collapsing it into a match
        // guard would duplicate the `Some(prior)` pattern for the else arm;
        // keeping the explicit if/else is clearer and behavior-identical.
        #[allow(clippy::collapsible_match)]
        match lines.last_mut() {
            None => {
                let logical = line.clone();
                lines.push(LogicalLine {
                    physical_lines: vec![line],
                    logical,
                    first_row: first + idx as StableRowIndex,
                });
            }
            Some(prior) => {
                if prior.logical.last_cell_was_wrapped()
                    && prior.logical.len() <= MAX_LOGICAL_LINE_LEN
                {
                    let seqno = prior.logical.current_seqno().max(line.current_seqno());
                    prior.logical.set_last_cell_was_wrapped(false, seqno);
                    prior.logical.append_line(line.clone(), seqno);
                    prior.physical_lines.push(line);
                } else {
                    let logical = line.clone();
                    lines.push(LogicalLine {
                        physical_lines: vec![line],
                        logical,
                        first_row: first + idx as StableRowIndex,
                    });
                }
            }
        }
    }
    lines
}

/// A helper that allows you to implement Pane::get_lines in terms
/// of your Pane::with_lines_mut method.
pub fn impl_get_lines_via_with_lines<P: Pane + ?Sized>(
    pane: &P,
    lines: Range<StableRowIndex>,
) -> (StableRowIndex, Vec<Line>) {
    struct LineCollector {
        first: StableRowIndex,
        lines: Vec<Line>,
    }

    let mut collector = LineCollector {
        first: 0,
        lines: vec![],
    };

    impl WithPaneLines for LineCollector {
        fn with_lines_mut(&mut self, first_row: StableRowIndex, lines: &mut [&mut Line]) {
            self.first = first_row;
            for line in lines.iter_mut() {
                self.lines.push(line.clone());
            }
        }
    }

    pane.with_lines_mut(lines, &mut collector);
    (collector.first, collector.lines)
}

#[cfg(test)]
mod test;
