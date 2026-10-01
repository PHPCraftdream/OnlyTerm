use super::*;

#[cfg(test)]
thread_local! {
    static REFERENCE_REFLOW: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Test-only: run `f` with the reflow helpers routed to the verbatim
/// reference implementations (`wrap_keeping_reference` /
/// `append_line_reference`). Absent in non-test builds.
#[cfg(test)]
pub(crate) fn with_reference_reflow<R>(f: impl FnOnce() -> R) -> R {
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            REFERENCE_REFLOW.with(|flag| flag.set(self.0));
        }
    }
    let _restore = REFERENCE_REFLOW.with(|flag| {
        let prior = flag.get();
        flag.set(true);
        Restore(prior)
    });
    f()
}

/// Joins a wrapped line with its successor; the test hook routes this
/// to the reference implementation.
#[inline]
fn join_rows(prior: &mut Line, line: Line, seqno: SequenceNo) {
    #[cfg(test)]
    {
        if REFERENCE_REFLOW.with(std::cell::Cell::get) {
            prior.append_line_reference(line, seqno);
        } else {
            prior.append_line(line, seqno);
        }
    }
    #[cfg(not(test))]
    {
        prior.append_line(line, seqno);
    }
}

/// Splits an over-wide line for the new width; the test hook routes
/// this to the reference implementation.
#[inline]
fn split_row(line: Line, cols: usize, keep: usize, seqno: SequenceNo) -> Vec<Line> {
    #[cfg(test)]
    {
        if REFERENCE_REFLOW.with(std::cell::Cell::get) {
            line.wrap_keeping_reference(cols, keep, seqno)
        } else {
            line.wrap_keeping(cols, keep, seqno)
        }
    }
    #[cfg(not(test))]
    {
        line.wrap_keeping(cols, keep, seqno)
    }
}

