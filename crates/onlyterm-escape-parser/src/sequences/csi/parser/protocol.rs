use super::*;

impl<'a> CSIParser<'a> {
    pub(super) fn select_character_path(&mut self, params: &'a [CsiParam]) -> Result<CSI, ()> {
        fn path(n: i64) -> Result<CharacterPath, ()> {
            Ok(match n {
                0 => CharacterPath::ImplementationDefault,
                1 => CharacterPath::LeftToRightOrTopToBottom,
                2 => CharacterPath::RightToLeftOrBottomToTop,
                _ => return Err(()),
            })
        }

        match params {
            [CsiParam::P(b' ')] => Ok(self.advance_by(
                1,
                params,
                CSI::SelectCharacterPath(CharacterPath::ImplementationDefault, 0),
            )),
            [CsiParam::Integer(a), CsiParam::P(b' ')] => {
                Ok(self.advance_by(2, params, CSI::SelectCharacterPath(path(*a)?, 0)))
            }
            [
                CsiParam::Integer(a),
                CsiParam::P(b';'),
                CsiParam::Integer(b),
                CsiParam::P(b' '),
            ] => Ok(self.advance_by(4, params, CSI::SelectCharacterPath(path(*a)?, *b))),
            _ => Err(()),
        }
    }

    pub(super) fn cursor_style(&mut self, params: &'a [CsiParam]) -> Result<CSI, ()> {
        match params {
            [CsiParam::Integer(p), CsiParam::P(b' ')] => match FromPrimitive::from_i64(*p) {
                None => Err(()),
                Some(style) => {
                    Ok(self.advance_by(2, params, CSI::Cursor(Cursor::CursorStyle(style))))
                }
            },
            _ => Err(()),
        }
    }

    pub(super) fn checksum_area(&mut self, params: &'a [CsiParam]) -> Result<CSI, ()> {
        let params = Cracked::parse(&params[..params.len() - 1])?;

        let request_id = params.int(0)?;
        let page_number = params.int(1)?;
        let top = OneBased::from_optional_esc_param(params.get(2))?;
        let left = OneBased::from_optional_esc_param(params.get(3))?;
        let bottom = OneBased::from_optional_esc_param(params.get(4))?;
        let right = OneBased::from_optional_esc_param(params.get(5))?;
        Ok(CSI::Window(Box::new(Window::ChecksumRectangularArea {
            request_id,
            page_number,
            top,
            left,
            bottom,
            right,
        })))
    }

    pub(super) fn dsr(&mut self, params: &'a [CsiParam]) -> Result<CSI, ()> {
        match params {
            [CsiParam::Integer(5)] => {
                Ok(self.advance_by(1, params, CSI::Device(Box::new(Device::StatusReport))))
            }

            [CsiParam::Integer(6)] => {
                Ok(self.advance_by(1, params, CSI::Cursor(Cursor::RequestActivePositionReport)))
            }
            _ => Err(()),
        }
    }

    pub(super) fn decstbm(&mut self, params: &'a [CsiParam]) -> Result<CSI, ()> {
        match params {
            [] => Ok(CSI::Cursor(Cursor::SetTopAndBottomMargins {
                top: OneBased::new(1),
                bottom: OneBased::new(u32::MAX),
            })),
            [p] => Ok(self.advance_by(
                1,
                params,
                CSI::Cursor(Cursor::SetTopAndBottomMargins {
                    top: OneBased::from_esc_param(p)?,
                    bottom: OneBased::new(u32::MAX),
                }),
            )),
            [a, CsiParam::P(b';'), b] => Ok(self.advance_by(
                3,
                params,
                CSI::Cursor(Cursor::SetTopAndBottomMargins {
                    top: OneBased::from_esc_param(a)?,
                    bottom: OneBased::from_esc_param_with_big_default(b)?,
                }),
            )),
            [CsiParam::P(b';'), b] => Ok(self.advance_by(
                2,
                params,
                CSI::Cursor(Cursor::SetTopAndBottomMargins {
                    top: OneBased::new(1),
                    bottom: OneBased::from_esc_param_with_big_default(b)?,
                }),
            )),
            _ => Err(()),
        }
    }

