use super::*;

impl TerminalState {
    fn checksum_rectangle(&mut self, left: u32, top: u32, right: u32, bottom: u32) -> u16 {
        let y_origin = if self.dec_origin_mode {
            self.top_and_bottom_margins.start
        } else {
            0
        } as u32;
        let x_origin = if self.dec_origin_mode {
            self.left_and_right_margins.start
        } else {
            0
        };
        let screen = self.screen_mut();
        let mut checksum = 0;
        /*
        debug!(
            "checksum left={} top={} right={} bottom={}",
            left as usize + x_origin,
            top + y_origin,
            right as usize + x_origin,
            bottom + y_origin
        );
        */

        for y in top..=bottom {
            let line_idx = screen.phys_row(VisibleRowIndex::from(y_origin + y));
            let line = screen.line_mut(line_idx);
            for cell in line.visible_cells().skip(x_origin + left as usize) {
                if cell.cell_index() > x_origin + right as usize {
                    break;
                }

                let ch = cell.str().chars().next().unwrap() as u32;
                // debug!("y={} col={} ch={:x} cell={:?}", y + y_origin, col, ch, cell);

                checksum += u16::from(ch as u8);
            }
        }

        // Treat uninitialized cells as spaces.
        // The concept of uninitialized cells in onlyterm is not the same as that on VT520 or that
        // on xterm, so, to prevent a lot of noise in esctest, treat them as spaces, at least when
        // asking for the checksum of a single cell (which is what esctest does).
        // See: https://github.com/wezterm/wezterm/pull/4565
        if checksum == 0 {
            32u16
        } else {
            checksum
        }
    }

    pub(crate) fn perform_csi_window(&mut self, window: Window) {
        match window {
            Window::ReportTextAreaSizeCells => {
                let screen = self.screen();
                let height = Some(screen.physical_rows as i64);
                let width = Some(screen.physical_cols as i64);

                let response = Box::new(Window::ResizeWindowCells { width, height });
                write!(self.writer, "{}", CSI::Window(response)).ok();
                self.writer.flush().ok();
            }

            Window::ReportCellSizePixels => {
                let screen = self.screen();
                let height = screen.physical_rows;
                let width = screen.physical_cols;
                let response = Box::new(Window::ReportCellSizePixelsResponse {
                    width: Some((self.pixel_width / width) as i64),
                    height: Some((self.pixel_height / height) as i64),
                });
                write!(self.writer, "{}", CSI::Window(response)).ok();
                self.writer.flush().ok();
            }

            Window::ReportTextAreaSizePixels => {
                let response = Box::new(Window::ResizeWindowPixels {
                    width: Some(self.pixel_width as i64),
                    height: Some(self.pixel_height as i64),
                });
                write!(self.writer, "{}", CSI::Window(response)).ok();
                self.writer.flush().ok();
            }

            Window::ReportWindowTitle => {
                if self.config.enable_title_reporting() {
                    write!(
                        self.writer,
                        "{}",
                        OperatingSystemCommand::SetWindowTitleSun(self.title.clone())
                    )
                    .ok();
                    self.writer.flush().ok();
                }
            }

            Window::ChecksumRectangularArea {
                request_id,
                top,
                left,
                bottom,
                right,
                ..
            } => {
                if self.config.enable_checksum_rectangular_area() {
                    let checksum = self.checksum_rectangle(
                        left.as_zero_based(),
                        top.as_zero_based(),
                        right.as_zero_based(),
                        bottom.as_zero_based(),
                    );
                    write!(self.writer, "\x1bP{}!~{:04x}\x1b\\", request_id, checksum).ok();
                    self.writer.flush().ok();
                }
            }
            Window::ResizeWindowCells { .. } => {
                // We don't allow the application to change the window size; that's
                // up to the user!
            }
            Window::Iconify | Window::DeIconify => {}
            Window::PopIconAndWindowTitle
            | Window::PopWindowTitle
            | Window::PopIconTitle
            | Window::PushIconAndWindowTitle
            | Window::PushIconTitle
            | Window::PushWindowTitle => {}

            _ => {
                if self.config.log_unknown_escape_sequences() {
                    log::warn!("unhandled Window CSI {:?}", window);
                }
            }
        }
    }
}
