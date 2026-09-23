#![allow(clippy::range_plus_one)]
use super::*;
use crate::config::BidiMode;
use log::debug;
use onlyterm_surface::SequenceNo;
use std::collections::VecDeque;
use std::sync::Arc;
use termwiz::input::KeyboardEncoding;

mod iterate;
mod ranges;
mod resize;
mod resize_history;
mod scroll;

/// Holds the model of a screen.  This can either be the primary screen
/// which includes lines of scrollback text, or the alternate screen
/// which holds no scrollback.  The intent is to have one instance of
/// Screen for each of these things.
#[derive(Debug, Clone)]
pub struct Screen {
    /// Holds the line data that comprises the screen contents.
    /// This is allocated with capacity for the entire scrollback.
    /// The last N lines are the visible lines, with those prior being
    /// the lines that have scrolled off the top of the screen.
    /// Index 0 is the topmost line of the screen/scrollback (depending
    /// on the current window size) and will be the first line to be
    /// popped off the front of the screen when a new line is added that
    /// would otherwise have exceeded the line capacity
    lines: VecDeque<Line>,

    /// Whenever we scroll a line off the top of the scrollback, we
    /// increment this.  We use this offset to translate between
    /// PhysRowIndex and StableRowIndex.
    stable_row_index_offset: usize,

    /// config so we can access Maximum number of lines of scrollback
    config: Arc<dyn TerminalConfiguration>,

    /// Whether scrollback is allowed; this is another way of saying
    /// that we're the primary rather than the alternate screen.
    allow_scrollback: bool,

    pub(crate) keyboard_stack: Vec<KeyboardEncoding>,

    /// Physical, visible height of the screen (not including scrollback)
    pub physical_rows: usize,
    /// Physical, visible width of the screen
    pub physical_cols: usize,
    pub dpi: u32,
    /// ConPTY's output extent excludes unused space added by window growth.
    output_rows: usize,

    pub(crate) saved_cursor: Option<SavedCursor>,
}

fn scrollback_size(config: &Arc<dyn TerminalConfiguration>, allow_scrollback: bool) -> usize {
    if allow_scrollback {
        config.scrollback_size()
    } else {
        0
    }
}

impl Screen {
    /// Create a new Screen with the specified dimensions.
    /// The Cells in the viewable portion of the screen are set to the
    /// default cell attributes.
    pub fn new(
        size: TerminalSize,
        config: &Arc<dyn TerminalConfiguration>,
        allow_scrollback: bool,
        seqno: SequenceNo,
        bidi_mode: BidiMode,
    ) -> Screen {
        let physical_rows = size.rows.max(1);
        let physical_cols = size.cols.max(1);

        let mut lines =
            VecDeque::with_capacity(physical_rows + scrollback_size(config, allow_scrollback));
        for _ in 0..physical_rows {
            let mut line = Line::new(seqno);
            bidi_mode.apply_to_line(&mut line, seqno);
            lines.push_back(line);
        }

        Screen {
            lines,
            config: Arc::clone(config),
            allow_scrollback,
            physical_rows,
            physical_cols,
            stable_row_index_offset: 0,
            dpi: size.dpi,
            output_rows: 0,
            keyboard_stack: vec![],
            saved_cursor: None,
        }
    }

    pub fn full_reset(&mut self) {
        self.keyboard_stack.clear();
        self.output_rows = 0;
    }

    fn scrollback_size(&self) -> usize {
        scrollback_size(&self.config, self.allow_scrollback)
    }

    /// Get mutable reference to a line, relative to start of scrollback.
    #[inline]
    pub fn line_mut(&mut self, idx: PhysRowIndex) -> &mut Line {
        &mut self.lines[idx]
    }

    /// Returns the number of occupied rows of scrollback
    pub fn scrollback_rows(&self) -> usize {
        self.lines.len()
    }

    /// Sets a line dirty.  The line is relative to the visible origin.
    #[inline]
    pub fn dirty_line(&mut self, idx: VisibleRowIndex, seqno: SequenceNo) {
        let line_idx = self.phys_row(idx);
        if line_idx < self.lines.len() {
            self.lines[line_idx].update_last_change_seqno(seqno);
        }
    }