    pub(super) fn xterm_key_modifier(&mut self, params: &'a [CsiParam]) -> Result<CSI, ()> {
        match params {
            [CsiParam::P(b'>'), a, CsiParam::P(b';'), b] => {
                let resource =
                    XtermKeyModifierResource::parse(a.as_integer().ok_or(())?).ok_or(())?;
                Ok(self.advance_by(
                    4,
                    params,
                    CSI::Mode(Mode::XtermKeyMode {
                        resource,
                        value: Some(b.as_integer().ok_or(())?),
                    }),
                ))
            }
            [CsiParam::P(b'>'), a, CsiParam::P(b';')] => {
                let resource =
                    XtermKeyModifierResource::parse(a.as_integer().ok_or(())?).ok_or(())?;
                Ok(self.advance_by(
                    3,
                    params,
                    CSI::Mode(Mode::XtermKeyMode {
                        resource,
                        value: None,
                    }),
                ))
            }
            [CsiParam::P(b'>'), p] => {
                let resource =
                    XtermKeyModifierResource::parse(p.as_integer().ok_or(())?).ok_or(())?;
                Ok(self.advance_by(
                    2,
                    params,
                    CSI::Mode(Mode::XtermKeyMode {
                        resource,
                        value: None,
                    }),
                ))
            }
            _ => Err(()),
        }
    }

    pub(super) fn decslrm(&mut self, params: &'a [CsiParam]) -> Result<CSI, ()> {
        match params {
            [] => {
                // with no params this is a request to save the cursor
                // and is technically in conflict with SetLeftAndRightMargins.
                // The emulator needs to decide based on DECSLRM mode
                // whether this saves the cursor or is SetLeftAndRightMargins
                // with default parameters!
                Ok(CSI::Cursor(Cursor::SaveCursor))
            }
            [p] => Ok(self.advance_by(
                1,
                params,
                CSI::Cursor(Cursor::SetLeftAndRightMargins {
                    left: OneBased::from_esc_param(p)?,
                    right: OneBased::new(u32::MAX),
                }),
            )),
            [a, CsiParam::P(b';'), b] => Ok(self.advance_by(
                3,
                params,
                CSI::Cursor(Cursor::SetLeftAndRightMargins {
                    left: OneBased::from_esc_param(a)?,
                    right: OneBased::from_esc_param(b)?,
                }),
            )),
            [CsiParam::P(b';'), b] => Ok(self.advance_by(
                2,
                params,
                CSI::Cursor(Cursor::SetLeftAndRightMargins {
                    left: OneBased::new(1),
                    right: OneBased::from_esc_param(b)?,
                }),
            )),
            _ => Err(()),
        }
    }

    pub(super) fn req_primary_device_attributes(
        &mut self,
        params: &'a [CsiParam],
    ) -> Result<Device, ()> {
        match params {
            [] => Ok(Device::RequestPrimaryDeviceAttributes),
            [CsiParam::Integer(0)] => {
                Ok(self.advance_by(1, params, Device::RequestPrimaryDeviceAttributes))
            }
            _ => Err(()),
        }
    }

    pub(super) fn req_terminal_name_and_version(
        &mut self,
        params: &'a [CsiParam],
    ) -> Result<Device, ()> {
        match params {
            [_] => Ok(Device::RequestTerminalNameAndVersion),

            [_, CsiParam::Integer(0)] => {
                Ok(self.advance_by(2, params, Device::RequestTerminalNameAndVersion))
            }
            _ => Err(()),
        }
    }

