// The range_plus_one lint can't see when the LHS is not compatible with
// and inclusive range
#![allow(clippy::range_plus_one)]
use super::*;
use num_traits::ToPrimitive;
use std::io::Write;
use terminfo::Value;
use wezterm_escape_parser::csi::{
    Cursor, DecPrivateMode, DecPrivateModeCode, Device, Edit, EraseInDisplay, EraseInLine, Mode,
    Sgr, TerminalMode, TerminalModeCode, Window, XtSmGraphics, XtSmGraphicsAction,
    XtSmGraphicsItem, XtSmGraphicsStatus, XtermKeyModifierResource,
};
use wezterm_escape_parser::OneBased;

impl TerminalState {
    /// <https://invisible-island.net/xterm/ctlseqs/ctlseqs.html#h4-Device-Control-functions:DCS-plus-q-Pt-ST.F95>
    /// XTGETTCAP
    pub(super) fn xt_get_tcap(&mut self, names: Vec<String>) {
        let mut res = String::new();

        for name in &names {
            res.push_str("\x1bP");

            let encoded_name = hex::encode_upper(name);
            match name.as_str() {
                "TN" | "name" => {
                    res.push_str("1+r");
                    res.push_str(&encoded_name);
                    res.push('=');

                    let encoded_val = hex::encode_upper(&self.term_program);
                    res.push_str(&encoded_val);
                }

                "Co" | "colors" => {
                    res.push_str("1+r");
                    res.push_str(&encoded_name);
                    res.push('=');
                    let encoded_val = hex::encode_upper("256");
                    res.push_str(&encoded_val);
                }

                "RGB" => {
                    res.push_str("1+r");
                    res.push_str(&encoded_name);
                    res.push('=');
                    let encoded_val = hex::encode_upper("8/8/8");
                    res.push_str(&encoded_val);
                }

                _ => {
                    if let Some(value) = DB.raw(name) {
                        res.push_str("1+r");
                        res.push_str(&encoded_name);
                        res.push('=');
                        let value = match value {
                            Value::True => hex::encode_upper("1"),
                            Value::Number(n) => hex::encode_upper(n.to_string()),
                            Value::String(s) => hex::encode_upper(s),
                        };
                        res.push_str(&value);
                    } else {
                        log::trace!("xt_get_tcap: unknown name {}", name);
                        res.push_str("0+r");
                        res.push_str(&encoded_name);
                    }
                }
            }
            res.push_str("\x1b\\");
        }

        log::trace!(
            "XTGETTCAP {:?} responding with {}",
            names,
            res.escape_debug()
        );
        self.writer.write_all(res.as_bytes()).ok();
        self.writer.flush().ok();
    }

