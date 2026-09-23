use super::*;

impl TerminalState {
    /// Indicates that mode is permanently enabled
    fn decqrm_response_permanent(&mut self, mode: Mode) {
        let (is_dec, number) = match &mode {
            Mode::QueryDecPrivateMode(DecPrivateMode::Code(code)) => (true, code.to_u16().unwrap()),
            Mode::QueryDecPrivateMode(DecPrivateMode::Unspecified(code)) => (true, *code),
            Mode::QueryMode(TerminalMode::Code(code)) => (false, code.to_u16().unwrap()),
            Mode::QueryMode(TerminalMode::Unspecified(code)) => (false, *code),
            _ => unreachable!(),
        };

        let prefix = if is_dec { "?" } else { "" };

        write!(self.writer, "\x1b[{prefix}{number};3$y").ok();
        self.writer.flush().ok();
    }

    fn decqrm_response(&mut self, mode: Mode, mut recognized: bool, enabled: bool) {
        let (is_dec, number) = match &mode {
            Mode::QueryDecPrivateMode(DecPrivateMode::Code(code)) => (true, code.to_u16().unwrap()),
            Mode::QueryDecPrivateMode(DecPrivateMode::Unspecified(code)) => {
                recognized = false;
                (true, *code)
            }
            Mode::QueryMode(TerminalMode::Code(code)) => (false, code.to_u16().unwrap()),
            Mode::QueryMode(TerminalMode::Unspecified(code)) => {
                recognized = false;
                (false, *code)
            }
            _ => unreachable!(),
        };

        let prefix = if is_dec { "?" } else { "" };

        let status = if recognized {
            if enabled {
                1 // set
            } else {
                2 // reset
            }
        } else {
            0
        };

        log::trace!("{:?} -> recognized={} status={}", mode, recognized, status);
        write!(self.writer, "\x1b[{}{};{}$y", prefix, number, status).ok();
        self.writer.flush().ok();
    }

