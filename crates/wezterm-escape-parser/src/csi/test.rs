    use super::*;
    use std::io::Write;

    fn parse(control: char, params: &[i64], expected: &str) -> Vec<CSI> {
        let mut cparams = vec![];
        for &p in params {
            if !cparams.is_empty() {
                cparams.push(CsiParam::P(b';'));
            }
            cparams.push(CsiParam::Integer(p));
        }
        let res = CSI::parse(&cparams, false, control).collect();
        assert_eq!(encode(&res), expected, "parsed -> {res:?}");
        res
    }

    fn encode(seq: &Vec<CSI>) -> String {
        let mut res = Vec::new();
        for s in seq {
            write!(res, "{}", s).unwrap();
        }
        String::from_utf8(res).unwrap()
    }

    #[test]
    fn basic() {
        assert_eq!(parse('m', &[], "\x1b[0m"), vec![CSI::Sgr(Sgr::Reset)]);
        assert_eq!(parse('m', &[0], "\x1b[0m"), vec![CSI::Sgr(Sgr::Reset)]);
        assert_eq!(
            parse('m', &[1], "\x1b[1m"),
            vec![CSI::Sgr(Sgr::Intensity(Intensity::Bold))]
        );
        assert_eq!(
            parse('m', &[1, 3], "\x1b[1m\x1b[3m"),
            vec![
                CSI::Sgr(Sgr::Intensity(Intensity::Bold)),
                CSI::Sgr(Sgr::Italic(true)),
            ]
        );

        // Verify that we propagate Unspecified for codes
        // that we don't recognize.
        assert_eq!(
            parse('m', &[1, 3, 1231231], "\x1b[1m\x1b[3m\x1b[1231231m"),
            vec![
                CSI::Sgr(Sgr::Intensity(Intensity::Bold)),
                CSI::Sgr(Sgr::Italic(true)),
                CSI::Unspecified(Box::new(Unspecified {
                    params: [CsiParam::Integer(1231231)].to_vec(),
                    parameters_truncated: false,
                    control: 'm',
                })),
            ]
        );
        assert_eq!(
            parse('m', &[1, 1231231, 3], "\x1b[1m\x1b[1231231;3m"),
            vec![
                CSI::Sgr(Sgr::Intensity(Intensity::Bold)),
                CSI::Unspecified(Box::new(Unspecified {
                    params: [
                        CsiParam::Integer(1231231),
                        CsiParam::P(b';'),
                        CsiParam::Integer(3)
                    ]
                    .to_vec(),
                    parameters_truncated: false,
                    control: 'm',
                })),
            ]
        );
        assert_eq!(
            parse('m', &[1231231, 3], "\x1b[1231231;3m"),
            vec![CSI::Unspecified(Box::new(Unspecified {
                params: [
                    CsiParam::Integer(1231231),
                    CsiParam::P(b';'),
                    CsiParam::Integer(3)
                ]
                .to_vec(),
                parameters_truncated: false,
                control: 'm',
            }))]
        );
    }

    #[test]
    fn blinks() {
        assert_eq!(
            parse('m', &[5], "\x1b[5m"),
            vec![CSI::Sgr(Sgr::Blink(Blink::Slow))]
        );
        assert_eq!(
            parse('m', &[6], "\x1b[6m"),
            vec![CSI::Sgr(Sgr::Blink(Blink::Rapid))]
        );
        assert_eq!(
            parse('m', &[25], "\x1b[25m"),
            vec![CSI::Sgr(Sgr::Blink(Blink::None))]
        );
    }

    #[test]
    fn underlines() {
        assert_eq!(
            parse('m', &[21], "\x1b[21m"),
            vec![CSI::Sgr(Sgr::Underline(Underline::Double))]
        );
        assert_eq!(
            parse('m', &[4], "\x1b[4m"),
            vec![CSI::Sgr(Sgr::Underline(Underline::Single))]
        );
    }

    #[test]
    fn underline_color() {
        assert_eq!(
            parse('m', &[58, 2], "\x1b[58;2m"),
            vec![CSI::Unspecified(Box::new(Unspecified {
                params: [
                    CsiParam::Integer(58),
                    CsiParam::P(b';'),
                    CsiParam::Integer(2)
                ]
                .to_vec(),
                parameters_truncated: false,
                control: 'm',
            }))]
        );

        assert_eq!(
            parse('m', &[58, 2, 255, 255, 255], "\x1b[58:2::255:255:255m"),
            vec![CSI::Sgr(Sgr::UnderlineColor(ColorSpec::TrueColor(
                (255, 255, 255).into(),
            )))]
        );
        assert_eq!(
            parse('m', &[58, 5, 220, 255, 255], "\x1b[58;5;220m\x1b[255;255m"),
            vec![
                CSI::Sgr(Sgr::UnderlineColor(ColorSpec::PaletteIndex(220))),
                CSI::Unspecified(Box::new(Unspecified {
                    params: [
                        CsiParam::Integer(255),
                        CsiParam::P(b';'),
                        CsiParam::Integer(255)
                    ]
                    .to_vec(),
                    parameters_truncated: false,
                    control: 'm',
                })),
            ]
        );
    }

    #[test]
    fn color() {
        assert_eq!(
            parse('m', &[38, 2], "\x1b[38;2m"),
            vec![CSI::Unspecified(Box::new(Unspecified {
                params: [
                    CsiParam::Integer(38),
                    CsiParam::P(b';'),
                    CsiParam::Integer(2)
                ]
                .to_vec(),
                parameters_truncated: false,
                control: 'm',
            }))]
        );

        assert_eq!(
            parse('m', &[38, 2, 255, 255, 255], "\x1b[38:2::255:255:255m"),
            vec![CSI::Sgr(Sgr::Foreground(ColorSpec::TrueColor(
                (255, 255, 255).into(),
            )))]
        );
        assert_eq!(
            parse('m', &[38, 5, 220, 255, 255], "\x1b[38;5;220m\x1b[255;255m"),
            vec![
                CSI::Sgr(Sgr::Foreground(ColorSpec::PaletteIndex(220))),
                CSI::Unspecified(Box::new(Unspecified {
                    params: [
                        CsiParam::Integer(255),
                        CsiParam::P(b';'),
                        CsiParam::Integer(255)
                    ]
                    .to_vec(),
                    parameters_truncated: false,
                    control: 'm',
                })),
            ]
        );
    }

    #[test]
    fn edit() {
        assert_eq!(
            parse('J', &[], "\x1b[J"),
            vec![CSI::Edit(Edit::EraseInDisplay(
                EraseInDisplay::EraseToEndOfDisplay,
            ))]
        );
        assert_eq!(
            parse('J', &[0], "\x1b[J"),
            vec![CSI::Edit(Edit::EraseInDisplay(
                EraseInDisplay::EraseToEndOfDisplay,
            ))]
        );
        assert_eq!(
            parse('J', &[1], "\x1b[1J"),
            vec![CSI::Edit(Edit::EraseInDisplay(
                EraseInDisplay::EraseToStartOfDisplay,
            ))]
        );
    }

    #[test]
    fn window() {
        assert_eq!(
            parse('t', &[6], "\x1b[6t"),
            vec![CSI::Window(Box::new(Window::LowerWindow))]
        );
        assert_eq!(
            parse('t', &[6, 15, 7], "\x1b[6;15;7t"),
            vec![CSI::Window(Box::new(
                Window::ReportCellSizePixelsResponse {
                    width: Some(7),
                    height: Some(15)
                }
            ))]
        );
    }

    #[test]
    fn cursor() {
        assert_eq!(
            parse('C', &[], "\x1b[C"),
            vec![CSI::Cursor(Cursor::Right(1))]
        );
        // check that 0 is treated as 1
        assert_eq!(
            parse('C', &[0], "\x1b[C"),
            vec![CSI::Cursor(Cursor::Right(1))]
        );
        assert_eq!(
            parse('C', &[1], "\x1b[C"),
            vec![CSI::Cursor(Cursor::Right(1))]
        );
        assert_eq!(
            parse('C', &[4], "\x1b[4C"),
            vec![CSI::Cursor(Cursor::Right(4))]
        );

        // Check permutations of optional parameters
        assert_eq!(
            parse('H', &[], "\x1b[1;1H"),
            vec![CSI::Cursor(Cursor::Position {
                line: OneBased::new(1),
                col: OneBased::new(1)
            })]
        );
        let res: Vec<_> = CSI::parse(&[CsiParam::P(b';')], false, 'H').collect();
        assert_eq!(encode(&res), "\x1b[1;1H");
        assert_eq!(
            res,
            vec![CSI::Cursor(Cursor::Position {
                line: OneBased::new(1),
                col: OneBased::new(1)
            })]
        );
        assert_eq!(
            parse('H', &[2], "\x1b[2;1H"),
            vec![CSI::Cursor(Cursor::Position {
                line: OneBased::new(2),
                col: OneBased::new(1)
            })]
        );
        let res: Vec<_> =
            CSI::parse(&[CsiParam::Integer(2), CsiParam::P(b';')], false, 'H').collect();
        assert_eq!(encode(&res), "\x1b[2;1H");
        assert_eq!(
            res,
            vec![CSI::Cursor(Cursor::Position {
                line: OneBased::new(2),
                col: OneBased::new(1)
            })]
        );
        let res: Vec<_> =
            CSI::parse(&[CsiParam::P(b';'), CsiParam::Integer(2)], false, 'H').collect();
        assert_eq!(encode(&res), "\x1b[1;2H");
        assert_eq!(
            res,
            vec![CSI::Cursor(Cursor::Position {
                line: OneBased::new(1),
                col: OneBased::new(2)
            })]
        );
        assert_eq!(
            parse('H', &[2, 3], "\x1b[2;3H"),
            vec![CSI::Cursor(Cursor::Position {
                line: OneBased::new(2),
                col: OneBased::new(3)
            })]
        );
    }

    #[test]
    fn ansiset() {
        assert_eq!(
            parse('h', &[20], "\x1b[20h"),
            vec![CSI::Mode(Mode::SetMode(TerminalMode::Code(
                TerminalModeCode::AutomaticNewline
            )))]
        );
        assert_eq!(
            parse('l', &[20], "\x1b[20l"),
            vec![CSI::Mode(Mode::ResetMode(TerminalMode::Code(
                TerminalModeCode::AutomaticNewline
            )))]
        );
    }

    #[test]
    fn bidi_modes() {
        assert_eq!(
            parse('h', &[8], "\x1b[8h"),
            vec![CSI::Mode(Mode::SetMode(TerminalMode::Code(
                TerminalModeCode::BiDirectionalSupportMode
            )))]
        );
        assert_eq!(
            parse('l', &[8], "\x1b[8l"),
            vec![CSI::Mode(Mode::ResetMode(TerminalMode::Code(
                TerminalModeCode::BiDirectionalSupportMode
            )))]
        );
    }

    #[test]
    fn mouse() {
        let res: Vec<_> = CSI::parse(
            &[
                CsiParam::P(b'<'),
                CsiParam::Integer(0),
                CsiParam::P(b';'),
                CsiParam::Integer(12),
                CsiParam::P(b';'),
                CsiParam::Integer(300),
            ],
            false,
            'M',
        )
        .collect();
        assert_eq!(encode(&res), "\x1b[<0;12;300M");
        assert_eq!(
            res,
            vec![CSI::Mouse(MouseReport::SGR1006 {
                x: 12,
                y: 300,
                button: MouseButton::Button1Press,
                modifiers: Modifiers::NONE,
            })]
        );
    }

    #[test]
    fn soft_reset() {
        let res: Vec<_> = CSI::parse(&[CsiParam::P(b'!')], false, 'p').collect();
        assert_eq!(encode(&res), "\x1b[!p");
        assert_eq!(res, vec![CSI::Device(Box::new(Device::SoftReset))],);
    }

    #[test]
    fn device_attr() {
        let res: Vec<_> = CSI::parse(
            &[
                CsiParam::P(b'?'),
                CsiParam::Integer(63),
                CsiParam::P(b';'),
                CsiParam::Integer(1),
                CsiParam::P(b';'),
                CsiParam::Integer(2),
                CsiParam::P(b';'),
                CsiParam::Integer(4),
                CsiParam::P(b';'),
                CsiParam::Integer(6),
                CsiParam::P(b';'),
                CsiParam::Integer(9),
                CsiParam::P(b';'),
                CsiParam::Integer(15),
                CsiParam::P(b';'),
                CsiParam::Integer(22),
            ],
            false,
            'c',
        )
        .collect();

        assert_eq!(
            res,
            vec![CSI::Device(Box::new(Device::DeviceAttributes(
                DeviceAttributes::Vt320(DeviceAttributeFlags::new(vec![
                    DeviceAttribute::Code(DeviceAttributeCodes::Columns132),
                    DeviceAttribute::Code(DeviceAttributeCodes::Printer),
                    DeviceAttribute::Code(DeviceAttributeCodes::SixelGraphics),
                    DeviceAttribute::Code(DeviceAttributeCodes::SelectiveErase),
                    DeviceAttribute::Code(DeviceAttributeCodes::NationalReplacementCharsets),
                    DeviceAttribute::Code(DeviceAttributeCodes::TechnicalCharacters),
                    DeviceAttribute::Code(DeviceAttributeCodes::AnsiColor),
                ])),
            )))]
        );
        assert_eq!(encode(&res), "\x1b[?63;1;2;4;6;9;15;22c");
    }
