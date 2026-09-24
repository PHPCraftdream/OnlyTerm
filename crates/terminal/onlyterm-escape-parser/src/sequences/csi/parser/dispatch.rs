use super::*;

impl<'a> CSIParser<'a> {
    pub(super) fn parse_next(&mut self, params: &'a [CsiParam]) -> Result<CSI, ()> {
        match (self.control, self.orig_params) {
            ('k', [.., CsiParam::P(b' ')]) => self.select_character_path(params),
            ('q', [.., CsiParam::P(b' ')]) => self.cursor_style(params),
            ('y', [.., CsiParam::P(b'*')]) => self.checksum_area(params),

            ('c', [CsiParam::P(b'='), ..]) => self
                .req_tertiary_device_attributes(params)
                .map(|dev| CSI::Device(Box::new(dev))),
            ('c', [CsiParam::P(b'>'), ..]) => self
                .req_secondary_device_attributes(params)
                .map(|dev| CSI::Device(Box::new(dev))),

            ('m', [CsiParam::P(b'<'), ..]) | ('M', [CsiParam::P(b'<'), ..]) => {
                self.mouse_sgr1006(params).map(CSI::Mouse)
            }

            ('c', [CsiParam::P(b'?'), ..]) => self
                .secondary_device_attributes(params)
                .map(|dev| CSI::Device(Box::new(dev))),

            ('S', [CsiParam::P(b'?'), ..]) => XtSmGraphics::parse(params),
            ('p', [CsiParam::Integer(_), CsiParam::P(b'$')])
            | ('p', [CsiParam::P(b'?'), CsiParam::Integer(_), CsiParam::P(b'$')]) => {
                self.decrqm(params)
            }
            ('h', [CsiParam::P(b'?'), ..]) => self
                .dec(self.focus(params, 1, 0))
                .map(|mode| CSI::Mode(Mode::SetDecPrivateMode(mode))),
            ('l', [CsiParam::P(b'?'), ..]) => self
                .dec(self.focus(params, 1, 0))
                .map(|mode| CSI::Mode(Mode::ResetDecPrivateMode(mode))),
            ('r', [CsiParam::P(b'?'), ..]) => self
                .dec(self.focus(params, 1, 0))
                .map(|mode| CSI::Mode(Mode::RestoreDecPrivateMode(mode))),
            ('q', [CsiParam::P(b'>'), ..]) => self
                .req_terminal_name_and_version(params)
                .map(|dev| CSI::Device(Box::new(dev))),
            ('s', [CsiParam::P(b'?'), ..]) => self
                .dec(self.focus(params, 1, 0))
                .map(|mode| CSI::Mode(Mode::SaveDecPrivateMode(mode))),
            ('m', [CsiParam::P(b'>'), ..]) => self.xterm_key_modifier(params),

            ('p', [CsiParam::P(b'!')]) => Ok(CSI::Device(Box::new(Device::SoftReset))),
            ('u', [CsiParam::P(b'='), CsiParam::Integer(flags)]) => {
                Ok(CSI::Keyboard(Keyboard::SetKittyState {
                    flags: KittyKeyboardFlags::from_bits_truncate(
                        (*flags).try_into().map_err(|_| ())?,
                    ),
                    mode: KittyKeyboardMode::AssignAll,
                }))
            }
            (
                'u',
                [
                    CsiParam::P(b'='),
                    CsiParam::Integer(flags),
                    CsiParam::P(b';'),
                    CsiParam::Integer(mode),
                ],
            ) => Ok(CSI::Keyboard(Keyboard::SetKittyState {
                flags: KittyKeyboardFlags::from_bits_truncate((*flags).try_into().map_err(|_| ())?),
                mode: match *mode {
                    1 => KittyKeyboardMode::AssignAll,
                    2 => KittyKeyboardMode::SetSpecified,
                    3 => KittyKeyboardMode::ClearSpecified,
                    _ => return Err(()),
                },
            })),
            ('u', [CsiParam::P(b'>')]) => Ok(CSI::Keyboard(Keyboard::PushKittyState {
                flags: KittyKeyboardFlags::NONE,
                mode: KittyKeyboardMode::AssignAll,
            })),
            ('u', [CsiParam::P(b'>'), CsiParam::Integer(flags)]) => {
                Ok(CSI::Keyboard(Keyboard::PushKittyState {
                    flags: KittyKeyboardFlags::from_bits_truncate(
                        (*flags).try_into().map_err(|_| ())?,
                    ),
                    mode: KittyKeyboardMode::AssignAll,
                }))
            }
            (
                'u',
                [
                    CsiParam::P(b'>'),
                    CsiParam::Integer(flags),
                    CsiParam::P(b';'),
                    CsiParam::Integer(mode),
                ],
            ) => Ok(CSI::Keyboard(Keyboard::PushKittyState {
                flags: KittyKeyboardFlags::from_bits_truncate((*flags).try_into().map_err(|_| ())?),
                mode: match *mode {
                    1 => KittyKeyboardMode::AssignAll,
                    2 => KittyKeyboardMode::SetSpecified,
                    3 => KittyKeyboardMode::ClearSpecified,
                    _ => return Err(()),
                },
            })),
            ('u', [CsiParam::P(b'?')]) => Ok(CSI::Keyboard(Keyboard::QueryKittySupport)),
            ('u', [CsiParam::P(b'?'), CsiParam::Integer(flags)]) => {
                Ok(CSI::Keyboard(Keyboard::ReportKittyState(
                    KittyKeyboardFlags::from_bits_truncate((*flags).try_into().map_err(|_| ())?),
                )))
            }
            ('u', [CsiParam::P(b'<'), CsiParam::Integer(how_many)]) => Ok(CSI::Keyboard(
                Keyboard::PopKittyState((*how_many).try_into().map_err(|_| ())?),
            )),
            ('u', [CsiParam::P(b'<')]) => Ok(CSI::Keyboard(Keyboard::PopKittyState(1))),

            _ => match self.control {
                'c' => self
                    .req_primary_device_attributes(params)
                    .map(|dev| CSI::Device(Box::new(dev))),

                '@' => parse!(Edit, InsertCharacter, params),
                '`' => parse!(Cursor, CharacterPositionAbsolute, params),
                'A' => parse!(Cursor, Up, params),
                'B' => parse!(Cursor, Down, params),
                'C' => parse!(Cursor, Right, params),
                'D' => parse!(Cursor, Left, params),
                'E' => parse!(Cursor, NextLine, params),
                'F' => parse!(Cursor, PrecedingLine, params),
                'G' => parse!(Cursor, CharacterAbsolute, params),
                'H' => parse!(Cursor, Position, line, col, params),
                'I' => parse!(Cursor, ForwardTabulation, params),
                'J' => parse!(Edit, EraseInDisplay, params),
                'K' => parse!(Edit, EraseInLine, params),
                'L' => parse!(Edit, InsertLine, params),
                'M' => parse!(Edit, DeleteLine, params),
                'P' => parse!(Edit, DeleteCharacter, params),
                'R' => parse!(Cursor, ActivePositionReport, line, col, params),
                'S' => parse!(Edit, ScrollUp, params),
                'T' => parse!(Edit, ScrollDown, params),
                'W' => parse!(Cursor, TabulationControl, params),
                'X' => parse!(Edit, EraseCharacter, params),
                'Y' => parse!(Cursor, LineTabulation, params),
                'Z' => parse!(Cursor, BackwardTabulation, params),

                'a' => parse!(Cursor, CharacterPositionForward, params),
                'b' => parse!(Edit, Repeat, params),
                'd' => parse!(Cursor, LinePositionAbsolute, params),
                'e' => parse!(Cursor, LinePositionForward, params),
                'f' => parse!(Cursor, CharacterAndLinePosition, line, col, params),
                'g' => parse!(Cursor, TabulationClear, params),
                'h' => self
                    .terminal_mode(params)
                    .map(|mode| CSI::Mode(Mode::SetMode(mode))),
                'j' => parse!(Cursor, CharacterPositionBackward, params),
                'k' => parse!(Cursor, LinePositionBackward, params),
                'l' => self
                    .terminal_mode(params)
                    .map(|mode| CSI::Mode(Mode::ResetMode(mode))),

                'm' => self.sgr(params).map(CSI::Sgr),
                'n' => self.dsr(params),
                'r' => self.decstbm(params),
                's' => self.decslrm(params),
                't' => self.window(params).map(|p| CSI::Window(Box::new(p))),
                'u' => noparams!(Cursor, RestoreCursor, params),
                'x' => self
                    .req_terminal_parameters(params)
                    .map(|dev| CSI::Device(Box::new(dev))),

                _ => Err(()),
            },
        }
    }

    /// Consume some number of elements from params and update it.
    /// Take care to avoid setting params back to an empty slice
    /// as this would trigger returning a default value and/or
    /// an unterminated parse loop.
    pub(super) fn advance_by<T>(&mut self, n: usize, params: &'a [CsiParam], result: T) -> T {
        let n = if matches!(params.get(n), Some(CsiParam::P(b';'))) {
            n + 1
        } else {
            n
        };

        let (_, next) = params.split_at(n);
        if !next.is_empty() {
            self.params = Some(next);
        }
        result
    }

    pub(super) fn focus(
        &self,
        params: &'a [CsiParam],
        from_start: usize,
        from_end: usize,
    ) -> &'a [CsiParam] {
        if params == self.orig_params {
            let len = params.len();
            &params[from_start..len - from_end]
        } else {
            params
        }
    }
}
