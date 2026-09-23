use super::*;

impl TerminalState {
    pub(crate) fn clear_semantic_attribute_due_to_movement(&mut self) {
        if self.clear_semantic_attribute_on_newline {
            self.clear_semantic_attribute_on_newline = false;
            self.pen.set_semantic_type(SemanticType::default());
        }
    }

    /// Sets the cursor position to precisely the x and values provided
    fn set_cursor_position_absolute(&mut self, x: usize, y: VisibleRowIndex) {
        if self.cursor.y != y {
            self.clear_semantic_attribute_due_to_movement();
        }
        self.cursor.y = y;
        self.cursor.x = x;
        self.cursor.seqno = self.seqno;
        self.wrap_next = false;
    }

    /// Sets the cursor position. x and y are 0-based and relative to the
    /// top left of the visible screen.
    pub(crate) fn set_cursor_pos(&mut self, x: &Position, y: &Position) {
        let x = match *x {
            Position::Relative(x) => (self.cursor.x as i64 + x)
                .min(
                    if self.dec_origin_mode {
                        self.left_and_right_margins.end
                    } else {
                        self.screen().physical_cols
                    } as i64
                        - 1,
                )
                .max(0),
            Position::Absolute(x) => (x + if self.dec_origin_mode {
                self.left_and_right_margins.start
            } else {
                0
            } as i64)
                .min(
                    if self.dec_origin_mode {
                        self.left_and_right_margins.end
                    } else {
                        // We allow 1 extra for the cursor x position
                        // to account for some resize/rewrap scenarios
                        // where we don't want to forget that the
                        // cursor belongs to a wrapped line
                        self.screen().physical_cols + 1
                    } as i64
                        - 1,
                )
                .max(0),
        };

        let y = match *y {
            Position::Relative(y) => (self.cursor.y + y)
                .min(
                    if self.dec_origin_mode {
                        self.top_and_bottom_margins.end
                    } else {
                        self.screen().physical_rows as i64
                    } - 1,
                )
                .max(0),
            Position::Absolute(y) => (y + if self.dec_origin_mode {
                self.top_and_bottom_margins.start
            } else {
                0
            })
            .min(
                if self.dec_origin_mode {
                    self.top_and_bottom_margins.end
                } else {
                    self.screen().physical_rows as i64
                } - 1,
            )
            .max(0),
        };

        self.set_cursor_position_absolute(x as usize, y);
    }

    pub(crate) fn scroll_up(&mut self, num_rows: usize) {
        let seqno = self.seqno;
        let blank_attr = self.pen.clone_sgr_only();
        let top_and_bottom_margins = self.top_and_bottom_margins.clone();
        let left_and_right_margins = self.left_and_right_margins.clone();
        let bidi_mode = self.get_bidi_mode();
        self.screen_mut().scroll_up_within_margins(
            &top_and_bottom_margins,
            &left_and_right_margins,
            num_rows,
            seqno,
            blank_attr,
            bidi_mode,
        )
    }

    pub(crate) fn scroll_down(&mut self, num_rows: usize) {
        let seqno = self.seqno;
        let blank_attr = self.pen.clone_sgr_only();
        let top_and_bottom_margins = self.top_and_bottom_margins.clone();
        let left_and_right_margins = self.left_and_right_margins.clone();
        let bidi_mode = self.get_bidi_mode();
        self.screen_mut().scroll_down_within_margins(
            &top_and_bottom_margins,
            &left_and_right_margins,
            num_rows,
            seqno,
            blank_attr,
            bidi_mode,
        )
    }

    /// Defined by FinalTermSemanticPrompt; a fresh-line is a NOP if the
    /// cursor is already at the left margin, otherwise it is the same as
    /// a new line.
    pub(crate) fn fresh_line(&mut self) {
        if self.cursor.x == self.left_and_right_margins.start {
            return;
        }
        self.new_line(true);
    }

    pub(crate) fn new_line(&mut self, move_to_first_column: bool) {
        let x = if move_to_first_column {
            self.left_and_right_margins.start
        } else {
            self.cursor.x
        };
        let y = self.cursor.y;
        let y = if y == self.top_and_bottom_margins.end - 1 {
            self.scroll_up(1);
            y
        } else {
            y + 1
        };
        self.set_cursor_pos(&Position::Absolute(x as i64), &Position::Absolute(y));
    }

    /// Moves the cursor down one line in the same column.
    /// If the cursor is at the bottom margin, the page scrolls up.
    pub(crate) fn c1_index(&mut self) {
        if self.left_and_right_margins.contains(&self.cursor.x) {
            if self.cursor.y == self.top_and_bottom_margins.end - 1 {
                self.scroll_up(1);
            } else {
                self.set_cursor_pos(&Position::Relative(0), &Position::Relative(1));
            }
        }
    }

