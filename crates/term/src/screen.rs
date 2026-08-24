#![allow(clippy::range_plus_one)]
use super::*;
use crate::config::BidiMode;
use log::debug;
use onlyterm_surface::SequenceNo;
use std::collections::VecDeque;
use std::sync::Arc;
use termwiz::input::KeyboardEncoding;

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
            keyboard_stack: vec![],
            saved_cursor: None,
        }
    }

    pub fn full_reset(&mut self) {
        self.keyboard_stack.clear();
    }

    fn scrollback_size(&self) -> usize {
        scrollback_size(&self.config, self.allow_scrollback)
    }

    fn rewrap_lines(
        &mut self,
        physical_cols: usize,
        physical_rows: usize,
        cursor_x: usize,
        cursor_y: PhysRowIndex,
        seqno: SequenceNo,
    ) -> (usize, PhysRowIndex) {
        let mut rewrapped = VecDeque::new();
        let mut logical_line: Option<Line> = None;
        let mut logical_cursor_x: Option<usize> = None;
        let mut adjusted_cursor = (cursor_x, cursor_y);

        for (phys_idx, mut line) in self.lines.drain(..).enumerate() {
            line.update_last_change_seqno(seqno);
            let was_wrapped = line.last_cell_was_wrapped();

            if was_wrapped {
                line.set_last_cell_was_wrapped(false, seqno);
            }

            let line = match logical_line.take() {
                None => {
                    if phys_idx == cursor_y {
                        logical_cursor_x = Some(cursor_x);
                    }
                    line
                }
                Some(mut prior) => {
                    if phys_idx == cursor_y {
                        logical_cursor_x = Some(cursor_x + prior.len());
                    }
                    prior.append_line(line, seqno);
                    prior
                }
            };

            if was_wrapped {
                logical_line.replace(line);
                continue;
            }

            if let Some(x) = logical_cursor_x.take() {
                let num_lines = x / physical_cols;
                let last_x = x - (num_lines * physical_cols);
                adjusted_cursor = (last_x, rewrapped.len() + num_lines);

                // Special case: if the cursor lands in column zero, we'll
                // lose track of its logical association with the wrapped
                // line and it won't resize with the line correctly.
                // Put it back on the prior line. The cursor is now
                // technically outside of the viewport width.
                //
                // This only applies when the cursor really did land in
                // column zero *because its logical line wrapped* there
                // (`num_lines > 0`, ie. its offset is a non-zero multiple
                // of the new width): that is the position a terminal
                // parks at the end of the previous row with the wrap
                // pending, so reflow has to park it there too. Testing
                // `adjusted_cursor.1 > 0` instead -- ie. merely "not on
                // the first row of the screen" -- also caught the wholly
                // ordinary cursor a plain newline leaves at the start of
                // its own row, and yanked it up onto the end of the row
                // above (and to `physical_cols`, one past the last valid
                // column) on every widening resize. The next character
                // the program printed then landed at the far right of the
                // previous line and pushed the rest of its output a row
                // down -- which is what garbled a `--start-conf` tab,
                // whose startup commands are typed in immediately and so
                // leave the shell mid-output when the window maximizes.
                if num_lines > 0 && adjusted_cursor.0 == 0 && adjusted_cursor.1 > 0 {
                    if physical_cols < self.physical_cols {
                        // getting smaller: preserve its original position
                        // on the prior line
                        adjusted_cursor.0 = cursor_x;
                    } else {
                        // getting larger; we were most likely in column 1
                        // or somewhere close. Jump to the end of the
                        // prior line.
                        adjusted_cursor.0 = physical_cols;
                    }
                    adjusted_cursor.1 -= 1;
                }
            }

            if line.len() <= physical_cols {
                rewrapped.push_back(line);
            } else {
                for line in line.wrap(physical_cols, seqno) {
                    rewrapped.push_back(line);
                }
            }
        }
        self.lines = rewrapped;

        // If we resized narrower and generated additional lines,
        // we may need to scroll the lines to make room.  However,
        // if the bottom line(s) are whitespace, we'll prune those
        // out first in the rewrap case so that we don't lose any
        // real information off the top of the scrollback
        let capacity = physical_rows + self.scrollback_size();
        while self.lines.len() > capacity
            && self.lines.back().map(Line::is_whitespace).unwrap_or(false)
        {
            self.lines.pop_back();
        }

        adjusted_cursor
    }

    /// Resize the physical, viewable portion of the screen
    pub fn resize(
        &mut self,
        size: TerminalSize,
        cursor: CursorPosition,
        seqno: SequenceNo,
        is_conpty: bool,
        bidi_mode: BidiMode,
    ) -> CursorPosition {
        let physical_rows = size.rows.max(1);
        let physical_cols = size.cols.max(1);

        if physical_rows == self.physical_rows
            && physical_cols == self.physical_cols
            && size.dpi == self.dpi
        {
            return cursor;
        }
        log::debug!(
            "resize screen to {physical_cols}x{physical_rows} dpi={}",
            size.dpi
        );
        self.dpi = size.dpi;

        // pre-prune blank lines that range from the cursor position to the end of the display;
        // this avoids growing the scrollback size when rapidly switching between normal and
        // maximized states.
        let cursor_phys = self.phys_row(cursor.y);
        for _ in cursor_phys + 1..self.lines.len() {
            if self.lines.back().map(Line::is_whitespace).unwrap_or(false) {
                self.lines.pop_back();
            }
        }

        let (cursor_x, cursor_y) = if physical_cols != self.physical_cols {
            // Check to see if we need to rewrap lines that were
            // wrapped due to reaching the right hand side of the terminal.
            // For each one that we find, we need to join it with its
            // successor and then re-split it.
            // We only do this for the primary, and not for the alternate
            // screen (hence the check for allow_scrollback), to avoid
            // conflicting screen updates with full screen apps.
            if self.allow_scrollback {
                self.rewrap_lines(physical_cols, physical_rows, cursor.x, cursor_phys, seqno)
            } else {
                for line in &mut self.lines {
                    if physical_cols < self.physical_cols {
                        // Do a simple prune of the lines instead
                        line.resize(physical_cols, seqno);
                    } else {
                        // otherwise: invalidate them
                        line.update_last_change_seqno(seqno);
                    }
                }
                (cursor.x, cursor_phys)
            }
        } else {
            (cursor.x, cursor_phys)
        };

        let capacity = physical_rows + self.scrollback_size();
        let current_capacity = self.lines.capacity();
        if capacity > current_capacity {
            self.lines.reserve(capacity - current_capacity);
        }

        // If we resized wider and the rewrap resulted in fewer
        // lines than the viewport size, or we resized taller,
        // pad us back out to the viewport size
        while self.lines.len() < physical_rows {
            let mut line = Line::new(seqno);
            bidi_mode.apply_to_line(&mut line, seqno);
            self.lines.push_back(line);
        }

        let new_cursor_y;

        // true if a resize operation should consider rows that have
        // made it to scrollback as being immutable.
        // When immutable, the resize operation will pad out the screen height
        // with additional blank rows and due to implementation details means
        // that the user will need to scroll back the scrollbar post-resize
        // than they would otherwise.
        //
        // When mutable, resizing the window taller won't add extra rows;
        // instead the resize will tend to have "bottom gravity" meaning that
        // making the window taller will reveal more history than in the other
        // mode.
        //
        // mutable is generally speaking a nicer experience.
        //
        // On Windows, the PTY layer doesn't play well with a mutable scrollback,
        // frequently moving the cursor up to high and erasing portions of the
        // screen.
        //
        // This behavior only happens with the windows pty layer; it doesn't
        // manifest when using eg: ssh directly to a remote unix system.
        let resize_preserves_scrollback = is_conpty;

        if resize_preserves_scrollback {
            new_cursor_y = cursor
                .y
                .saturating_add(cursor_y as i64)
                .saturating_sub(cursor_phys as i64)
                .max(0);

            // We need to ensure that the bottom of the screen has sufficient lines;
            // we use simple subtraction of physical_rows from the bottom of the lines
            // array to define the visible region.  Our resize operation may have
            // temporarily violated that, which can result in the cursor unintentionally
            // moving up into the scrollback and damaging the output
            let required_num_rows_after_cursor =
                physical_rows.saturating_sub(new_cursor_y as usize);
            let actual_num_rows_after_cursor = self.lines.len().saturating_sub(cursor_y);
            for _ in actual_num_rows_after_cursor..required_num_rows_after_cursor {
                let mut line = Line::new(seqno);
                bidi_mode.apply_to_line(&mut line, seqno);
                self.lines.push_back(line);
            }
        } else {
            // Compute the new cursor location; this is logically the inverse
            // of the phys_row() function, but given the revised cursor_y
            // (the rewrap adjusted physical row of the cursor).  This
            // computes its new VisibleRowIndex given the new viewport size.
            new_cursor_y = cursor_y as VisibleRowIndex
                - (self.lines.len() as VisibleRowIndex - physical_rows as VisibleRowIndex);
        }

        self.physical_rows = physical_rows;
        self.physical_cols = physical_cols;
        CursorPosition {
            x: cursor_x,
            y: new_cursor_y,
            shape: cursor.shape,
            visibility: cursor.visibility,
            seqno,
        }
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
        let line_idx = self.phys_row(y);
        let line = self.line_mut(line_idx);
        line.set_cell_grapheme(x, text, width, attr, seqno);
    }

    pub fn cell_mut(&mut self, x: usize, y: VisibleRowIndex) -> Option<&mut Cell> {
        let line_idx = self.phys_row(y);
        let line = self.lines.get_mut(line_idx)?;
        line.cells_mut().get_mut(x)
    }

    pub fn get_cell(&mut self, x: usize, y: VisibleRowIndex) -> Option<&Cell> {
        let line_idx = self.phys_row(y);
        let line = self.lines.get_mut(line_idx)?;
        line.cells_mut().get(x)
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

    /// Translate a VisibleRowIndex into a PhysRowIndex.  The resultant index
    /// will be invalidated by inserting or removing rows!
    #[inline]
    pub fn phys_row(&self, row: VisibleRowIndex) -> PhysRowIndex {
        let row = self.clamp_visible_row(row);
        self.lines
            .len()
            .saturating_sub(self.physical_rows)
            .saturating_add(row as PhysRowIndex)
    }

    /// Given a possibly negative row number, return the corresponding physical
    /// row.  This is similar to phys_row() but allows indexing backwards into
    /// the scrollback.
    #[inline]
    pub fn scrollback_or_visible_row(&self, row: ScrollbackOrVisibleRowIndex) -> PhysRowIndex {
        ((self.lines.len() - self.physical_rows) as ScrollbackOrVisibleRowIndex + row).max(0)
            as usize
    }

    #[inline]
    pub fn scrollback_or_visible_range(
        &self,
        range: &Range<ScrollbackOrVisibleRowIndex>,
    ) -> Range<PhysRowIndex> {
        self.scrollback_or_visible_row(range.start)..self.scrollback_or_visible_row(range.end)
    }

    /// Converts a StableRowIndex range to the current effective
    /// physical row index range.  If the StableRowIndex goes off the top
    /// of the scrollback, we'll return the top n rows, but if it goes off
    /// the bottom we'll return the bottom n rows.
    pub fn stable_range(&self, range: &Range<StableRowIndex>) -> Range<PhysRowIndex> {
        let range_len = (range.end - range.start) as usize;

        let first = match self.stable_row_to_phys(range.start) {
            Some(first) => first,
            None => {
                return 0..range_len.min(self.lines.len());
            }
        };

        let last = match self.stable_row_to_phys(range.end.saturating_sub(1)) {
            Some(last) => last,
            None => {
                let last = self.lines.len() - 1;
                return last.saturating_sub(range_len)..last + 1;
            }
        };

        first..last + 1
    }

    /// Translate a range of VisibleRowIndex to a range of PhysRowIndex.
    /// The resultant range will be invalidated by inserting or removing rows!
    #[inline]
    pub fn phys_range(&self, range: &Range<VisibleRowIndex>) -> Range<PhysRowIndex> {
        self.phys_row(range.start)..self.phys_row(range.end)
    }

    #[inline]
    pub fn phys_to_stable_row_index(&self, phys: PhysRowIndex) -> StableRowIndex {
        (phys + self.stable_row_index_offset) as StableRowIndex
    }

    #[inline]
    pub fn stable_row_to_phys(&self, stable: StableRowIndex) -> Option<PhysRowIndex> {
        let idx = stable - self.stable_row_index_offset as isize;
        if idx < 0 || idx >= self.lines.len() as isize {
            // Index is no longer valid
            None
        } else {
            Some(idx as PhysRowIndex)
        }
    }

    #[inline]
    pub fn visible_row_to_stable_row(&self, vis: VisibleRowIndex) -> StableRowIndex {
        self.phys_to_stable_row_index(self.phys_row(vis))
    }

    pub fn lines_in_phys_range(&self, phys_range: Range<PhysRowIndex>) -> Vec<Line> {
        self.lines
            .iter()
            .skip(phys_range.start)
            .take(phys_range.end - phys_range.start)
            .cloned()
            .collect()
    }

    pub fn get_changed_stable_rows(
        &self,
        stable_lines: Range<StableRowIndex>,
        seqno: SequenceNo,
    ) -> Vec<StableRowIndex> {
        let phys = self.stable_range(&stable_lines);
        let mut set = vec![];
        for (idx, line) in self
            .lines
            .iter()
            .enumerate()
            .skip(phys.start)
            .take(phys.end - phys.start)
        {
            if line.changed_since(seqno) {
                set.push(self.phys_to_stable_row_index(idx))
            }
        }
        set
    }

    pub fn with_phys_lines<F>(&self, phys_range: Range<PhysRowIndex>, mut func: F)
    where
        F: FnMut(&[&Line]),
    {
        let (first, second) = self.lines.as_slices();
        let first_len = first.len();
        let first_range = 0..first.len();
        let second_range = first.len()..first.len() + second.len();
        let first_range = phys_intersection(&first_range, &phys_range);
        let second_range = phys_intersection(&second_range, &phys_range);

        let mut lines: Vec<&Line> = Vec::with_capacity(phys_range.end - phys_range.start);
        for line in &first[first_range] {
            lines.push(line);
        }
        for line in &second[second_range.start.saturating_sub(first_len)
            ..second_range.end.saturating_sub(first_len)]
        {
            lines.push(line);
        }
        func(&lines)
    }

    pub fn with_phys_lines_mut<F>(&mut self, phys_range: Range<PhysRowIndex>, mut func: F)
    where
        F: FnMut(&mut [&mut Line]),
    {
        let (first, second) = self.lines.as_mut_slices();
        let first_len = first.len();
        let first_range = 0..first.len();
        let second_range = first.len()..first.len() + second.len();
        let first_range = phys_intersection(&first_range, &phys_range);
        let second_range = phys_intersection(&second_range, &phys_range);

        let mut lines: Vec<&mut Line> = Vec::with_capacity(phys_range.end - phys_range.start);
        for line in &mut first[first_range] {
            lines.push(line);
        }
        for line in &mut second[second_range.start.saturating_sub(first_len)
            ..second_range.end.saturating_sub(first_len)]
        {
            lines.push(line);
        }
        func(&mut lines)
    }

    pub fn for_each_phys_line<F>(&self, mut f: F)
    where
        F: FnMut(usize, &Line),
    {
        for (idx, line) in self.lines.iter().enumerate() {
            f(idx, line);
        }
    }

    pub fn for_each_phys_line_mut<F>(&mut self, mut f: F)
    where
        F: FnMut(usize, &mut Line),
    {
        for (idx, line) in self.lines.iter_mut().enumerate() {
            f(idx, line);
        }
    }

    pub fn for_each_logical_line_in_stable_range_mut<F>(
        &mut self,
        stable_range: Range<StableRowIndex>,
        mut f: F,
    ) where
        F: FnMut(Range<StableRowIndex>, &mut [&mut Line]) -> bool,
    {
        let mut phys_range = self.stable_range(&stable_range);

        // Avoid pathological cases where we have eg: a really long logical line
        // (such as 1.5MB of json) that we previously wrapped.  We don't want to
        // un-wrap, scan, and re-wrap that thing.
        // This is an imperfect length constraint to partially manage the cost.
        const MAX_LOGICAL_LINE_LEN: usize = 1024;

        // Look backwards to find the start of the first logical line
        let mut back_len = 0;
        while phys_range.start > 0 {
            let prior = &mut self.lines[phys_range.start - 1];
            if !prior.last_cell_was_wrapped() {
                break;
            }
            if prior.len() + back_len > MAX_LOGICAL_LINE_LEN {
                break;
            }
            back_len += prior.len();
            phys_range.start -= 1
        }

        let mut phys_row = phys_range.start;
        while phys_row < phys_range.end {
            // Look forwards until we find the end of this logical line
            let mut total_len = 0;
            let mut end_inclusive = phys_row;

            // First pass to measure number of lines
            for idx in phys_row.. {
                if let Some(line) = self.lines.get(idx) {
                    if total_len > 0 && total_len + line.len() > MAX_LOGICAL_LINE_LEN {
                        break;
                    }
                    end_inclusive = idx;
                    total_len += line.len();
                    if !line.last_cell_was_wrapped() {
                        break;
                    }
                } else if idx == phys_row {
                    // No more rows exist
                    return;
                } else {
                    break;
                }
            }

            let phys_range = phys_row..end_inclusive + 1;

            let logical_stable_range = self.phys_to_stable_row_index(phys_row)
                ..self.phys_to_stable_row_index(end_inclusive + 1);

            phys_row = end_inclusive + 1;

            if logical_stable_range.end < stable_range.start {
                continue;
            }
            if logical_stable_range.start > stable_range.end {
                break;
            }

            let mut continue_iteration = false;
            self.with_phys_lines_mut(phys_range, |lines| {
                continue_iteration = f(logical_stable_range.clone(), lines);
            });

            if !continue_iteration {
                break;
            }
        }
    }

    pub fn for_each_logical_line_in_stable_range<F>(
        &self,
        stable_range: Range<StableRowIndex>,
        mut f: F,
    ) where
        F: FnMut(Range<StableRowIndex>, &[&Line]) -> bool,
    {
        let mut phys_range = self.stable_range(&stable_range);

        // Avoid pathological cases where we have eg: a really long logical line
        // (such as 1.5MB of json) that we previously wrapped.  We don't want to
        // un-wrap, scan, and re-wrap that thing.
        // This is an imperfect length constraint to partially manage the cost.
        const MAX_LOGICAL_LINE_LEN: usize = 1024;

        // Look backwards to find the start of the first logical line
        let mut back_len = 0;
        while phys_range.start > 0 {
            let prior = &self.lines[phys_range.start - 1];
            if !prior.last_cell_was_wrapped() {
                break;
            }
            if prior.len() + back_len > MAX_LOGICAL_LINE_LEN {
                break;
            }
            back_len += prior.len();
            phys_range.start -= 1
        }

        let mut phys_row = phys_range.start;
        let mut line_vec: Vec<&Line> = vec![];
        while phys_row < phys_range.end {
            // Look forwards until we find the end of this logical line
            let mut total_len = 0;
            let mut end_inclusive = phys_row;
            line_vec.clear();

            for idx in phys_row.. {
                if let Some(line) = self.lines.get(idx) {
                    if total_len > 0 && total_len + line.len() > MAX_LOGICAL_LINE_LEN {
                        break;
                    }
                    end_inclusive = idx;
                    total_len += line.len();
                    line_vec.push(line);
                    if !line.last_cell_was_wrapped() {
                        break;
                    }
                } else if idx == phys_row {
                    // No more rows exist
                    return;
                } else {
                    break;
                }
            }

            let logical_stable_range = self.phys_to_stable_row_index(phys_row)
                ..self.phys_to_stable_row_index(end_inclusive + 1);

            phys_row = end_inclusive + 1;

            if logical_stable_range.end < stable_range.start {
                continue;
            }
            if logical_stable_range.start > stable_range.end {
                break;
            }

            let continue_iteration = f(logical_stable_range, &line_vec);

            if !continue_iteration {
                break;
            }
        }
    }
}

fn phys_intersection(r1: &Range<PhysRowIndex>, r2: &Range<PhysRowIndex>) -> Range<PhysRowIndex> {
    let start = r1.start.max(r2.start);
    let end = r1.end.min(r2.end);
    if end > start {
        start..end
    } else {
        0..0
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
