use super::*;

impl<'a> CSIParser<'a> {
    pub(super) fn parse_sgr_color(&mut self, params: &'a [CsiParam]) -> Result<ColorSpec, ()> {
        match params {
            // onlyterm extension to support an optional alpha channel in the `:` form only
            [
                _,
                CsiParam::P(b':'),
                CsiParam::Integer(6),
                CsiParam::P(b':'),
                CsiParam::Integer(_colorspace),
                CsiParam::P(b':'),
                red,
                CsiParam::P(b':'),
                green,
                CsiParam::P(b':'),
                blue,
                CsiParam::P(b':'),
                alpha,
                ..,
            ] => {
                let res: SrgbaTuple =
                    (to_u8(red)?, to_u8(green)?, to_u8(blue)?, to_u8(alpha)?).into();
                Ok(self.advance_by(13, params, res.into()))
            }
            [
                _,
                CsiParam::P(b':'),
                CsiParam::Integer(6),
                CsiParam::P(b':'),
                /* empty colorspace */ CsiParam::P(b':'),
                red,
                CsiParam::P(b':'),
                green,
                CsiParam::P(b':'),
                blue,
                CsiParam::P(b':'),
                alpha,
                ..,
            ] => {
                let res: SrgbaTuple =
                    (to_u8(red)?, to_u8(green)?, to_u8(blue)?, to_u8(alpha)?).into();
                Ok(self.advance_by(12, params, res.into()))
            }
            [
                _,
                CsiParam::P(b':'),
                CsiParam::Integer(6),
                CsiParam::P(b':'),
                red,
                CsiParam::P(b':'),
                green,
                CsiParam::P(b':'),
                blue,
                CsiParam::P(b':'),
                alpha,
                ..,
            ] => {
                let res: SrgbaTuple =
                    (to_u8(red)?, to_u8(green)?, to_u8(blue)?, to_u8(alpha)?).into();
                Ok(self.advance_by(11, params, res.into()))
            }

            // standard sgr colors
            [
                _,
                CsiParam::P(b':'),
                CsiParam::Integer(2),
                CsiParam::P(b':'),
                CsiParam::Integer(_colorspace),
                CsiParam::P(b':'),
                red,
                CsiParam::P(b':'),
                green,
                CsiParam::P(b':'),
                blue,
                ..,
            ] => {
                let res = RgbColor::new_8bpc(to_u8(red)?, to_u8(green)?, to_u8(blue)?).into();
                Ok(self.advance_by(11, params, res))
            }

            [
                _,
                CsiParam::P(b':'),
                CsiParam::Integer(2),
                CsiParam::P(b':'),
                /* empty colorspace */ CsiParam::P(b':'),
                red,
                CsiParam::P(b':'),
                green,
                CsiParam::P(b':'),
                blue,
                ..,
            ] => {
                let res = RgbColor::new_8bpc(to_u8(red)?, to_u8(green)?, to_u8(blue)?).into();
                Ok(self.advance_by(10, params, res))
            }

            [
                _,
                CsiParam::P(b';'),
                CsiParam::Integer(2),
                CsiParam::P(b';'),
                red,
                CsiParam::P(b';'),
                green,
                CsiParam::P(b';'),
                blue,
                ..,
            ]
            | [
                _,
                CsiParam::P(b':'),
                CsiParam::Integer(2),
                CsiParam::P(b':'),
                red,
                CsiParam::P(b':'),
                green,
                CsiParam::P(b':'),
                blue,
                ..,
            ] => {
                let res = RgbColor::new_8bpc(to_u8(red)?, to_u8(green)?, to_u8(blue)?).into();
                Ok(self.advance_by(9, params, res))
            }

            [
                _,
                CsiParam::P(b';'),
                CsiParam::Integer(5),
                CsiParam::P(b';'),
                idx,
                ..,
            ]
            | [
                _,
                CsiParam::P(b':'),
                CsiParam::Integer(5),
                CsiParam::P(b':'),
                idx,
                ..,
            ] => Ok(self.advance_by(5, params, ColorSpec::PaletteIndex(to_u8(idx)?))),
            _ => Err(()),
        }
    }