    pub(super) fn perform_device(&mut self, dev: Device) {
        match dev {
            Device::DeviceAttributes(a) => {
                if self.config.log_unknown_escape_sequences() {
                    log::warn!("unhandled: {:?}", a);
                }
            }
            Device::SoftReset => {
                // TODO: see https://vt100.net/docs/vt510-rm/DECSTR.html
                self.pen = CellAttributes::default();
                self.insert = false;
                self.dec_origin_mode = false;
                // Note that xterm deviates from the documented DECSTR
                // setting for dec_auto_wrap, so we do too
                self.dec_auto_wrap = true;
                self.application_cursor_keys = false;
                self.modify_other_keys = None;
                self.application_keypad = false;
                self.top_and_bottom_margins = 0..self.screen().physical_rows as i64;
                self.left_and_right_margins = 0..self.screen().physical_cols;
                self.left_and_right_margin_mode = false;
                self.screen.activate_alt_screen(self.seqno);
                self.screen.saved_cursor().take();
                self.screen.activate_primary_screen(self.seqno);
                self.screen.saved_cursor().take();
                self.kitty_remove_all_placements(true);

                self.reverse_wraparound_mode = false;
                self.reverse_video_mode = false;
                self.bidi_enabled.take();
                self.bidi_hint.take();

                self.g0_charset = CharSet::Ascii;
                self.g1_charset = CharSet::Ascii;
            }
            Device::RequestPrimaryDeviceAttributes => {
                let mut ident = "\x1b[?65".to_string(); // Vt500
                ident.push_str(";4"); // Sixel graphics
                ident.push_str(";6"); // Selective erase
                ident.push_str(";18"); // windowing extensions
                ident.push_str(";22"); // ANSI color, vt525
                ident.push_str(";52"); // Clipboard access
                ident.push('c');

                self.writer.write_all(ident.as_bytes()).ok();
                self.writer.flush().ok();
            }
            Device::RequestSecondaryDeviceAttributes => {
                // Response is: Pp ; Pv ; Pc
                // Where Pp=1 means vt220
                // and Pv is the firmware version.
                // Pc is always 0.
                // Because our default TERM is xterm, the firmware
                // version will be considered to be equialent to xterm's
                // patch levels, with the following effects:
                // pv < 95 -> ttymouse=xterm
                // pv >= 95 < 277 -> ttymouse=xterm2
                // pv >= 277 -> ttymouse=sgr
                // pv >= 279 - xterm will probe for additional device settings.
                self.writer.write_all(b"\x1b[>1;277;0c").ok();
                self.writer.flush().ok();
            }
            Device::RequestTertiaryDeviceAttributes => {
                self.writer
                    .write_all(format!("\x1bP!|00000000{}", ST).as_bytes())
                    .ok();
                self.writer.flush().ok();
            }
            Device::RequestTerminalNameAndVersion => {
                self.writer.write_all(DCS.as_bytes()).ok();
                self.writer
                    .write_all(
                        format!(">|{} {}{}", self.term_program, self.term_version, ST).as_bytes(),
                    )
                    .ok();
                self.writer.flush().ok();
            }
            Device::RequestTerminalParameters(a) => {
                self.writer
                    .write_all(format!("\x1b[{};1;1;128;128;1;0x", a + 2).as_bytes())
                    .ok();
                self.writer.flush().ok();
            }
            Device::StatusReport => {
                self.writer.write_all(b"\x1b[0n").ok();
                self.writer.flush().ok();
            }
            Device::XtSmGraphics(g) => {
                let response = if matches!(g.item, XtSmGraphicsItem::Unspecified(_)) {
                    XtSmGraphics {
                        item: g.item,
                        action_or_status: XtSmGraphicsStatus::InvalidItem.to_i64(),
                        value: vec![],
                    }
                } else {
                    match g.action() {
                        None | Some(XtSmGraphicsAction::SetToValue) => XtSmGraphics {
                            item: g.item,
                            action_or_status: XtSmGraphicsStatus::InvalidAction.to_i64(),
                            value: vec![],
                        },
                        Some(XtSmGraphicsAction::ResetToDefault) => XtSmGraphics {
                            item: g.item,
                            action_or_status: XtSmGraphicsStatus::Success.to_i64(),
                            value: vec![],
                        },
                        Some(XtSmGraphicsAction::ReadMaximumAllowedValue)
                        | Some(XtSmGraphicsAction::ReadAttribute) => match g.item {
                            XtSmGraphicsItem::Unspecified(_) => unreachable!("checked above"),
                            XtSmGraphicsItem::NumberOfColorRegisters => XtSmGraphics {
                                item: g.item,
                                action_or_status: XtSmGraphicsStatus::Success.to_i64(),
                                value: vec![65536],
                            },
                            XtSmGraphicsItem::RegisGraphicsGeometry
                            | XtSmGraphicsItem::SixelGraphicsGeometry => XtSmGraphics {
                                item: g.item,
                                action_or_status: XtSmGraphicsStatus::Success.to_i64(),
                                value: vec![self.pixel_width as i64, self.pixel_height as i64],
                            },
                        },
                    }
                };

                let dev = Device::XtSmGraphics(response);

                write!(self.writer, "\x1b[{}", dev).ok();
                self.writer.flush().ok();
            }
        }
    }

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

    pub(super) fn perform_csi_mode(&mut self, mode: Mode) {
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
                self.keyboard_encoding = KeyboardEncoding::Win32;
            }

            Mode::ResetDecPrivateMode(DecPrivateMode::Code(DecPrivateModeCode::Win32InputMode)) => {
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
                // This is handled in wezterm's mux
            }
            Mode::ResetDecPrivateMode(DecPrivateMode::Code(
                DecPrivateModeCode::SynchronizedOutput,
            )) => {
                // This is handled in wezterm's mux
            }
            Mode::QueryDecPrivateMode(DecPrivateMode::Code(
                DecPrivateModeCode::SynchronizedOutput,
            )) => {
                // This is handled in wezterm's mux; if we get here, then it isn't enabled,
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
        // The concept of uninitialized cells in wezterm is not the same as that on VT520 or that
        // on xterm, so, to prevent a lot of noise in esctest, treat them as spaces, at least when
        // asking for the checksum of a single cell (which is what esctest does).
        // See: https://github.com/wezterm/wezterm/pull/4565
        if checksum == 0 {
            32u16
        } else {
            checksum
        }
    }