impl Screen {
    /// `conpty_top` is the first row of ConPTY's buffer (the old viewport
    /// top) in ConPTY mode; the third result is where that row now starts.
    fn rewrap_lines(
        &mut self,
        physical_cols: usize,
        physical_rows: usize,
        (cursor_x, cursor_y): (usize, PhysRowIndex),
        saved_cursor: Option<(usize, PhysRowIndex)>,
        seqno: SequenceNo,
        conpty_top: Option<PhysRowIndex>,
    ) -> (
        (usize, PhysRowIndex),
        Option<(usize, PhysRowIndex)>,
        PhysRowIndex,
    ) {
        let is_conpty = conpty_top.is_some();
        let mut rewrapped = VecDeque::new();
        let mut new_top = 0;
        let mut logical_line: Option<Line> = None;
        let mut logical_cursor_x: Option<usize> = None;
        let mut logical_saved_x: Option<usize> = None;
        let mut adjusted_cursor = (cursor_x, cursor_y);
        let mut adjusted_saved = saved_cursor;

        let old_cols = self.physical_cols;
        let total = self.lines.len();
        for (phys_idx, mut line) in self.lines.drain(..).enumerate() {
            if Some(phys_idx) == conpty_top {
                new_top = rewrapped.len();
            }
            line.update_last_change_seqno(seqno);
            let was_wrapped = line.last_cell_was_wrapped();

            if was_wrapped {
                line.set_last_cell_was_wrapped(false, seqno);
                // ConPTY measures a wrapped row at its full width.
                if is_conpty && line.len() < old_cols {
                    line.resize(old_cols, seqno);
                }
            }

            let prior_len = logical_line.as_ref().map_or(0, Line::len);
            if phys_idx == cursor_y {
                logical_cursor_x = Some(cursor_x + prior_len);
            }
            if let Some((saved_x, saved_y)) = saved_cursor {
                if phys_idx == saved_y {
                    logical_saved_x = Some(saved_x + prior_len);
                }
            }

            let line = match logical_line.take() {
                None => line,
                Some(mut prior) => {
                    join_rows(&mut prior, line, seqno);
                    prior
                }
            };

            // A wrapped bottom row has no successor to join; flush it here.
            let is_last = phys_idx + 1 == total;
            if was_wrapped && !is_last {
                logical_line.replace(line);
                continue;
            }

            if let Some(x) = logical_cursor_x.take() {
                adjusted_cursor = Self::rewrapped_cursor_position(
                    x,
                    cursor_x,
                    rewrapped.len(),
                    physical_cols,
                    self.physical_cols,
                );
            }
            if let (Some(x), Some((saved_x, _))) = (logical_saved_x.take(), saved_cursor) {
                adjusted_saved = Some(Self::rewrapped_cursor_position(
                    x,
                    saved_x,
                    rewrapped.len(),
                    physical_cols,
                    self.physical_cols,
                ));
            }

            // ConPTY keeps the blanks of wrapped rows; only trailing blanks
            // of the final, unwrapped row are dropped.
            let keep = match (is_conpty, was_wrapped) {
                (false, _) => 0,
                (true, true) => line.len(),
                (true, false) => prior_len,
            };

            if line.len() <= physical_cols {
                rewrapped.push_back(line);
            } else {
                for line in split_row(line, physical_cols, keep, seqno) {
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

        (adjusted_cursor, adjusted_saved, new_top)
    }

    /// ConPTY's reflow stops one buffer height below the cursor row. Drops
    /// the blank rows past `limit` and reports whether the reflow was cut
    /// short there, i.e. rows past it still held text or a wrap mark.
    fn conpty_cut_below(&mut self, limit: usize) -> bool {
        let mut cut = false;
        while self.lines.len() > limit
            && self.lines.back().map(Line::is_whitespace).unwrap_or(false)
        {
            cut |= self
                .lines
                .back()
                .map(Line::last_cell_was_wrapped)
                .unwrap_or(false);
            self.lines.pop_back();
        }
        cut || self.lines.len() > limit
    }

    /// End of ConPTY's output extent: its last row holding text or a wrap
    /// mark, and at least the cursor row.
    fn conpty_extent(&self, cursor_y: usize) -> usize {
        let text_end = self
            .lines
            .iter()
            .rposition(|line| line.last_cell_was_wrapped() || !line.is_whitespace())
            .map_or(0, |idx| idx + 1);
        text_end.max(cursor_y + 1).min(self.lines.len())
    }

    /// ConPTY's reflow copies rows up to its extent. A same-width reflow that
    /// is not cut short leaves the last copied row unwrapped; a width reflow
    /// already set the marks in `rewrap_lines`. Returns the resulting extent.
    fn conpty_reflow_extent(
        &mut self,
        cursor_y: usize,
        unwrap_last: bool,
        seqno: SequenceNo,
    ) -> usize {
        let extent = self.conpty_extent(cursor_y);
        if unwrap_last && extent > 0 && self.lines[extent - 1].last_cell_was_wrapped() {
            self.lines[extent - 1].set_last_cell_was_wrapped(false, seqno);
            return self.conpty_extent(cursor_y);
        }
        extent
    }

    fn rewrapped_cursor_position(
        logical_x: usize,
        original_x: usize,
        line_start: usize,
        new_cols: usize,
        old_cols: usize,
    ) -> (usize, PhysRowIndex) {
        let rows = logical_x / new_cols;
        let mut x = logical_x % new_cols;
        let mut y = line_start + rows;
        // A nonzero logical offset at column zero belongs to the prior row.
        if rows > 0 && x == 0 && y > 0 {
            x = if new_cols < old_cols {
                original_x
            } else {
                new_cols
            };
            y -= 1;
        }
        (x, y)
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
        let saved_cursor_phys = self
            .saved_cursor
            .as_ref()
            .map(|saved| (saved.position.x, self.phys_row(saved.position.y)));
        let old_top = self.lines.len().saturating_sub(self.physical_rows);
        if is_conpty && self.allow_scrollback {
            // Erase-to-end can paint unused padding without extending output.
            let output_end = (old_top + self.output_rows).max(cursor_phys + 1);
            while self.lines.len() > output_end
                && self.lines.back().map(Line::is_whitespace).unwrap_or(false)
            {
                self.lines.pop_back();
            }
        }
        let reflowed = physical_cols != self.physical_cols && self.allow_scrollback;
        let mut cut = false;
        if is_conpty && self.allow_scrollback {
            // A width change applies ConPTY's row limits after the reflow.
            if !reflowed {
                // Native ConPTY shifts rows upward on shrink until the cursor
                // reaches the top. Pruning all trailing blanks would instead
                // pin the prompt while subsequent absolute cursor updates move.
                let shrink = self.lines.len().saturating_sub(old_top + physical_rows);
                let shift = shrink.min(cursor.y.max(0) as usize);
                if shrink > shift {
                    cut = self.conpty_cut_below(self.lines.len() - (shrink - shift));
                }
            }
        } else {
            let prune_limit = cursor_phys + 1;
            for _ in prune_limit..self.lines.len() {
                if self.lines.back().map(Line::is_whitespace).unwrap_or(false) {
                    self.lines.pop_back();
                }
            }
        }

        if is_conpty && reflowed && old_top > 0 {
            // ConPTY's buffer starts at the viewport: a line wrapped from
            // history reflows there as a line of its own.
            let line = &mut self.lines[old_top - 1];
            if line.last_cell_was_wrapped() {
                line.set_last_cell_was_wrapped(false, seqno);
            }
        }
        let mut new_top = old_top;
        let ((cursor_x, cursor_y), saved_cursor) = if physical_cols != self.physical_cols {
            // Check to see if we need to rewrap lines that were
            // wrapped due to reaching the right hand side of the terminal.
            // For each one that we find, we need to join it with its
            // successor and then re-split it.
            // We only do this for the primary, and not for the alternate
            // screen (hence the check for allow_scrollback), to avoid
            // conflicting screen updates with full screen apps.
            if self.allow_scrollback {
                let (cursor, saved, top) = self.rewrap_lines(
                    physical_cols,
                    physical_rows,
                    (cursor.x, cursor_phys),
                    saved_cursor_phys,
                    seqno,
                    is_conpty.then_some(old_top),
                );
                new_top = top;
                (cursor, saved)
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
                ((cursor.x, cursor_phys), saved_cursor_phys)
            }
        } else {
            ((cursor.x, cursor_phys), saved_cursor_phys)
        };

        let capacity = physical_rows + self.scrollback_size();
        let mut output_end = self.lines.len();
        // Viewport top after a width change, following ConPTY's buffer.
        let mut conpty_top = None;
        if is_conpty && reflowed {
            // ConPTY first reflows the width at the taller of both heights,
            // dropping rows off its top once they exceed that height.
            let tallest = physical_rows.max(self.physical_rows);
            self.conpty_cut_below(cursor_y + tallest);
            output_end = self.conpty_reflow_extent(cursor_y, false, seqno);
            let mut top = new_top + self.lines.len().saturating_sub(new_top + tallest);
            // A shorter height triggers a second reflow, which copies rows
            // only up to that extent.
            if physical_rows < self.physical_rows {
                while self.lines.len() > output_end
                    && self.lines.back().map(Line::is_whitespace).unwrap_or(false)
                {
                    self.lines.pop_back();
                }
                let cut = self.conpty_cut_below(cursor_y + physical_rows);
                output_end = self.conpty_reflow_extent(cursor_y, !cut, seqno);
                top += self.lines.len().saturating_sub(top + physical_rows);
            }
            conpty_top = Some(top.min(cursor_y));
        } else if is_conpty && self.allow_scrollback && physical_rows != self.physical_rows {
            output_end = self.conpty_reflow_extent(cursor_y, !cut, seqno);
        }
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
            let preserved_cursor_y = match conpty_top {
                Some(top) => (cursor_y - top) as i64,
                None => cursor
                    .y
                    .saturating_add(cursor_y as i64)
                    .saturating_sub(cursor_phys as i64)
                    .max(0),
            };

            // We need to ensure that the bottom of the screen has sufficient lines;
            // we use simple subtraction of physical_rows from the bottom of the lines
            // array to define the visible region.  Our resize operation may have
            // temporarily violated that, which can result in the cursor unintentionally
            // moving up into the scrollback and damaging the output
            let required_num_rows_after_cursor =
                physical_rows.saturating_sub(preserved_cursor_y as usize);
            let actual_num_rows_after_cursor = self.lines.len().saturating_sub(cursor_y);
            for _ in actual_num_rows_after_cursor..required_num_rows_after_cursor {
                let mut line = Line::new(seqno);
                bidi_mode.apply_to_line(&mut line, seqno);
                self.lines.push_back(line);
            }
        }

        // Padding preserves ConPTY's viewport on growth. On shrink, nonblank
        // rows below the cursor can still move its line toward the top; derive
        // the cursor from that line, not its pre-resize visible row number.
        let new_cursor_y = cursor_y as VisibleRowIndex
            - (self.lines.len() as VisibleRowIndex - physical_rows as VisibleRowIndex);

        if let (Some(saved), Some((x, y))) = (self.saved_cursor.as_mut(), saved_cursor) {
            saved.position.x = x;
            saved.position.y = y as VisibleRowIndex
                - (self.lines.len() as VisibleRowIndex - physical_rows as VisibleRowIndex);
            saved.position.seqno = seqno;
            saved.wrap_next = false;
        }

        self.output_rows = output_end
            .saturating_sub(self.lines.len() - physical_rows)
            .min(physical_rows);
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
}