    pub(super) fn req_secondary_device_attributes(
        &mut self,
        params: &'a [CsiParam],
    ) -> Result<Device, ()> {
        match params {
            [CsiParam::P(b'>')] => Ok(Device::RequestSecondaryDeviceAttributes),
            [CsiParam::P(b'>'), CsiParam::Integer(0)] => {
                Ok(self.advance_by(2, params, Device::RequestSecondaryDeviceAttributes))
            }
            _ => Err(()),
        }
    }

    pub(super) fn req_tertiary_device_attributes(
        &mut self,
        params: &'a [CsiParam],
    ) -> Result<Device, ()> {
        match params {
            [CsiParam::P(b'=')] => Ok(Device::RequestTertiaryDeviceAttributes),
            [CsiParam::P(b'='), CsiParam::Integer(0)] => {
                Ok(self.advance_by(2, params, Device::RequestTertiaryDeviceAttributes))
            }
            _ => Err(()),
        }
    }

    pub(super) fn secondary_device_attributes(
        &mut self,
        params: &'a [CsiParam],
    ) -> Result<Device, ()> {
        match params {
            [
                _,
                CsiParam::Integer(1),
                CsiParam::P(b';'),
                CsiParam::Integer(0),
            ] => Ok(self.advance_by(
                4,
                params,
                Device::DeviceAttributes(DeviceAttributes::Vt101WithNoOptions),
            )),
            [_, CsiParam::Integer(6)] => {
                Ok(self.advance_by(2, params, Device::DeviceAttributes(DeviceAttributes::Vt102)))
            }
            [
                _,
                CsiParam::Integer(1),
                CsiParam::P(b';'),
                CsiParam::Integer(2),
            ] => Ok(self.advance_by(
                4,
                params,
                Device::DeviceAttributes(DeviceAttributes::Vt100WithAdvancedVideoOption),
            )),
            [_, CsiParam::Integer(62), ..] => Ok(self.advance_by(
                params.len(),
                params,
                Device::DeviceAttributes(DeviceAttributes::Vt220(
                    DeviceAttributeFlags::from_params(&params[2..]),
                )),
            )),
            [_, CsiParam::Integer(63), ..] => Ok(self.advance_by(
                params.len(),
                params,
                Device::DeviceAttributes(DeviceAttributes::Vt320(
                    DeviceAttributeFlags::from_params(&params[2..]),
                )),
            )),
            [_, CsiParam::Integer(64), ..] => Ok(self.advance_by(
                params.len(),
                params,
                Device::DeviceAttributes(DeviceAttributes::Vt420(
                    DeviceAttributeFlags::from_params(&params[2..]),
                )),
            )),
            _ => Err(()),
        }
    }

    pub(super) fn req_terminal_parameters(&mut self, params: &'a [CsiParam]) -> Result<Device, ()> {
        match params {
            [] | [CsiParam::Integer(0)] => Ok(Device::RequestTerminalParameters(0)),
            [CsiParam::Integer(1)] => Ok(Device::RequestTerminalParameters(1)),
            _ => Err(()),
        }
    }

