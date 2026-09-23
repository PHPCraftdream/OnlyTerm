use super::*;

impl TerminalState {
    /// Informs the terminal that the viewport of the window has resized to the
    /// specified dimensions.
    /// We need to resize both the primary and alt screens, adjusting
    /// the cursor positions of both accordingly.
    pub fn resize(&mut self, size: TerminalSize) {
        self.increment_seqno();
        let discard_new_blank_history = self.enable_conpty_quirks
            && !self.screen.alt_screen_is_active
            && size.rows.max(1) < self.screen.screen.physical_rows
            && self.screen.screen.scrollback_rows() == self.screen.screen.physical_rows;
        let (cursor_main, cursor_alt) = if self.screen.alt_screen_is_active {
            (
                self.screen
                    .screen
                    .saved_cursor
                    .as_ref()
                    .map(|s| s.position)
                    .unwrap_or_default(),
                self.cursor,
            )
        } else {
            (
                self.cursor,
                self.screen
                    .alt_screen
                    .saved_cursor
                    .as_ref()
                    .map(|s| s.position)
                    .unwrap_or_default(),
            )
        };

        let bidi_mode = self.get_bidi_mode();
        let (adjusted_cursor_main, adjusted_cursor_alt) = self.screen.resize(
            size,
            cursor_main,
            cursor_alt,
            self.seqno,
            self.enable_conpty_quirks,
            bidi_mode,
        );
        if discard_new_blank_history {
            let palette = self.palette();
            self.screen.screen.discard_blank_resize_history(&palette);
        }
        self.top_and_bottom_margins = 0..size.rows as i64;
        self.left_and_right_margins = 0..size.cols;
        self.pixel_height = size.pixel_height;
        self.pixel_width = size.pixel_width;
        self.dpi = size.dpi;
        self.tabs.resize(size.cols);

        if self.screen.alt_screen_is_active {
            self.set_cursor_pos(
                &Position::Absolute(adjusted_cursor_alt.x as i64),
                &Position::Absolute(adjusted_cursor_alt.y),
            );

            if let Some(saved) = self.screen.screen.saved_cursor.as_mut() {
                saved.position.x = adjusted_cursor_main.x;
                saved.position.y = adjusted_cursor_main.y;
                saved.position.seqno = self.seqno;
                saved.wrap_next = false;
            }
        } else {
            self.set_cursor_pos(
                &Position::Absolute(adjusted_cursor_main.x as i64),
                &Position::Absolute(adjusted_cursor_main.y),
            );
            if let Some(saved) = self.screen.alt_screen.saved_cursor.as_mut() {
                saved.position.x = adjusted_cursor_alt.x;
                saved.position.y = adjusted_cursor_alt.y;
                saved.position.seqno = self.seqno;
                saved.wrap_next = false;
            }
        }
    }

    pub fn get_size(&self) -> TerminalSize {
        let screen = self.screen();
        TerminalSize {
            dpi: self.dpi,
            pixel_width: self.pixel_width,
            pixel_height: self.pixel_height,
            rows: screen.physical_rows,
            cols: screen.physical_cols,
        }
    }
}