    /// Returns a copy of the visible lines in the screen (no scrollback)
    #[cfg(test)]
    pub fn visible_lines(&self) -> Vec<Line> {
        let line_idx = self.lines.len() - self.physical_rows;
        let mut lines = Vec::new();
        for line in self.lines.iter().skip(line_idx) {
            if lines.len() >= self.physical_rows {
                break;
            }
            lines.push(line.clone());
        }
        lines
    }

    /// Returns a copy of the lines in the screen (including scrollback)
    #[cfg(test)]
    pub fn all_lines(&self) -> Vec<Line> {
        self.lines.iter().cloned().collect()
    }

    pub fn insert_cell(
        &mut self,
        x: usize,
        y: VisibleRowIndex,
        right_margin: usize,
        seqno: SequenceNo,
    ) {
        let phys_cols = self.physical_cols;

        let line_idx = self.phys_row(y);
        let line = self.line_mut(line_idx);
        line.update_last_change_seqno(seqno);
        line.insert_cell(x, Cell::default(), right_margin, seqno);
        if line.len() > phys_cols {
            // Don't allow the line width to grow beyond
            // the physical width
            line.resize(phys_cols, seqno);
        }
    }

    pub fn erase_cell(
        &mut self,
        x: usize,
        y: VisibleRowIndex,
        right_margin: usize,
        seqno: SequenceNo,
        blank_attr: CellAttributes,
    ) {
        let line_idx = self.phys_row(y);
        let line = self.line_mut(line_idx);
        line.erase_cell_with_margin(x, right_margin, seqno, blank_attr);
    }

    /// Set a cell.  the x and y coordinates are relative to the visible screeen
    /// origin.  0,0 is the top left.
    pub fn set_cell(&mut self, x: usize, y: VisibleRowIndex, cell: &Cell, seqno: SequenceNo) {
        self.note_output_row(y);
        let line_idx = self.phys_row(y);
        //debug!("set_cell x={} y={} phys={} {:?}", x, y, line_idx, cell);

        let line = self.line_mut(line_idx);
        line.set_cell(x, cell.clone(), seqno);
    }

    pub fn set_cell_grapheme(
        &mut self,
        x: usize,
        y: VisibleRowIndex,
        text: &str,
        width: usize,
        attr: CellAttributes,
        seqno: SequenceNo,
    ) {
        self.note_output_row(y);
        let line_idx = self.phys_row(y);
        let line = self.line_mut(line_idx);
        line.set_cell_grapheme(x, text, width, attr, seqno);
    }

    pub fn cell_mut(&mut self, x: usize, y: VisibleRowIndex) -> Option<&mut Cell> {
        self.note_output_row(y);
        let line_idx = self.phys_row(y);
        let line = self.lines.get_mut(line_idx)?;
        line.cells_mut().get_mut(x)
    }

    pub fn get_cell(&mut self, x: usize, y: VisibleRowIndex) -> Option<&Cell> {
        let line_idx = self.phys_row(y);
        let line = self.lines.get_mut(line_idx)?;
        line.cells_mut().get(x)
    }

    fn note_output_row(&mut self, y: VisibleRowIndex) {
        self.output_rows = self
            .output_rows
            .max((y.max(0) as usize + 1).min(self.physical_rows));
    }

    pub fn clear_line(
        &mut self,
        y: VisibleRowIndex,
        cols: Range<usize>,
        attr: &CellAttributes,
        seqno: SequenceNo,
        bidi_mode: BidiMode,
    ) {
        let line_idx = self.phys_row(y);
        let line = self.line_mut(line_idx);
        if cols.start == 0 {
            bidi_mode.apply_to_line(line, seqno);
        }
        line.fill_range(cols, &Cell::blank_with_attrs(attr.clone()), seqno);
    }