    pub(super) fn underline(&mut self, params: &'a [CsiParam]) -> Result<Sgr, ()> {
        let (sgr, n) = match params {
            [_, CsiParam::P(b':'), CsiParam::Integer(0), ..] => {
                (Sgr::Underline(Underline::None), 3)
            }
            [_, CsiParam::P(b':'), CsiParam::Integer(1), ..] => {
                (Sgr::Underline(Underline::Single), 3)
            }
            [_, CsiParam::P(b':'), CsiParam::Integer(2), ..] => {
                (Sgr::Underline(Underline::Double), 3)
            }
            [_, CsiParam::P(b':'), CsiParam::Integer(3), ..] => {
                (Sgr::Underline(Underline::Curly), 3)
            }
            [_, CsiParam::P(b':'), CsiParam::Integer(4), ..] => {
                (Sgr::Underline(Underline::Dotted), 3)
            }
            [_, CsiParam::P(b':'), CsiParam::Integer(5), ..] => {
                (Sgr::Underline(Underline::Dashed), 3)
            }
            _ => (Sgr::Underline(Underline::Single), 1),
        };

        Ok(self.advance_by(n, params, sgr))
    }

    pub(super) fn sgr(&mut self, params: &'a [CsiParam]) -> Result<Sgr, ()> {
        if params.is_empty() {
            // With no parameters, treat as equivalent to Reset.
            Ok(Sgr::Reset)
        } else {
            for p in params {
                match p {
                    CsiParam::P(b';')
                    | CsiParam::P(b':')
                    | CsiParam::P(b'?')
                    | CsiParam::Integer(_) => {}
                    _ => return Err(()),
                }
            }

            // Consume a single parameter and return the parsed result
            macro_rules! one {
                ($t:expr) => {
                    Ok(self.advance_by(1, params, $t))
                };
            }

            match &params[0] {
                CsiParam::P(b';') => {
                    // Starting with an empty item is equivalent to a reset
                    self.advance_by(1, params, Ok(Sgr::Reset))
                }

                // There are a small number of DEC private SGR parameters that
                // have equivalents in the normal SGR space.
                // We're simply inlining recognizing them here, and mapping them
                // to those SGR equivalents. That makes parsing "lossy" in the
                // sense that the original sequence is lost, but semantically,
                // the result is the same.
                // These codes are taken from the "SGR" section of
                // "Digital ANSI-Compliant Printing Protocol
                // Level 2 Programming Reference Manual"
                // on page 7-78.
                // <https://vaxhaven.com/images/f/f7/EK-PPLV2-PM-B01.pdf>
                /* Withdrawn because xterm introduced a conflict:
                 * <https://github.com/mintty/mintty/issues/1171#issuecomment-1336174469>
                 * <https://github.com/mintty/mintty/issues/1189>
                CsiParam::P(b'?') if params.len() > 1 => match &params[1] {
                    // Consume two parameters and return the parsed result
                    macro_rules! two {
                        ($t:expr) => {
                            Ok(self.advance_by(2, params, $t))
                        };
                    }
                    CsiParam::Integer(i) => match FromPrimitive::from_i64(*i) {
                        None => Err(()),
                        Some(code) => match code {
                            0 => two!(Sgr::Reset),
                            4 => two!(Sgr::VerticalAlign(VerticalAlign::SuperScript)),
                            5 => two!(Sgr::VerticalAlign(VerticalAlign::SubScript)),
                            6 => two!(Sgr::Overline(true)),
                            24 => two!(Sgr::VerticalAlign(VerticalAlign::BaseLine)),
                            26 => two!(Sgr::Overline(false)),
                            _ => Err(()),
                        },
                    },
                    _ => Err(()),
                },
                */
                CsiParam::P(_) => Err(()),
                CsiParam::Integer(i) => match FromPrimitive::from_i64(*i) {
                    None => Err(()),
                    Some(sgr) => match sgr {
                        SgrCode::Reset => one!(Sgr::Reset),
                        SgrCode::IntensityBold => one!(Sgr::Intensity(Intensity::Bold)),
                        SgrCode::IntensityDim => one!(Sgr::Intensity(Intensity::Half)),
                        SgrCode::NormalIntensity => one!(Sgr::Intensity(Intensity::Normal)),
                        SgrCode::UnderlineOn => {
                            self.underline(params) //.map(Sgr::Underline)
                        }
                        SgrCode::UnderlineDouble => one!(Sgr::Underline(Underline::Double)),
                        SgrCode::UnderlineOff => one!(Sgr::Underline(Underline::None)),
                        SgrCode::UnderlineColor => {
                            self.parse_sgr_color(params).map(Sgr::UnderlineColor)
                        }
                        SgrCode::ResetUnderlineColor => {
                            one!(Sgr::UnderlineColor(ColorSpec::default()))
                        }
                        SgrCode::BlinkOn => one!(Sgr::Blink(Blink::Slow)),
                        SgrCode::RapidBlinkOn => one!(Sgr::Blink(Blink::Rapid)),
                        SgrCode::BlinkOff => one!(Sgr::Blink(Blink::None)),
                        SgrCode::ItalicOn => one!(Sgr::Italic(true)),
                        SgrCode::ItalicOff => one!(Sgr::Italic(false)),
                        SgrCode::VerticalAlignSuperScript => {
                            one!(Sgr::VerticalAlign(VerticalAlign::SuperScript))
                        }
                        SgrCode::VerticalAlignSubScript => {
                            one!(Sgr::VerticalAlign(VerticalAlign::SubScript))
                        }
                        SgrCode::VerticalAlignBaseLine => {
                            one!(Sgr::VerticalAlign(VerticalAlign::BaseLine))
                        }
                        SgrCode::ForegroundColor => {
                            self.parse_sgr_color(params).map(Sgr::Foreground)
                        }
                        SgrCode::ForegroundBlack => one!(Sgr::Foreground(AnsiColor::Black.into())),
                        SgrCode::ForegroundRed => one!(Sgr::Foreground(AnsiColor::Maroon.into())),
                        SgrCode::ForegroundGreen => one!(Sgr::Foreground(AnsiColor::Green.into())),
                        SgrCode::ForegroundYellow => one!(Sgr::Foreground(AnsiColor::Olive.into())),
                        SgrCode::ForegroundBlue => one!(Sgr::Foreground(AnsiColor::Navy.into())),
                        SgrCode::ForegroundMagenta => {
                            one!(Sgr::Foreground(AnsiColor::Purple.into()))
                        }
                        SgrCode::ForegroundCyan => one!(Sgr::Foreground(AnsiColor::Teal.into())),
                        SgrCode::ForegroundWhite => one!(Sgr::Foreground(AnsiColor::Silver.into())),
                        SgrCode::ForegroundDefault => one!(Sgr::Foreground(ColorSpec::Default)),
                        SgrCode::ForegroundBrightBlack => {
                            one!(Sgr::Foreground(AnsiColor::Grey.into()))
                        }
                        SgrCode::ForegroundBrightRed => {
                            one!(Sgr::Foreground(AnsiColor::Red.into()))
                        }
                        SgrCode::ForegroundBrightGreen => {
                            one!(Sgr::Foreground(AnsiColor::Lime.into()))
                        }
                        SgrCode::ForegroundBrightYellow => {
                            one!(Sgr::Foreground(AnsiColor::Yellow.into()))
                        }
                        SgrCode::ForegroundBrightBlue => {
                            one!(Sgr::Foreground(AnsiColor::Blue.into()))
                        }
                        SgrCode::ForegroundBrightMagenta => {
                            one!(Sgr::Foreground(AnsiColor::Fuchsia.into()))
                        }
                        SgrCode::ForegroundBrightCyan => {
                            one!(Sgr::Foreground(AnsiColor::Aqua.into()))
                        }
                        SgrCode::ForegroundBrightWhite => {
                            one!(Sgr::Foreground(AnsiColor::White.into()))
                        }

                        SgrCode::BackgroundColor => {
                            self.parse_sgr_color(params).map(Sgr::Background)
                        }
                        SgrCode::BackgroundBlack => one!(Sgr::Background(AnsiColor::Black.into())),
                        SgrCode::BackgroundRed => one!(Sgr::Background(AnsiColor::Maroon.into())),
                        SgrCode::BackgroundGreen => one!(Sgr::Background(AnsiColor::Green.into())),
                        SgrCode::BackgroundYellow => one!(Sgr::Background(AnsiColor::Olive.into())),
                        SgrCode::BackgroundBlue => one!(Sgr::Background(AnsiColor::Navy.into())),
                        SgrCode::BackgroundMagenta => {
                            one!(Sgr::Background(AnsiColor::Purple.into()))
                        }
                        SgrCode::BackgroundCyan => one!(Sgr::Background(AnsiColor::Teal.into())),
                        SgrCode::BackgroundWhite => one!(Sgr::Background(AnsiColor::Silver.into())),
                        SgrCode::BackgroundDefault => one!(Sgr::Background(ColorSpec::Default)),
                        SgrCode::BackgroundBrightBlack => {
                            one!(Sgr::Background(AnsiColor::Grey.into()))
                        }
                        SgrCode::BackgroundBrightRed => {
                            one!(Sgr::Background(AnsiColor::Red.into()))
                        }
                        SgrCode::BackgroundBrightGreen => {
                            one!(Sgr::Background(AnsiColor::Lime.into()))
                        }
                        SgrCode::BackgroundBrightYellow => {
                            one!(Sgr::Background(AnsiColor::Yellow.into()))
                        }
                        SgrCode::BackgroundBrightBlue => {
                            one!(Sgr::Background(AnsiColor::Blue.into()))
                        }
                        SgrCode::BackgroundBrightMagenta => {
                            one!(Sgr::Background(AnsiColor::Fuchsia.into()))
                        }
                        SgrCode::BackgroundBrightCyan => {
                            one!(Sgr::Background(AnsiColor::Aqua.into()))
                        }
                        SgrCode::BackgroundBrightWhite => {
                            one!(Sgr::Background(AnsiColor::White.into()))
                        }

                        SgrCode::InverseOn => one!(Sgr::Inverse(true)),
                        SgrCode::InverseOff => one!(Sgr::Inverse(false)),
                        SgrCode::InvisibleOn => one!(Sgr::Invisible(true)),
                        SgrCode::InvisibleOff => one!(Sgr::Invisible(false)),
                        SgrCode::StrikeThroughOn => one!(Sgr::StrikeThrough(true)),
                        SgrCode::StrikeThroughOff => one!(Sgr::StrikeThrough(false)),
                        SgrCode::OverlineOn => one!(Sgr::Overline(true)),
                        SgrCode::OverlineOff => one!(Sgr::Overline(false)),
                        SgrCode::DefaultFont => one!(Sgr::Font(Font::Default)),
                        SgrCode::AltFont1 => one!(Sgr::Font(Font::Alternate(1))),
                        SgrCode::AltFont2 => one!(Sgr::Font(Font::Alternate(2))),
                        SgrCode::AltFont3 => one!(Sgr::Font(Font::Alternate(3))),
                        SgrCode::AltFont4 => one!(Sgr::Font(Font::Alternate(4))),
                        SgrCode::AltFont5 => one!(Sgr::Font(Font::Alternate(5))),
                        SgrCode::AltFont6 => one!(Sgr::Font(Font::Alternate(6))),
                        SgrCode::AltFont7 => one!(Sgr::Font(Font::Alternate(7))),
                        SgrCode::AltFont8 => one!(Sgr::Font(Font::Alternate(8))),
                        SgrCode::AltFont9 => one!(Sgr::Font(Font::Alternate(9))),
                    },
                },
            }
        }
    }
}