    /// Parse extended mouse reports known as SGR 1006 mode
    pub(super) fn mouse_sgr1006(&mut self, params: &'a [CsiParam]) -> Result<MouseReport, ()> {
        let (p0, p1, p2) = match params {
            [
                CsiParam::P(b'<'),
                CsiParam::Integer(p0),
                CsiParam::P(b';'),
                CsiParam::Integer(p1),
                CsiParam::P(b';'),
                CsiParam::Integer(p2),
            ] => (*p0, *p1, *p2),
            _ => return Err(()),
        };

        // 'M' encodes a press, 'm' a release.
        let button = match (self.control, p0 & 0b110_0011) {
            ('M', 0) => MouseButton::Button1Press,
            ('m', 0) => MouseButton::Button1Release,
            ('M', 1) => MouseButton::Button2Press,
            ('m', 1) => MouseButton::Button2Release,
            ('M', 2) => MouseButton::Button3Press,
            ('m', 2) => MouseButton::Button3Release,
            ('M', 64) => MouseButton::Button4Press,
            ('m', 64) => MouseButton::Button4Release,
            ('M', 65) => MouseButton::Button5Press,
            ('m', 65) => MouseButton::Button5Release,
            ('M', 66) => MouseButton::Button6Press,
            ('m', 66) => MouseButton::Button6Release,
            ('M', 67) => MouseButton::Button7Press,
            ('m', 67) => MouseButton::Button7Release,
            ('M', 32) => MouseButton::Button1Drag,
            ('M', 33) => MouseButton::Button2Drag,
            ('M', 34) => MouseButton::Button3Drag,
            // Note that there is some theoretical ambiguity with these None values.
            // The ambiguity stems from alternative encodings of the mouse protocol;
            // when set to SGR1006 mode the variants with the `3` parameter do not
            // occur.  They included here as a reminder for when support for those
            // other encodings is added and this block is likely copied and pasted
            // or refactored for re-use with them.
            ('M', 35) => MouseButton::None, // mouse motion with no buttons
            ('m', 35) => MouseButton::None, // mouse motion with no buttons (in Windows Terminal)
            ('M', 3) => MouseButton::None,  // legacy notification about button release
            ('m', 3) => MouseButton::None,  // release+press doesn't make sense
            _ => {
                return Err(());
            }
        };

        let mut modifiers = Modifiers::NONE;
        if p0 & 4 != 0 {
            modifiers |= Modifiers::SHIFT;
        }
        if p0 & 8 != 0 {
            modifiers |= Modifiers::ALT;
        }
        if p0 & 16 != 0 {
            modifiers |= Modifiers::CTRL;
        }

        Ok(self.advance_by(
            6,
            params,
            MouseReport::SGR1006 {
                x: p1 as u16,
                y: p2 as u16,
                button,
                modifiers,
            },
        ))
    }

    pub(super) fn decrqm(&mut self, params: &'a [CsiParam]) -> Result<CSI, ()> {
        Ok(CSI::Mode(match params {
            [CsiParam::Integer(p), CsiParam::P(b'$')] => {
                Mode::QueryMode(match FromPrimitive::from_i64(*p) {
                    None => TerminalMode::Unspecified(p.to_u16().ok_or(())?),
                    Some(mode) => TerminalMode::Code(mode),
                })
            }
            [CsiParam::P(b'?'), CsiParam::Integer(p), CsiParam::P(b'$')] => {
                Mode::QueryDecPrivateMode(match FromPrimitive::from_i64(*p) {
                    None => DecPrivateMode::Unspecified(p.to_u16().ok_or(())?),
                    Some(mode) => DecPrivateMode::Code(mode),
                })
            }
            _ => return Err(()),
        }))
    }

    pub(super) fn dec(&mut self, params: &'a [CsiParam]) -> Result<DecPrivateMode, ()> {
        match params {
            [CsiParam::Integer(p0), ..] => match FromPrimitive::from_i64(*p0) {
                None => Ok(self.advance_by(
                    1,
                    params,
                    DecPrivateMode::Unspecified(p0.to_u16().ok_or(())?),
                )),
                Some(mode) => Ok(self.advance_by(1, params, DecPrivateMode::Code(mode))),
            },
            _ => Err(()),
        }
    }

    pub(super) fn terminal_mode(&mut self, params: &'a [CsiParam]) -> Result<TerminalMode, ()> {
        let p0 = params.first().and_then(CsiParam::as_integer).ok_or(())?;
        match FromPrimitive::from_i64(p0) {
            None => {
                Ok(self.advance_by(1, params, TerminalMode::Unspecified(p0.to_u16().ok_or(())?)))
            }
            Some(mode) => Ok(self.advance_by(1, params, TerminalMode::Code(mode))),
        }
    }
}