    /// Ensure that row is within the range of the physical portion of
    /// the screen; 0 .. physical_rows by clamping it to the nearest
    /// boundary.
    #[inline]
    fn clamp_visible_row(&self, row: VisibleRowIndex) -> VisibleRowIndex {
        (row.max(0) as usize).min(self.physical_rows) as VisibleRowIndex
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::color::ColorPalette;
    use onlyterm_bidi::ParagraphDirectionHint;
    use onlyterm_surface::SEQ_ZERO;

    #[derive(Debug)]
    struct TestConfig {
        scrollback: usize,
    }

    impl TerminalConfiguration for TestConfig {
        fn scrollback_size(&self) -> usize {
            self.scrollback
        }

        fn color_palette(&self) -> ColorPalette {
            ColorPalette::default()
        }
    }

    fn make_screen(physical_rows: usize, scrollback: usize) -> Screen {
        let config: Arc<dyn TerminalConfiguration> = Arc::new(TestConfig { scrollback });
        Screen::new(
            TerminalSize {
                rows: physical_rows,
                cols: 1,
                pixel_width: physical_rows * 8,
                pixel_height: physical_rows * 16,
                dpi: 0,
            },
            &config,
            true,
            SEQ_ZERO,
            BidiMode {
                enabled: false,
                hint: ParagraphDirectionHint::LeftToRight,
            },
        )
    }

    fn line_text(line: &Line) -> String {
        line.as_str().to_string()
    }

    /// Regression test for the upstream bug fixed by
    /// <https://github.com/wezterm/wezterm/pull/7177>: `with_phys_lines`
    /// failed to translate the absolute `second_range` (computed against
    /// the whole `VecDeque`) into an index relative to the `second` slice
    /// returned by `VecDeque::as_slices()`, whereas its sibling
    /// `with_phys_lines_mut` did this translation correctly. Whenever the
    /// backing `VecDeque` had physically wrapped around its ring buffer
    /// (i.e. both halves returned by `as_slices()` were non-empty), the
    /// unpatched `with_phys_lines` would silently read from the wrong
    /// offset into `second`, returning the wrong lines (or panicking with
    /// an out-of-bounds slice index) instead of the requested physical
    /// rows.
    #[test]
    fn with_phys_lines_matches_mut_after_vecdeque_wraps() {
        let mut screen = make_screen(4, 4);

        // Force the backing VecDeque's ring buffer to physically wrap by
        // repeatedly popping a line off the front and pushing a new,
        // uniquely labelled line onto the back. This keeps the number of
        // rows constant while advancing the internal head index through
        // every possible offset, so `as_slices()` is guaranteed to
        // eventually report two non-empty slices with the front slice
        // shorter than the back slice (the scenario that silently
        // corrupts data rather than merely panicking).
        let mut wrapped_with_shorter_front = false;
        for i in 0..512 {
            screen.lines.pop_front();
            screen.lines.push_back(Line::from(format!("L{i}").as_str()));

            let (first, second) = screen.lines.as_slices();
            if !first.is_empty() && !second.is_empty() && first.len() < second.len() {
                wrapped_with_shorter_front = true;
                break;
            }
        }
        assert!(
            wrapped_with_shorter_front,
            "test setup failed to force the VecDeque to wrap with a front slice \
             shorter than the back slice; as_slices() = {:?}",
            {
                let (first, second) = screen.lines.as_slices();
                (first.len(), second.len())
            }
        );

        let (first_len, second_len) = {
            let (first, second) = screen.lines.as_slices();
            (first.len(), second.len())
        };
        let total = screen.lines.len();
        assert_eq!(total, first_len + second_len);

        // Ground truth: the logical (phys-index-ordered) content of the
        // screen, independent of how the ring buffer happens to be laid
        // out internally.
        let ground_truth: Vec<String> = screen.lines.iter().map(line_text).collect();

        // Query a range that dips into the "second" slice but stops
        // short of the very end of the deque, so that a buggy
        // implementation would read valid-but-wrong memory (a silent
        // content mismatch) rather than merely panicking on an
        // out-of-bounds slice.
        let phys_range = 0..(total - 1);
        assert!(
            phys_range.end > first_len,
            "range must cross into the second slice"
        );

        let mut from_mut: Vec<String> = vec![];
        screen.with_phys_lines_mut(phys_range.clone(), |lines| {
            from_mut = lines.iter().map(|l| line_text(l)).collect();
        });

        let mut from_immutable: Vec<String> = vec![];
        screen.with_phys_lines(phys_range.clone(), |lines| {
            from_immutable = lines.iter().map(|l| line_text(l)).collect();
        });

        let expected = ground_truth[phys_range.clone()].to_vec();

        assert_eq!(
            from_mut, expected,
            "with_phys_lines_mut (known-correct reference) did not match ground truth"
        );
        assert_eq!(
            from_immutable, expected,
            "with_phys_lines returned the wrong lines after the VecDeque wrapped \
             (first_len={first_len}, second_len={second_len}); this is the bug fixed \
             by upstream PR #7177"
        );
        assert_eq!(
            from_immutable, from_mut,
            "with_phys_lines and with_phys_lines_mut disagree on the same phys_range"
        );
    }
}
