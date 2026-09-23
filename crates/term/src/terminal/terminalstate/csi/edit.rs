#![allow(clippy::range_plus_one)]
use super::*;

impl TerminalState {
    pub(crate) fn perform_csi_edit(&mut self, edit: Edit) {
        let seqno = self.seqno;
        match edit {
            Edit::DeleteCharacter(n) => {
                let y = self.cursor.y;
                let x = self.cursor.x;

                if x >= self.left_and_right_margins.start && x < self.left_and_right_margins.end {
                    let right_margin = self.left_and_right_margins.end;
                    let limit = (x + n as usize).min(right_margin);

                    let blank_attr = self.pen.clone_sgr_only();
                    let screen = self.screen_mut();
                    for _ in x..limit {
                        screen.erase_cell(x, y, right_margin, seqno, blank_attr.clone());
                    }
                }
            }
            Edit::DeleteLine(n) => {
                if self.top_and_bottom_margins.contains(&self.cursor.y)
                    && self.left_and_right_margins.contains(&self.cursor.x)
                {
                    let top_and_bottom_margins = self.cursor.y..self.top_and_bottom_margins.end;
                    let left_and_right_margins = self.left_and_right_margins.clone();
                    let blank_attr = self.pen.clone_sgr_only();
                    let bidi_mode = self.get_bidi_mode();
                    self.screen_mut().scroll_up_within_margins(
                        &top_and_bottom_margins,
                        &left_and_right_margins,
                        n as usize,
                        seqno,
                        blank_attr,
                        bidi_mode,
                    );
                }
            }
            Edit::EraseCharacter(n) => {
                let y = self.cursor.y;
                let x = self.cursor.x;
                let limit = (x + n as usize).min(self.screen().physical_cols);
                {
                    let blank = Cell::blank_with_attrs(self.pen.clone_sgr_only());
                    let screen = self.screen_mut();
                    for x in x..limit {
                        screen.set_cell(x, y, &blank, seqno);
                    }
                }
            }

            Edit::EraseInLine(erase) => {
                let cx = self.cursor.x;
                let cy = self.cursor.y;
                let pen = self.pen.clone_sgr_only();
                let cols = self.screen().physical_cols;
                let bidi_mode = self.get_bidi_mode();
                let range = match erase {
                    // If wrap_next is true, then cx is effectively 1 column to the right.
                    // It feels wrong to handle this here, but in trying to centralize
                    // the logic for updating the cursor position, it causes regressions
                    // in the test suite.
                    // So this is here for now until a better solution is found.
                    // <https://github.com/wezterm/wezterm/issues/3548>
                    EraseInLine::EraseToEndOfLine => cx + if self.wrap_next { 1 } else { 0 }..cols,
                    EraseInLine::EraseToStartOfLine => 0..cx + 1,
                    EraseInLine::EraseLine => 0..cols,
                };

                self.screen_mut()
                    .clear_line(cy, range, &pen, seqno, bidi_mode);
            }
            Edit::InsertCharacter(n) => {
                // https://vt100.net/docs/vt510-rm/ICH.html
                // The ICH sequence inserts Pn blank characters with the normal character
                // attribute. The cursor remains at the beginning of the blank characters. Text
                // between the cursor and right margin moves to the right. Characters scrolled past
                // the right margin are lost. ICH has no effect outside the scrolling margins.

                let y = self.cursor.y;
                let x = self.cursor.x;
                if self.top_and_bottom_margins.contains(&y)
                    && self.left_and_right_margins.contains(&x)
                {
                    let margin = self.left_and_right_margins.end;
                    let screen = self.screen_mut();
                    for _ in 0..n as usize {
                        screen.insert_cell(x, y, margin, seqno);
                    }
                }
            }
            Edit::InsertLine(n) => {
                if self.top_and_bottom_margins.contains(&self.cursor.y)
                    && self.left_and_right_margins.contains(&self.cursor.x)
                {
                    let bidi_mode = self.get_bidi_mode();
                    let top_and_bottom_margins = self.cursor.y..self.top_and_bottom_margins.end;
                    let left_and_right_margins = self.left_and_right_margins.clone();
                    let blank_attr = self.pen.clone_sgr_only();
                    self.screen_mut().scroll_down_within_margins(
                        &top_and_bottom_margins,
                        &left_and_right_margins,
                        n as usize,
                        seqno,
                        blank_attr,
                        bidi_mode,
                    );
                }
            }
            Edit::ScrollDown(n) => self.scroll_down(n as usize),
            Edit::ScrollUp(n) => self.scroll_up(n as usize),
            Edit::EraseInDisplay(erase) => self.erase_in_display(erase),
            Edit::Repeat(n) => {
                let mut y = self.cursor.y;
                let mut x = self.cursor.x;
                let left_and_right_margins = self.left_and_right_margins.clone();
                let top_and_bottom_margins = self.top_and_bottom_margins.clone();

                // Resolve the source cell.  It may be a double-wide character.
                let cell = {
                    let screen = self.screen_mut();
                    let to_copy = x.saturating_sub(1);
                    let line_idx = screen.phys_row(y);
                    let line = screen.line_mut(line_idx);

                    match line.cells_mut().get(to_copy).cloned() {
                        None => Cell::blank(),
                        Some(candidate) => {
                            if candidate.str() == " " && to_copy > 0 {
                                // It's a blank.  It may be the second part of
                                // a double-wide pair; look ahead of it.
                                let prior = &line.cells_mut()[to_copy - 1];
                                if prior.width() > 1 {
                                    prior.clone()
                                } else {
                                    candidate
                                }
                            } else {
                                candidate
                            }
                        }
                    }
                };

                for _ in 0..n {
                    {
                        let screen = self.screen_mut();
                        let line_idx = screen.phys_row(y);
                        let line = screen.line_mut(line_idx);

                        line.set_cell(x, cell.clone(), seqno);
                    }
                    x += 1;
                    if x > left_and_right_margins.end - 1 {
                        x = left_and_right_margins.start;
                        if y == top_and_bottom_margins.end - 1 {
                            self.scroll_up(1);
                        } else {
                            y += 1;
                            if y > top_and_bottom_margins.end - 1 {
                                y = top_and_bottom_margins.end;
                            }
                        }
                    }
                }
                self.cursor.x = x;
                self.cursor.y = y;
            }
        }
    }

    /// https://vt100.net/docs/vt510-rm/DECSLRM.html
    pub(crate) fn set_left_and_right_margins(&mut self, left: OneBased, right: OneBased) {
        // The terminal only recognizes this control function if vertical split
        // screen mode (DECLRMM) is set.
        if self.left_and_right_margin_mode {
            let cols = self.screen().physical_cols as u32;
            let left = left.as_zero_based().min(cols - 1) as usize;
            let right = right.as_zero_based().min(cols - 1) as usize;

            // The value of the left margin (Pl) must be less than the right margin (Pr).
            if left >= right {
                return;
            }

            // The minimum size of the scrolling region is two columns per DEC,
            // but xterm allows 1.
            /*
            if right - left < 2 {
                return;
            }
            */

            self.left_and_right_margins = left..right + 1;

            // DECSLRM moves the cursor to column 1, line 1 of the page.
            self.set_cursor_pos(&Position::Absolute(0), &Position::Absolute(0));
        }
    }
}