    pub(crate) fn perform_csi_mode(&mut self, mode: Mode) {
        match mode {
            Mode::SetDecPrivateMode(DecPrivateMode::Code(
                DecPrivateModeCode::StartBlinkingCursor,
            ))
            | Mode::ResetDecPrivateMode(DecPrivateMode::Code(
                DecPrivateModeCode::StartBlinkingCursor,
            )) => {}
            Mode::QueryDecPrivateMode(DecPrivateMode::Code(
                DecPrivateModeCode::StartBlinkingCursor,
            )) => {
                self.decqrm_response(mode, true, false);
            }

            Mode::SetDecPrivateMode(DecPrivateMode::Code(DecPrivateModeCode::AutoRepeat))
            | Mode::ResetDecPrivateMode(DecPrivateMode::Code(DecPrivateModeCode::AutoRepeat)) => {
                // We leave key repeat to the GUI layer prefs
            }

            Mode::SetDecPrivateMode(DecPrivateMode::Code(DecPrivateModeCode::Win32InputMode)) => {
                log::info!("diag: app negotiated win32-input-mode (DECSET 9001)");
                self.keyboard_encoding = KeyboardEncoding::Win32;
            }

            Mode::ResetDecPrivateMode(DecPrivateMode::Code(DecPrivateModeCode::Win32InputMode)) => {
                log::info!("diag: app dropped win32-input-mode (DECRST 9001)");
                self.keyboard_encoding = KeyboardEncoding::Xterm;
            }

            Mode::QueryDecPrivateMode(DecPrivateMode::Code(DecPrivateModeCode::Win32InputMode)) => {
                self.decqrm_response(
                    mode,
                    true,
                    self.keyboard_encoding == KeyboardEncoding::Win32,
                );
            }

            Mode::SetDecPrivateMode(DecPrivateMode::Code(
                DecPrivateModeCode::ReverseWraparound,
            )) => {
                self.reverse_wraparound_mode = true;
            }

            Mode::ResetDecPrivateMode(DecPrivateMode::Code(
                DecPrivateModeCode::ReverseWraparound,
            )) => {
                self.reverse_wraparound_mode = false;
            }

            Mode::QueryDecPrivateMode(DecPrivateMode::Code(
                DecPrivateModeCode::ReverseWraparound,
            )) => {
                self.decqrm_response(mode, true, self.reverse_wraparound_mode);
            }

            Mode::SetDecPrivateMode(DecPrivateMode::Code(
                DecPrivateModeCode::LeftRightMarginMode,
            )) => {
                self.left_and_right_margin_mode = true;
            }

            Mode::ResetDecPrivateMode(DecPrivateMode::Code(
                DecPrivateModeCode::LeftRightMarginMode,
            )) => {
                self.left_and_right_margin_mode = false;
                self.left_and_right_margins = 0..self.screen().physical_cols;
            }

            Mode::QueryDecPrivateMode(DecPrivateMode::Code(
                DecPrivateModeCode::LeftRightMarginMode,
            )) => {
                self.decqrm_response(mode, true, self.left_and_right_margin_mode);
            }

            Mode::SetDecPrivateMode(DecPrivateMode::Code(
                DecPrivateModeCode::GraphemeClustering,
            ))
            | Mode::ResetDecPrivateMode(DecPrivateMode::Code(
                DecPrivateModeCode::GraphemeClustering,
            )) => {
                // Permanently enabled
            }

            Mode::QueryDecPrivateMode(DecPrivateMode::Code(
                DecPrivateModeCode::GraphemeClustering,
            )) => {
                self.decqrm_response_permanent(mode);
            }

            Mode::SetDecPrivateMode(DecPrivateMode::Code(DecPrivateModeCode::SaveCursor)) => {
                self.dec_save_cursor();
            }

            Mode::ResetDecPrivateMode(DecPrivateMode::Code(DecPrivateModeCode::SaveCursor)) => {
                self.dec_restore_cursor();
            }

            Mode::SetDecPrivateMode(DecPrivateMode::Code(DecPrivateModeCode::AutoWrap)) => {
                self.dec_auto_wrap = true;
            }

            Mode::ResetDecPrivateMode(DecPrivateMode::Code(DecPrivateModeCode::AutoWrap)) => {
                self.dec_auto_wrap = false;
            }

            Mode::QueryDecPrivateMode(DecPrivateMode::Code(DecPrivateModeCode::AutoWrap)) => {
                self.decqrm_response(mode, true, self.dec_auto_wrap);
            }

            Mode::SetDecPrivateMode(DecPrivateMode::Code(DecPrivateModeCode::OriginMode)) => {
                self.dec_origin_mode = true;
                self.set_cursor_pos(&Position::Absolute(0), &Position::Absolute(0));
            }

            Mode::ResetDecPrivateMode(DecPrivateMode::Code(DecPrivateModeCode::OriginMode)) => {
                self.dec_origin_mode = false;
                self.set_cursor_pos(&Position::Absolute(0), &Position::Absolute(0));
            }

            Mode::QueryDecPrivateMode(DecPrivateMode::Code(DecPrivateModeCode::OriginMode)) => {
                self.decqrm_response(mode, true, self.dec_origin_mode);
            }

            Mode::SetDecPrivateMode(DecPrivateMode::Code(
                DecPrivateModeCode::UsePrivateColorRegistersForEachGraphic,
            )) => {
                self.use_private_color_registers_for_each_graphic = true;
            }
            Mode::ResetDecPrivateMode(DecPrivateMode::Code(
                DecPrivateModeCode::UsePrivateColorRegistersForEachGraphic,
            )) => {
                self.use_private_color_registers_for_each_graphic = false;
            }
            Mode::QueryDecPrivateMode(DecPrivateMode::Code(
                DecPrivateModeCode::UsePrivateColorRegistersForEachGraphic,
            )) => {
                self.decqrm_response(
                    mode,
                    true,
                    self.use_private_color_registers_for_each_graphic,
                );
            }

            Mode::SetDecPrivateMode(DecPrivateMode::Code(
                DecPrivateModeCode::SynchronizedOutput,
            )) => {
                // This is handled in onlyterm's mux
            }
            Mode::ResetDecPrivateMode(DecPrivateMode::Code(
                DecPrivateModeCode::SynchronizedOutput,
            )) => {
                // This is handled in onlyterm's mux
            }
            Mode::QueryDecPrivateMode(DecPrivateMode::Code(
                DecPrivateModeCode::SynchronizedOutput,
            )) => {
                // This is handled in onlyterm's mux; if we get here, then it isn't enabled,
                // so we always report false
                self.decqrm_response(mode, true, false);
            }

            Mode::SetDecPrivateMode(DecPrivateMode::Code(DecPrivateModeCode::SmoothScroll))
            | Mode::ResetDecPrivateMode(DecPrivateMode::Code(DecPrivateModeCode::SmoothScroll)) => {
                // We always output at our "best" rate
            }

            Mode::SetDecPrivateMode(DecPrivateMode::Code(DecPrivateModeCode::ReverseVideo)) => {
                // Turn on reverse video for all of the lines on the
                // display.
                self.reverse_video_mode = true;
            }

            Mode::ResetDecPrivateMode(DecPrivateMode::Code(DecPrivateModeCode::ReverseVideo)) => {
                // Turn off reverse video for all of the lines on the
                // display.
                self.reverse_video_mode = false;
            }

            Mode::SetDecPrivateMode(DecPrivateMode::Code(DecPrivateModeCode::Select132Columns))
            | Mode::ResetDecPrivateMode(DecPrivateMode::Code(
                DecPrivateModeCode::Select132Columns,
            )) => {
                // Note: we don't support 132 column mode so we treat
                // both set/reset as the same and we're really just here
                // for the other side effects of this sequence
                // https://vt100.net/docs/vt510-rm/DECCOLM.html

                self.top_and_bottom_margins = 0..self.screen().physical_rows as i64;
                self.left_and_right_margins = 0..self.screen().physical_cols;
                self.set_cursor_pos(&Position::Absolute(0), &Position::Absolute(0));
                self.erase_in_display(EraseInDisplay::EraseDisplay);
            }
            Mode::QueryDecPrivateMode(DecPrivateMode::Code(
                DecPrivateModeCode::Select132Columns,
            )) => {
                self.decqrm_response(mode, true, false);
            }

            Mode::SetMode(TerminalMode::Code(TerminalModeCode::BiDirectionalSupportMode)) => {
                self.bidi_enabled.replace(true);
            }
            Mode::ResetMode(TerminalMode::Code(TerminalModeCode::BiDirectionalSupportMode)) => {
                self.bidi_enabled.replace(false);
            }
            Mode::QueryMode(TerminalMode::Code(TerminalModeCode::BiDirectionalSupportMode)) => {
                self.decqrm_response(
                    mode,
                    true,
                    self.bidi_enabled
                        .unwrap_or_else(|| self.config.bidi_mode().enabled),
                );
            }

            Mode::SetMode(TerminalMode::Code(TerminalModeCode::Insert)) => {
                self.insert = true;
            }
            Mode::ResetMode(TerminalMode::Code(TerminalModeCode::Insert)) => {
                self.insert = false;
            }
            Mode::QueryMode(TerminalMode::Code(TerminalModeCode::Insert)) => {
                self.decqrm_response(mode, true, self.insert);
            }

            Mode::SetMode(TerminalMode::Code(TerminalModeCode::AutomaticNewline)) => {
                self.newline_mode = true;
            }
            Mode::ResetMode(TerminalMode::Code(TerminalModeCode::AutomaticNewline)) => {
                self.newline_mode = false;
            }
            Mode::QueryMode(TerminalMode::Code(TerminalModeCode::AutomaticNewline)) => {
                self.decqrm_response(mode, true, self.newline_mode);
            }

            Mode::SetDecPrivateMode(DecPrivateMode::Code(DecPrivateModeCode::BracketedPaste)) => {
                self.bracketed_paste = true;
            }
            Mode::ResetDecPrivateMode(DecPrivateMode::Code(DecPrivateModeCode::BracketedPaste)) => {
                self.bracketed_paste = false;
            }
            Mode::QueryDecPrivateMode(DecPrivateMode::Code(DecPrivateModeCode::BracketedPaste)) => {
                self.decqrm_response(mode, true, self.bracketed_paste);
            }

            Mode::SetDecPrivateMode(DecPrivateMode::Code(
                DecPrivateModeCode::OptEnableAlternateScreen,
            ))
            | Mode::SetDecPrivateMode(DecPrivateMode::Code(
                DecPrivateModeCode::EnableAlternateScreen,
            )) => {
                if !self.screen.is_alt_screen_active() {
                    self.screen.activate_alt_screen(self.seqno);
                    self.pen = CellAttributes::default();
                }
            }
            Mode::ResetDecPrivateMode(DecPrivateMode::Code(
                DecPrivateModeCode::OptEnableAlternateScreen,
            )) => {
                if self.screen.is_alt_screen_active() {
                    self.pen = CellAttributes::default();
                    self.erase_in_display(EraseInDisplay::EraseDisplay);
                    self.screen.activate_primary_screen(self.seqno);
                }
            }

            Mode::ResetDecPrivateMode(DecPrivateMode::Code(
                DecPrivateModeCode::EnableAlternateScreen,
            )) => {
                if self.screen.is_alt_screen_active() {
                    self.screen.activate_primary_screen(self.seqno);
                    self.pen = CellAttributes::default();
                }
            }

            Mode::SetDecPrivateMode(DecPrivateMode::Code(
                DecPrivateModeCode::ApplicationCursorKeys,
            )) => {
                self.application_cursor_keys = true;
            }
            Mode::ResetDecPrivateMode(DecPrivateMode::Code(
                DecPrivateModeCode::ApplicationCursorKeys,
            )) => {
                self.application_cursor_keys = false;
            }
            Mode::QueryDecPrivateMode(DecPrivateMode::Code(
                DecPrivateModeCode::ApplicationCursorKeys,
            )) => {
                self.decqrm_response(mode, true, self.application_cursor_keys);
            }

            Mode::SetDecPrivateMode(DecPrivateMode::Code(DecPrivateModeCode::SixelDisplayMode)) => {
                self.sixel_display_mode = true;
            }
            Mode::ResetDecPrivateMode(DecPrivateMode::Code(
                DecPrivateModeCode::SixelDisplayMode,
            )) => {
                self.sixel_display_mode = false;
            }
            Mode::QueryDecPrivateMode(DecPrivateMode::Code(
                DecPrivateModeCode::SixelDisplayMode,
            )) => {
                self.decqrm_response(mode, true, self.sixel_display_mode);
            }

            Mode::SetDecPrivateMode(DecPrivateMode::Code(DecPrivateModeCode::DecAnsiMode)) => {
                self.dec_ansi_mode = true;
            }
            Mode::ResetDecPrivateMode(DecPrivateMode::Code(DecPrivateModeCode::DecAnsiMode)) => {
                self.dec_ansi_mode = false;
            }
            Mode::QueryDecPrivateMode(DecPrivateMode::Code(DecPrivateModeCode::DecAnsiMode)) => {
                self.decqrm_response(mode, true, self.dec_ansi_mode);
            }

            Mode::SetDecPrivateMode(DecPrivateMode::Code(DecPrivateModeCode::ShowCursor)) => {
                self.cursor_visible = true;
            }
            Mode::ResetDecPrivateMode(DecPrivateMode::Code(DecPrivateModeCode::ShowCursor)) => {
                self.cursor_visible = false;
            }
            Mode::QueryDecPrivateMode(DecPrivateMode::Code(DecPrivateModeCode::ShowCursor)) => {
                self.decqrm_response(mode, true, self.cursor_visible);
            }
            Mode::SetMode(TerminalMode::Code(TerminalModeCode::ShowCursor)) => {
                self.cursor_visible = true;
            }
            Mode::ResetMode(TerminalMode::Code(TerminalModeCode::ShowCursor)) => {
                self.cursor_visible = false;
            }

            Mode::SetDecPrivateMode(DecPrivateMode::Code(DecPrivateModeCode::MouseTracking)) => {
                self.mouse_tracking = true;
                self.last_mouse_move.take();
            }
            Mode::ResetDecPrivateMode(DecPrivateMode::Code(DecPrivateModeCode::MouseTracking)) => {
                self.mouse_tracking = false;
                self.last_mouse_move.take();
            }
            Mode::QueryDecPrivateMode(DecPrivateMode::Code(DecPrivateModeCode::MouseTracking)) => {
                self.decqrm_response(mode, true, self.mouse_tracking);
            }

            Mode::SetDecPrivateMode(DecPrivateMode::Code(
                DecPrivateModeCode::HighlightMouseTracking,
            ))
            | Mode::ResetDecPrivateMode(DecPrivateMode::Code(
                DecPrivateModeCode::HighlightMouseTracking,
            )) => {}

            Mode::SetDecPrivateMode(DecPrivateMode::Code(DecPrivateModeCode::ButtonEventMouse)) => {
                self.button_event_mouse = true;
                self.last_mouse_move.take();
            }
            Mode::ResetDecPrivateMode(DecPrivateMode::Code(
                DecPrivateModeCode::ButtonEventMouse,
            )) => {
                self.button_event_mouse = false;
                self.last_mouse_move.take();
            }
            Mode::QueryDecPrivateMode(DecPrivateMode::Code(
                DecPrivateModeCode::ButtonEventMouse,
            )) => {
                self.decqrm_response(mode, true, self.button_event_mouse);
            }

            Mode::SetDecPrivateMode(DecPrivateMode::Code(DecPrivateModeCode::AnyEventMouse)) => {
                self.any_event_mouse = true;
                self.last_mouse_move.take();
            }
            Mode::ResetDecPrivateMode(DecPrivateMode::Code(DecPrivateModeCode::AnyEventMouse)) => {
                self.any_event_mouse = false;
                self.last_mouse_move.take();
            }
            Mode::QueryDecPrivateMode(DecPrivateMode::Code(DecPrivateModeCode::AnyEventMouse)) => {
                self.decqrm_response(mode, true, self.any_event_mouse);
            }

            Mode::SetDecPrivateMode(DecPrivateMode::Code(DecPrivateModeCode::FocusTracking)) => {
                self.focus_tracking = true;
                self.last_mouse_move.take();
            }
            Mode::ResetDecPrivateMode(DecPrivateMode::Code(DecPrivateModeCode::FocusTracking)) => {
                self.focus_tracking = false;
                self.last_mouse_move.take();
            }
            Mode::QueryDecPrivateMode(DecPrivateMode::Code(DecPrivateModeCode::FocusTracking)) => {
                self.decqrm_response(mode, true, self.focus_tracking);
            }

            Mode::SetDecPrivateMode(DecPrivateMode::Code(DecPrivateModeCode::SGRMouse)) => {
                self.mouse_encoding = MouseEncoding::SGR;
                self.last_mouse_move.take();
            }
            Mode::ResetDecPrivateMode(DecPrivateMode::Code(DecPrivateModeCode::SGRMouse)) => {
                self.mouse_encoding = MouseEncoding::X10;
                self.last_mouse_move.take();
            }
            Mode::QueryDecPrivateMode(DecPrivateMode::Code(DecPrivateModeCode::SGRMouse)) => {
                self.decqrm_response(
                    mode,
                    true,
                    matches!(self.mouse_encoding, MouseEncoding::SGR),
                );
            }
            Mode::SetDecPrivateMode(DecPrivateMode::Code(DecPrivateModeCode::SGRPixelsMouse)) => {
                self.mouse_encoding = MouseEncoding::SgrPixels;
                self.last_mouse_move.take();
            }
            Mode::ResetDecPrivateMode(DecPrivateMode::Code(DecPrivateModeCode::SGRPixelsMouse)) => {
                self.mouse_encoding = MouseEncoding::X10;
                self.last_mouse_move.take();
            }
            Mode::QueryDecPrivateMode(DecPrivateMode::Code(DecPrivateModeCode::SGRPixelsMouse)) => {
                self.decqrm_response(
                    mode,
                    true,
                    matches!(self.mouse_encoding, MouseEncoding::SgrPixels),
                );
            }

            Mode::SetDecPrivateMode(DecPrivateMode::Code(DecPrivateModeCode::Utf8Mouse)) => {
                self.mouse_encoding = MouseEncoding::Utf8;
                self.last_mouse_move.take();
            }
            Mode::ResetDecPrivateMode(DecPrivateMode::Code(DecPrivateModeCode::Utf8Mouse)) => {
                self.mouse_encoding = MouseEncoding::X10;
                self.last_mouse_move.take();
            }
            Mode::QueryDecPrivateMode(DecPrivateMode::Code(DecPrivateModeCode::Utf8Mouse)) => {
                self.decqrm_response(
                    mode,
                    true,
                    matches!(self.mouse_encoding, MouseEncoding::Utf8),
                );
            }

            Mode::SetDecPrivateMode(DecPrivateMode::Code(
                DecPrivateModeCode::SixelScrollsRight,
            )) => {
                self.sixel_scrolls_right = true;
            }
            Mode::ResetDecPrivateMode(DecPrivateMode::Code(
                DecPrivateModeCode::SixelScrollsRight,
            )) => {
                self.sixel_scrolls_right = false;
            }
            Mode::QueryDecPrivateMode(DecPrivateMode::Code(
                DecPrivateModeCode::SixelScrollsRight,
            )) => {
                self.decqrm_response(mode, true, self.sixel_scrolls_right);
            }

            Mode::SetDecPrivateMode(DecPrivateMode::Code(
                DecPrivateModeCode::ClearAndEnableAlternateScreen,
            )) => {
                if !self.screen.is_alt_screen_active() {
                    self.dec_save_cursor();
                    self.screen.activate_alt_screen(self.seqno);
                    self.set_cursor_pos(&Position::Absolute(0), &Position::Absolute(0));
                    self.pen = CellAttributes::default();
                    self.erase_in_display(EraseInDisplay::EraseDisplay);
                }
            }
            Mode::ResetDecPrivateMode(DecPrivateMode::Code(
                DecPrivateModeCode::ClearAndEnableAlternateScreen,
            )) => {
                if self.screen.is_alt_screen_active() {
                    self.screen.activate_primary_screen(self.seqno);
                    self.dec_restore_cursor();
                }
            }
            Mode::SaveDecPrivateMode(DecPrivateMode::Code(n))
            | Mode::RestoreDecPrivateMode(DecPrivateMode::Code(n)) => {
                log::warn!("save/restore dec mode {:?} unimplemented", n)
            }

            Mode::SetDecPrivateMode(DecPrivateMode::Code(
                DecPrivateModeCode::MinTTYApplicationEscapeKeyMode,
            ))
            | Mode::ResetDecPrivateMode(DecPrivateMode::Code(
                DecPrivateModeCode::MinTTYApplicationEscapeKeyMode,
            )) => {}

            Mode::SetDecPrivateMode(DecPrivateMode::Code(
                DecPrivateModeCode::XTermMetaSendsEscape,
            ))
            | Mode::ResetDecPrivateMode(DecPrivateMode::Code(
                DecPrivateModeCode::XTermMetaSendsEscape,
            )) => {}

            Mode::SetDecPrivateMode(DecPrivateMode::Code(
                DecPrivateModeCode::XTermAltSendsEscape,
            ))
            | Mode::ResetDecPrivateMode(DecPrivateMode::Code(
                DecPrivateModeCode::XTermAltSendsEscape,
            )) => {}

            Mode::SetDecPrivateMode(DecPrivateMode::Unspecified(_))
            | Mode::ResetDecPrivateMode(DecPrivateMode::Unspecified(_))
            | Mode::SaveDecPrivateMode(DecPrivateMode::Unspecified(_))
            | Mode::RestoreDecPrivateMode(DecPrivateMode::Unspecified(_)) => {
                if self.config.log_unknown_escape_sequences() {
                    log::warn!("unhandled DecPrivateMode {:?}", mode);
                }
            }

            mode @ Mode::SetMode(_) | mode @ Mode::ResetMode(_) => {
                if self.config.log_unknown_escape_sequences() {
                    log::warn!("unhandled {:?}", mode);
                }
            }

            Mode::XtermKeyMode {
                resource: XtermKeyModifierResource::OtherKeys,
                value,
            } => {
                self.modify_other_keys = match value {
                    Some(0) => None,
                    _ => value,
                };
                log::debug!("XtermKeyMode OtherKeys -> {:?}", self.modify_other_keys);
            }

            Mode::XtermKeyMode { resource, value } => {
                if self.config.log_unknown_escape_sequences() {
                    log::warn!("unhandled XtermKeyMode {:?} {:?}", resource, value);
                }
            }

            Mode::QueryDecPrivateMode(_) | Mode::QueryMode(_) => {
                self.decqrm_response(mode, false, false);
            }
        }
    }
}
