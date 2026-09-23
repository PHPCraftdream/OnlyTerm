use super::*;

impl TerminalState {
    /// <https://invisible-island.net/xterm/ctlseqs/ctlseqs.html#h4-Device-Control-functions:DCS-plus-q-Pt-ST.F95>
    /// XTGETTCAP
    pub(crate) fn xt_get_tcap(&mut self, names: Vec<String>) {
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

    pub(crate) fn perform_device(&mut self, dev: Device) {
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
}