    pub(super) fn perform_csi_window(&mut self, window: Window) {
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

    pub(super) fn perform_csi_edit(&mut self, edit: Edit) {
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
    fn set_left_and_right_margins(&mut self, left: OneBased, right: OneBased) {
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

    pub(super) fn perform_csi_cursor(&mut self, cursor: Cursor) {
        let seqno = self.seqno;
        match cursor {
            Cursor::SetTopAndBottomMargins { top, bottom } => {
                let rows = self.screen().physical_rows;
                let top = i64::from(top.as_zero_based()).min(rows as i64 - 1).max(0);
                let bottom = i64::from(bottom.as_zero_based())
                    .min(rows as i64 - 1)
                    .max(0);
                if top >= bottom {
                    return;
                }
                self.top_and_bottom_margins = top..bottom + 1;
                self.set_cursor_pos(&Position::Absolute(0), &Position::Absolute(0));
                log::debug!(
                    "SetTopAndBottomMargins {:?} (and move cursor to top left: {:?})",
                    self.top_and_bottom_margins,
                    self.cursor
                );
            }

            Cursor::SetLeftAndRightMargins { left, right } => {
                self.set_left_and_right_margins(left, right);
            }

            Cursor::ForwardTabulation(n) => {
                for _ in 0..n {
                    self.c0_horizontal_tab();
                }
            }
            Cursor::BackwardTabulation(n) => {
                for _ in 0..n {
                    let x = self
                        .tabs
                        .find_prev_tab_stop(self.cursor.x)
                        .unwrap_or_default();
                    self.set_cursor_pos(&Position::Absolute(x as i64), &Position::Relative(0));
                }
            }

            Cursor::TabulationClear(to_clear) => {
                self.tabs.clear(
                    to_clear,
                    self.cursor.x,
                    self.config.log_unknown_escape_sequences(),
                );
            }

            Cursor::TabulationControl(_) => {}
            Cursor::LineTabulation(_) => {}

            Cursor::Left(_n) => {
                // https://vt100.net/docs/vt510-rm/CUB.html
                unreachable!("Actually handled in Performer::csi_dispatch by rewriting as ControlCode::Backspace");
            }

            Cursor::Right(n) => {
                // https://vt100.net/docs/vt510-rm/CUF.html
                let cols = self.screen().physical_cols;
                let new_x = if self.cursor.x >= self.left_and_right_margins.end {
                    // outside the margin, so allow movement to screen edge
                    (self.cursor.x + n as usize).min(cols - 1)
                } else {
                    // Else constrain to margin
                    (self.cursor.x + n as usize).min(self.left_and_right_margins.end - 1)
                };

                self.cursor.x = new_x;
                self.cursor.seqno = seqno;
                self.wrap_next = false;
            }

            Cursor::Up(n) => {
                // https://vt100.net/docs/vt510-rm/CUU.html

                let candidate = self.cursor.y.saturating_sub(i64::from(n));
                let new_y = if self.cursor.y < self.top_and_bottom_margins.start {
                    // above the top margin, so allow movement to
                    // top of screen
                    candidate
                } else {
                    // Else constrain to top margin
                    if candidate < self.top_and_bottom_margins.start {
                        self.top_and_bottom_margins.start
                    } else {
                        candidate
                    }
                };

                let new_y = new_y.max(0);

                self.cursor.y = new_y;
                self.cursor.seqno = seqno;
                self.wrap_next = false;
            }
            Cursor::Down(n) => {
                // https://vt100.net/docs/vt510-rm/CUD.html
                let rows = self.screen().physical_rows;
                let new_y = if self.cursor.y >= self.top_and_bottom_margins.end {
                    // below the bottom margin, so allow movement to
                    // bottom of screen
                    (self.cursor.y + i64::from(n)).min(rows as i64 - 1)
                } else {
                    // Else constrain to bottom margin
                    (self.cursor.y + i64::from(n)).min(self.top_and_bottom_margins.end - 1)
                };

                self.cursor.y = new_y;
                self.cursor.seqno = seqno;
                self.wrap_next = false;
            }

            Cursor::CharacterAndLinePosition { line, col } | Cursor::Position { line, col } => self
                .set_cursor_pos(
                    &Position::Absolute(i64::from(col.as_zero_based())),
                    &Position::Absolute(i64::from(line.as_zero_based())),
                ),
            Cursor::CharacterAbsolute(col) => self.set_cursor_pos(
                &Position::Absolute(i64::from(col.as_zero_based())),
                &Position::Relative(0),
            ),

            Cursor::CharacterPositionAbsolute(col) => {
                let col = col.as_zero_based() as usize;
                let col = if self.dec_origin_mode {
                    col + self.left_and_right_margins.start
                } else {
                    col
                };
                self.cursor.x = col.min(self.screen().physical_cols - 1);
                self.cursor.seqno = seqno;
                self.wrap_next = false;
            }

            Cursor::CharacterPositionBackward(col) => self.set_cursor_pos(
                &Position::Relative(-(i64::from(col))),
                &Position::Relative(0),
            ),
            Cursor::CharacterPositionForward(col) => {
                self.set_cursor_pos(&Position::Relative(i64::from(col)), &Position::Relative(0))
            }
            Cursor::LinePositionAbsolute(line) => self.set_cursor_pos(
                &Position::Relative(0),
                &Position::Absolute((i64::from(line)).saturating_sub(1)),
            ),
            Cursor::LinePositionBackward(line) => self.set_cursor_pos(
                &Position::Relative(0),
                &Position::Relative(-(i64::from(line))),
            ),
            Cursor::LinePositionForward(line) => {
                self.set_cursor_pos(&Position::Relative(0), &Position::Relative(i64::from(line)))
            }
            Cursor::NextLine(n) => {
                // https://vt100.net/docs/vt510-rm/CNL.html
                let rows = self.screen().physical_rows;
                let new_y = if self.cursor.y >= self.top_and_bottom_margins.end {
                    // below the bottom margin, so allow movement to
                    // bottom of screen
                    (self.cursor.y + i64::from(n)).min(rows as i64 - 1)
                } else {
                    // Else constrain to bottom margin
                    (self.cursor.y + i64::from(n)).min(self.top_and_bottom_margins.end - 1)
                };

                self.cursor.y = new_y;
                self.cursor.x = self.left_and_right_margins.start;
                self.cursor.seqno = seqno;
                self.wrap_next = false;
            }
            Cursor::PrecedingLine(n) => {
                // https://vt100.net/docs/vt510-rm/CPL.html
                let candidate = self.cursor.y.saturating_sub(i64::from(n));
                let new_y = if self.cursor.y < self.top_and_bottom_margins.start {
                    // above the top margin, so allow movement to
                    // top of screen
                    candidate
                } else {
                    // Else constrain to top margin
                    if candidate < self.top_and_bottom_margins.start {
                        self.top_and_bottom_margins.start
                    } else {
                        candidate
                    }
                };

                let new_y = new_y.max(0);

                self.cursor.y = new_y;
                self.cursor.x = self.left_and_right_margins.start;
                self.cursor.seqno = seqno;
                self.wrap_next = false;
            }

            Cursor::ActivePositionReport { .. } => {
                // This is really a response from the terminal, and
                // we don't need to process it as a terminal command
            }
            Cursor::RequestActivePositionReport => {
                let line = OneBased::from_zero_based(
                    (self.cursor.y.saturating_sub(if self.dec_origin_mode {
                        self.top_and_bottom_margins.start
                    } else {
                        0
                    })) as u32,
                );
                let col = OneBased::from_zero_based(
                    (self
                        .cursor
                        .x
                        .min(self.screen().physical_cols - 1)
                        .saturating_sub(if self.dec_origin_mode {
                            self.left_and_right_margins.start
                        } else {
                            0
                        })) as u32,
                );
                let report = CSI::Cursor(Cursor::ActivePositionReport { line, col });
                write!(self.writer, "{}", report).ok();
                self.writer.flush().ok();
            }
            Cursor::SaveCursor => {
                // The `CSI s` SaveCursor sequence is ambiguous with DECSLRM
                // with default parameters.  To resolve the ambiguity, DECSLRM
                // is recognized if DECLRMM mode is active which we do here
                // where we have the context!
                if self.left_and_right_margin_mode {
                    // https://vt100.net/docs/vt510-rm/DECSLRM.html
                    self.set_left_and_right_margins(
                        OneBased::new(1),
                        OneBased::new(self.screen().physical_cols as u32 + 1),
                    );
                } else {
                    self.dec_save_cursor();
                }
            }
            Cursor::RestoreCursor => self.dec_restore_cursor(),
            Cursor::CursorStyle(style) => {
                self.cursor.shape = match style {
                    CursorStyle::Default => CursorShape::Default,
                    CursorStyle::BlinkingBlock => CursorShape::BlinkingBlock,
                    CursorStyle::SteadyBlock => CursorShape::SteadyBlock,
                    CursorStyle::BlinkingUnderline => CursorShape::BlinkingUnderline,
                    CursorStyle::SteadyUnderline => CursorShape::SteadyUnderline,
                    CursorStyle::BlinkingBar => CursorShape::BlinkingBar,
                    CursorStyle::SteadyBar => CursorShape::SteadyBar,
                };
                log::debug!("Cursor shape is now {:?}", self.cursor.shape);
            }
        }
    }

    /// https://vt100.net/docs/vt510-rm/DECSC.html
    pub(super) fn dec_save_cursor(&mut self) {
        let saved = SavedCursor {
            position: self.cursor,
            wrap_next: self.wrap_next,
            pen: self.pen.clone(),
            dec_origin_mode: self.dec_origin_mode,
            g0_charset: self.g0_charset,
            g1_charset: self.g1_charset,
        };
        debug!(
            "saving cursor {:?} is_alt={}",
            saved,
            self.screen.is_alt_screen_active()
        );
        *self.screen.saved_cursor() = Some(saved);
    }

    /// https://vt100.net/docs/vt510-rm/DECRC.html
    pub(super) fn dec_restore_cursor(&mut self) {
        let saved = self
            .screen
            .saved_cursor()
            .clone()
            .unwrap_or_else(|| SavedCursor {
                position: CursorPosition::default(),
                wrap_next: false,
                pen: Default::default(),
                dec_origin_mode: false,
                g0_charset: CharSet::Ascii,
                g1_charset: CharSet::Ascii,
            });
        debug!(
            "restore cursor {:?} is_alt={}",
            saved,
            self.screen.is_alt_screen_active()
        );
        let x = saved.position.x;
        let y = saved.position.y;
        // Disable origin mode so that we can set the cursor position directly
        self.dec_origin_mode = false;
        self.set_cursor_pos(&Position::Absolute(x as i64), &Position::Absolute(y));
        self.cursor.shape = saved.position.shape;
        self.wrap_next = saved.wrap_next;
        self.pen = saved.pen;
        self.dec_origin_mode = saved.dec_origin_mode;
        self.g0_charset = saved.g0_charset;
        self.g1_charset = saved.g1_charset;
        self.shift_out = false;
        self.newline_mode = false;
    }

    pub(super) fn perform_csi_sgr(&mut self, sgr: Sgr) {
        debug!("{:?}", sgr);
        match sgr {
            Sgr::Reset => {
                let link = self.pen.hyperlink().map(Arc::clone);
                let semantic_type = self.pen.semantic_type();
                self.pen = CellAttributes::default();
                self.pen.set_hyperlink(link);
                self.pen.set_semantic_type(semantic_type);
            }
            Sgr::Intensity(intensity) => {
                self.pen.set_intensity(intensity);
            }
            Sgr::Underline(underline) => {
                self.pen.set_underline(underline);
            }
            Sgr::Overline(overline) => {
                self.pen.set_overline(overline);
            }
            Sgr::VerticalAlign(align) => {
                self.pen.set_vertical_align(align);
            }
            Sgr::Blink(blink) => {
                self.pen.set_blink(blink);
            }
            Sgr::Italic(italic) => {
                self.pen.set_italic(italic);
            }
            Sgr::Inverse(inverse) => {
                self.pen.set_reverse(inverse);
            }
            Sgr::Invisible(invis) => {
                self.pen.set_invisible(invis);
            }
            Sgr::StrikeThrough(strike) => {
                self.pen.set_strikethrough(strike);
            }
            Sgr::Foreground(col) => {
                self.pen.set_foreground(col);
            }
            Sgr::Background(col) => {
                self.pen.set_background(col);
            }
            Sgr::UnderlineColor(col) => {
                self.pen.set_underline_color(col);
            }
            Sgr::Font(_) => {}
        }
    }
}