    /// Moves the cursor to the first position on the next line.
    /// If the cursor is at the bottom margin, the page scrolls up.
    pub(crate) fn c1_nel(&mut self) {
        let y_clamp = if self.top_and_bottom_margins.contains(&self.cursor.y) {
            self.top_and_bottom_margins.end - 1
        } else {
            self.screen().physical_rows as VisibleRowIndex - 1
        };

        if self.left_and_right_margins.contains(&self.cursor.x) {
            if self.cursor.y == self.top_and_bottom_margins.end - 1 {
                self.scroll_up(1);
                self.set_cursor_position_absolute(self.left_and_right_margins.start, self.cursor.y);
            } else {
                self.set_cursor_position_absolute(
                    self.left_and_right_margins.start,
                    (self.cursor.y + 1).min(y_clamp),
                );
            }
        } else {
            // When outside left/right margins, NEL moves but does not scroll
            self.set_cursor_position_absolute(
                if self.cursor.x < self.left_and_right_margins.start {
                    self.cursor.x
                } else {
                    self.left_and_right_margins.start
                },
                (self.cursor.y + 1).min(y_clamp),
            );
        }
    }

    /// Sets a horizontal tab stop at the column where the cursor is.
    pub(crate) fn c1_hts(&mut self) {
        self.tabs.set_tab_stop(self.cursor.x);
    }

    /// Moves the cursor to the next tab stop. If there are no more tab stops,
    /// the cursor moves to the right margin. HT does not cause text to auto
    /// wrap.
    pub(crate) fn c0_horizontal_tab(&mut self) {
        let seqno = self.seqno;
        let x = match self.tabs.find_next_tab_stop(self.cursor.x) {
            Some(x) => x,
            None => self.left_and_right_margins.end - 1,
        };
        self.cursor.x = x.min(self.left_and_right_margins.end - 1);
        self.cursor.seqno = seqno;
    }

    /// Move the cursor up 1 line.  If the position is at the top scroll margin,
    /// scroll the region down.
    pub(crate) fn c1_reverse_index(&mut self) {
        if self.left_and_right_margins.contains(&self.cursor.x) {
            if self.cursor.y == self.top_and_bottom_margins.start {
                self.scroll_down(1);
            } else {
                self.set_cursor_pos(&Position::Relative(0), &Position::Relative(-1));
            }
        }
    }

    pub(crate) fn set_hyperlink(&mut self, link: Option<Hyperlink>) {
        self.pen.set_hyperlink(link.map(Arc::new));
    }

    pub(crate) fn erase_in_display(&mut self, erase: EraseInDisplay) {
        let seqno = self.seqno;
        let cy = self.cursor.y;
        let pen = self.pen.clone_sgr_only();
        let rows = self.screen().physical_rows as VisibleRowIndex;
        let col_range = 0..self.screen().physical_cols;
        let row_range = match erase {
            EraseInDisplay::EraseToEndOfDisplay => {
                self.perform_csi_edit(Edit::EraseInLine(EraseInLine::EraseToEndOfLine));
                cy + 1..rows
            }
            EraseInDisplay::EraseToStartOfDisplay => {
                self.perform_csi_edit(Edit::EraseInLine(EraseInLine::EraseToStartOfLine));
                0..cy
            }
            EraseInDisplay::EraseDisplay => 0..rows,
            EraseInDisplay::EraseScrollback => {
                self.screen_mut().erase_scrollback();
                return;
            }
        };

        {
            let bidi_mode = self.get_bidi_mode();
            let screen = self.screen_mut();
            for y in row_range {
                screen.clear_line(y, col_range.clone(), &pen, seqno, bidi_mode);
                let line_idx = screen.phys_row(y);
                screen.line_mut(line_idx).set_single_width(seqno);
            }
        }
    }

    pub(crate) fn get_bidi_mode(&self) -> BidiMode {
        let mut mode = self.config.bidi_mode();
        if let Some(enabled) = &self.bidi_enabled {
            mode.enabled = *enabled;
        }
        if let Some(hint) = &self.bidi_hint {
            mode.hint = *hint;
        }
        mode
    }

    /// Computes the set of `SemanticZone`s for the current terminal screen.
    /// Semantic zones are contiguous runs of cells that have the same
    /// `SemanticType` (Prompt, Input, Output).
    /// Due to the way that the terminal clears the screen, the raw, literal
    /// set of zones is overly fragmented by blanks.  This method will ignore
    /// trailing Output regions when computing the SemanticZone bounds.
    ///
    /// By default, all screen data is of type Output.  The shell needs to
    /// employ OSC 133 escapes to markup its output.
    pub fn get_semantic_zones(&mut self) -> anyhow::Result<Vec<SemanticZone>> {
        let screen = self.screen_mut();

        let mut current_zone: Option<SemanticZone> = None;
        let mut zones = vec![];

        let first_stable_row = screen.phys_to_stable_row_index(0);
        screen.for_each_phys_line_mut(|idx, line| {
            let stable_row = first_stable_row + idx as StableRowIndex;

            for zone_range in line.semantic_zone_ranges() {
                let new_zone = match current_zone.as_ref() {
                    None => true,
                    Some(zone) => zone.semantic_type != zone_range.semantic_type,
                };

                if new_zone {
                    if let Some(zone) = current_zone.take() {
                        zones.push(zone);
                    }

                    current_zone.replace(SemanticZone {
                        start_x: zone_range.range.start as usize,
                        start_y: stable_row,
                        end_x: zone_range.range.end as usize,
                        end_y: stable_row,
                        semantic_type: zone_range.semantic_type,
                    });
                }

                if let Some(zone) = current_zone.as_mut() {
                    zone.end_x = zone_range.range.end as usize;
                    zone.end_y = stable_row;
                }
            }
        });
        if let Some(zone) = current_zone.take() {
            zones.push(zone);
        }

        Ok(zones)
    }
}
