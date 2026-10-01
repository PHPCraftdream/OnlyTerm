use super::*;
use crate::color::ColorSpec;
use crate::csi::{
    CharacterPath, DecPrivateMode, DecPrivateModeCode, Device, Intensity, Mode, Sgr, Underline,
    Window, XtSmGraphics, XtSmGraphicsItem, XtermKeyModifierResource,
};
use crate::{EscCode, OneBased};
use k9::assert_equal as assert_eq;
use std::io::Write;

fn encode(seq: &Vec<Action>) -> String {
    let mut res = Vec::new();
    for s in seq {
        write!(res, "{}", s).unwrap();
    }
    String::from_utf8(res).unwrap()
}

// <https://github.com/markbt/streampager/issues/57>
#[test]
fn osc_bel_parse_first_as_vec() {
    let data = b"\x1b]8;;http://example.com\x07example\x1b]8;;\x07";
    let mut p = Parser::new();

    let mut offset = 0;
    let mut actions = vec![];
    while let Some((mut act, off)) = p.parse_first_as_vec(&data[offset..]) {
        actions.append(&mut act);
        offset += off;
    }

    k9::snapshot!(
        actions,
        r#"
[
    OperatingSystemCommand(
        SetHyperlink(
            Some(
                Hyperlink {
                    params: {},
                    uri: "http://example.com",
                    implicit: false,
                },
            ),
        ),
    ),
    Print(
        'e',
    ),
    Print(
        'x',
    ),
    Print(
        'a',
    ),
    Print(
        'm',
    ),
    Print(
        'p',
    ),
    Print(
        'l',
    ),
    Print(
        'e',
    ),
    OperatingSystemCommand(
        SetHyperlink(
            None,
        ),
    ),
]
"#
    );
}

// <https://github.com/markbt/streampager/issues/57>
#[test]
fn osc_st_parse_first_as_vec() {
    // This string includes an assitional trailing ST sequence which should
    // be parsed separately.
    let data = b"\x1b]8;;http://example.com\x1b\\example\x1b]8;;\x1b\\\x1b\\";
    let mut p = Parser::new();

    let mut offset = 0;
    let mut actions = vec![];
    let mut slices = vec![];
    while let Some((act, off)) = p.parse_first_as_vec(&data[offset..]) {
        // Store each vec of actions so we can confirm that the ST sequence is bundled with the
        // OSC SetHyperlink command.
        actions.push(act);
        // Additionally store all non-single-character slices so we can confirm these are split
        // correctly.
        if off > 1 {
            slices.push(&data[offset..offset + off]);
        }
        offset += off;
    }

    assert_eq!(
        slices,
        vec![
            b"\x1b]8;;http://example.com\x1b\\".as_slice(),
            b"\x1b]8;;\x1b\\".as_slice(),
            b"\x1b\\".as_slice()
        ]
    );

    k9::snapshot!(
        actions,
        r#"
[
    [
        OperatingSystemCommand(
            SetHyperlink(
                Some(
                    Hyperlink {
                        params: {},
                        uri: "http://example.com",
                        implicit: false,
                    },
                ),
            ),
        ),
        Esc(
            Code(
                StringTerminator,
            ),
        ),
    ],
    [
        Print(
            'e',
        ),
    ],
    [
        Print(
            'x',
        ),
    ],
    [
        Print(
            'a',
        ),
    ],
    [
        Print(
            'm',
        ),
    ],
    [
        Print(
            'p',
        ),
    ],
    [
        Print(
            'l',
        ),
    ],
    [
        Print(
            'e',
        ),
    ],
    [
        OperatingSystemCommand(
            SetHyperlink(
                None,
            ),
        ),
        Esc(
            Code(
                StringTerminator,
            ),
        ),
    ],
    [
        Esc(
            Code(
                StringTerminator,
            ),
        ),
    ],
]
"#
    );
}

#[test]
fn basic_parse() {
    let mut p = Parser::new();
    let actions = p.parse_as_vec(b"hello");
    assert_eq!(vec![Action::PrintString("hello".to_string())], actions);
    assert_eq!(encode(&actions), "hello");
}

#[test]
fn basic_bold() {
    let mut p = Parser::new();
    let actions = p.parse_as_vec(b"\x1b[1mb");
    assert_eq!(
        vec![
            Action::CSI(CSI::Sgr(Sgr::Intensity(Intensity::Bold))),
            Action::Print('b'),
        ],
        actions
    );
    assert_eq!(encode(&actions), "\x1b[1mb");
}

#[test]
fn basic_bold_italic() {
    let mut p = Parser::new();
    let actions = p.parse_as_vec(b"\x1b[1;3mb");
    assert_eq!(
        vec![
            Action::CSI(CSI::Sgr(Sgr::Intensity(Intensity::Bold))),
            Action::CSI(CSI::Sgr(Sgr::Italic(true))),
            Action::Print('b'),
        ],
        actions
    );

    assert_eq!(encode(&actions), "\x1b[1m\x1b[3mb");
}

#[test]
fn fancy_underline() {
    let mut p = Parser::new();

    let actions = p.parse_as_vec(b"\x1b[4:0;4:1;4:2;4:3;4:4;4:5mb");
    assert_eq!(
        vec![
            Action::CSI(CSI::Sgr(Sgr::Underline(Underline::None))),
            Action::CSI(CSI::Sgr(Sgr::Underline(Underline::Single))),
            Action::CSI(CSI::Sgr(Sgr::Underline(Underline::Double))),
            Action::CSI(CSI::Sgr(Sgr::Underline(Underline::Curly))),
            Action::CSI(CSI::Sgr(Sgr::Underline(Underline::Dotted))),
            Action::CSI(CSI::Sgr(Sgr::Underline(Underline::Dashed))),
            Action::Print('b'),
        ],
        actions
    );

    assert_eq!(
        encode(&actions),
        "\x1b[24m\x1b[4m\x1b[21m\x1b[4:3m\x1b[4:4m\x1b[4:5mb"
    );
}

#[test]
fn true_color() {
    let mut p = Parser::new();

    let actions = p.parse_as_vec(b"\x1b[38:2::128:64:192mw");
    assert_eq!(
        vec![
            Action::CSI(CSI::Sgr(Sgr::Foreground(ColorSpec::TrueColor(
                (128, 64, 192).into()
            )))),
            Action::Print('w'),
        ],
        actions
    );

    assert_eq!(encode(&actions), "\u{1b}[38:2::128:64:192mw");

    let actions = p.parse_as_vec(b"\x1b[38:2:0:255:0mw");
    assert_eq!(
        vec![
            Action::CSI(CSI::Sgr(Sgr::Foreground(ColorSpec::TrueColor(
                (0, 255, 0).into()
            )))),
            Action::Print('w'),
        ],
        actions
    );

    let actions = p.parse_as_vec(b"\x1b[38:6:0:255:0:127mw");
    assert_eq!(
        vec![
            Action::CSI(CSI::Sgr(Sgr::Foreground(ColorSpec::TrueColor(
                (0, 255, 0, 127).into()
            )))),
            Action::Print('w'),
        ],
        actions
    );
}

#[test]
fn basic_osc() {
    let mut p = Parser::new();
    let actions = p.parse_as_vec(b"\x1b]0;hello\x07");
    assert_eq!(
        vec![Action::OperatingSystemCommand(Box::new(
            OperatingSystemCommand::SetIconNameAndWindowTitle("hello".to_owned()),
        ))],
        actions
    );
    assert_eq!(encode(&actions), "\x1b]0;hello\x1b\\");

    let actions = p.parse_as_vec(b"\x1b]532534523;hello\x07");
    assert_eq!(
        vec![Action::OperatingSystemCommand(Box::new(
            OperatingSystemCommand::Unspecified(vec![b"532534523".to_vec(), b"hello".to_vec()]),
        ))],
        actions
    );
    assert_eq!(encode(&actions), "\x1b]532534523;hello\x1b\\");
}

#[test]
fn test_emoji_title_osc() {
    let input = "\x1b]0;\u{1f915}\x07";
    let mut p = Parser::new();
    let actions = p.parse_as_vec(input.as_bytes());
    assert_eq!(
        vec![Action::OperatingSystemCommand(Box::new(
            OperatingSystemCommand::SetIconNameAndWindowTitle("\u{1f915}".to_owned()),
        ))],
        actions
    );
    assert_eq!(encode(&actions), "\x1b]0;\u{1f915}\x1b\\");
}

#[test]
fn basic_esc() {
    let mut p = Parser::new();
    let actions = p.parse_as_vec(b"\x1bH");
    assert_eq!(
        vec![Action::Esc(Esc::Code(EscCode::HorizontalTabSet))],
        actions
    );
    assert_eq!(encode(&actions), "\x1bH");

    let actions = p.parse_as_vec(b"\x1b%H");
    assert_eq!(
        vec![Action::Esc(Esc::Unspecified {
            intermediate: Some(b'%'),
            control: b'H',
        })],
        actions
    );
    assert_eq!(encode(&actions), "\x1b%H");
}

#[test]
fn soft_reset() {
    let mut p = Parser::new();
    let actions = p.parse_as_vec(b"\x1b[!p");
    assert_eq!(
        vec![Action::CSI(CSI::Device(Box::new(
            crate::csi::Device::SoftReset
        )))],
        actions
    );
    assert_eq!(encode(&actions), "\x1b[!p");
}

#[test]
fn tmux_title_escape() {
    let mut p = Parser::new();
    let actions = p.parse_as_vec(b"\x1bktitle\x1b\\");
    assert_eq!(
        vec![
            Action::Esc(Esc::Code(EscCode::TmuxTitle)),
            // The bulk printable-ASCII fast path reports the run of text
            // as a single `PrintString`.
            Action::PrintString("title".to_string()),
            Action::Esc(Esc::Code(EscCode::StringTerminator)),
        ],
        actions
    );
}

fn round_trip_parse(s: &str) -> Vec<Action> {
    let mut p = Parser::new();
    let actions = p.parse_as_vec(s.as_bytes());
    assert_eq!(s, encode(&actions), "actions: {actions:?}");
    actions
}

fn parse_as(s: &str, expected: &str) -> Vec<Action> {
    let mut p = Parser::new();
    let actions = p.parse_as_vec(s.as_bytes());
    assert_eq!(expected, encode(&actions), "actions: {actions:?}");
    actions
}

#[test]
fn xtgettcap() {
    assert_eq!(
        round_trip_parse("\x1bP+q544e\x1b\\"),
        vec![
            Action::XtGetTcap(vec!["TN".to_string()]),
            Action::Esc(Esc::Code(EscCode::StringTerminator)),
        ]
    );
}

#[test]
fn bidi_modes() {
    assert_eq!(
        round_trip_parse("\x1b[1 k"),
        vec![Action::CSI(CSI::SelectCharacterPath(
            CharacterPath::LeftToRightOrTopToBottom,
            0
        ))]
    );
    assert_eq!(
        round_trip_parse("\x1b[2;1 k"),
        vec![Action::CSI(CSI::SelectCharacterPath(
            CharacterPath::RightToLeftOrBottomToTop,
            1
        ))]
    );
}

#[test]
fn xterm_key() {
    assert_eq!(
        round_trip_parse("\x1b[>4;2m"),
        vec![Action::CSI(CSI::Mode(Mode::XtermKeyMode {
            resource: XtermKeyModifierResource::OtherKeys,
            value: Some(2),
        }))]
    );
    assert_eq!(
        round_trip_parse("\x1b[>4;m"),
        vec![Action::CSI(CSI::Mode(Mode::XtermKeyMode {
            resource: XtermKeyModifierResource::OtherKeys,
            value: None,
        }))]
    );
}

#[test]
fn window() {
    assert_eq!(
        round_trip_parse("\x1b[22;2t"),
        vec![Action::CSI(CSI::Window(Box::new(Window::PushWindowTitle)))]
    );
}

#[test]
fn checksum_area() {
    assert_eq!(
        round_trip_parse("\x1b[1;2;3;4;5;6*y"),
        vec![Action::CSI(CSI::Window(Box::new(
            Window::ChecksumRectangularArea {
                request_id: 1,
                page_number: 2,
                top: OneBased::new(3),
                left: OneBased::new(4),
                bottom: OneBased::new(5),
                right: OneBased::new(6),
            }
        )))]
    );
}

#[test]
fn dec_private_modes() {
    assert_eq!(
        parse_as("\x1b[?1;1006h", "\x1b[?1h\x1b[?1006h"),
        vec![
            Action::CSI(CSI::Mode(Mode::SetDecPrivateMode(DecPrivateMode::Code(
                DecPrivateModeCode::ApplicationCursorKeys
            ),))),
            Action::CSI(CSI::Mode(Mode::SetDecPrivateMode(DecPrivateMode::Code(
                DecPrivateModeCode::SGRMouse
            ),))),
        ]
    );
}

#[test]
fn xtsmgraphics() {
    assert_eq!(
        round_trip_parse("\x1b[?1;3;256S"),
        vec![Action::CSI(CSI::Device(Box::new(Device::XtSmGraphics(
            XtSmGraphics {
                item: XtSmGraphicsItem::NumberOfColorRegisters,
                action_or_status: 3,
                value: vec![256]
            }
        ))))]
    );
}

#[test]
fn req_attr() {
    assert_eq!(
        round_trip_parse("\x1b[=c"),
        vec![Action::CSI(CSI::Device(Box::new(
            Device::RequestTertiaryDeviceAttributes
        )))]
    );
    assert_eq!(
        round_trip_parse("\x1b[>c"),
        vec![Action::CSI(CSI::Device(Box::new(
            Device::RequestSecondaryDeviceAttributes
        )))]
    );
}

#[test]
fn sgr() {
    assert_eq!(
        parse_as("\x1b[;4m", "\x1b[0m\x1b[4m"),
        vec![
            Action::CSI(CSI::Sgr(Sgr::Reset)),
            Action::CSI(CSI::Sgr(Sgr::Underline(Underline::Single))),
        ]
    );
}

#[test]
fn kitty_img() {
    use crate::apc::*;
    assert_eq!(
        round_trip_parse("\x1b_Gf=24,s=10,v=20;aGVsbG8=\x1b\\"),
        vec![
            Action::KittyImage(Box::new(KittyImage::TransmitData {
                transmit: KittyImageTransmit {
                    format: Some(KittyImageFormat::Rgb),
                    data: KittyImageData::Direct("aGVsbG8=".to_string()),
                    width: Some(10),
                    height: Some(20),
                    image_id: None,
                    image_number: None,
                    compression: KittyImageCompression::None,
                    more_data_follows: false,
                },
                verbosity: KittyImageVerbosity::Verbose,
            })),
            Action::Esc(Esc::Code(EscCode::StringTerminator)),
        ]
    );

    assert_eq!(
        parse_as(
            "\x1b_Ga=q,s=1,v=1,i=1;YWJjZA==\x1b\\",
            "\x1b_Ga=q,i=1,s=1,v=1;YWJjZA==\x1b\\"
        ),
        vec![
            Action::KittyImage(Box::new(KittyImage::Query {
                transmit: KittyImageTransmit {
                    format: None,
                    data: KittyImageData::Direct("YWJjZA==".to_string()),
                    width: Some(1),
                    height: Some(1),
                    image_id: Some(1),
                    image_number: None,
                    compression: KittyImageCompression::None,
                    more_data_follows: false,
                },
            })),
            Action::Esc(Esc::Code(EscCode::StringTerminator)),
        ]
    );
    assert_eq!(
        parse_as(
            "\x1b_Ga=q,t=f,s=1,v=1,i=2;L3Zhci90bXAvdG1wdGYxd3E4Ym4=\x1b\\",
            "\x1b_Ga=q,i=2,s=1,t=f,v=1;L3Zhci90bXAvdG1wdGYxd3E4Ym4=\x1b\\"
        ),
        vec![
            Action::KittyImage(Box::new(KittyImage::Query {
                transmit: KittyImageTransmit {
                    format: None,
                    data: KittyImageData::File {
                        path: "/var/tmp/tmptf1wq8bn".to_string(),
                        data_offset: None,
                        data_size: None,
                    },
                    width: Some(1),
                    height: Some(1),
                    image_id: Some(2),
                    image_number: None,
                    compression: KittyImageCompression::None,
                    more_data_follows: false,
                },
            })),
            Action::Esc(Esc::Code(EscCode::StringTerminator)),
        ]
    );
}

/* Withdrawn because xterm introduced a conflict:
 * <https://github.com/mintty/mintty/issues/1171#issuecomment-1336174469>
 * <https://github.com/mintty/mintty/issues/1189>
#[test]
fn dec_private_sgr() {
    use crate::cell::{VerticalAlign};
    assert_eq!(
        parse_as("\x1b[?0m", "\x1b[0m"),
        vec![Action::CSI(CSI::Sgr(Sgr::Reset))]
    );
    assert_eq!(
        parse_as("\x1b[?4m", "\x1b[73m"),
        vec![Action::CSI(CSI::Sgr(Sgr::VerticalAlign(
            VerticalAlign::SuperScript
        )))]
    );
    assert_eq!(
        parse_as("\x1b[?5m", "\x1b[74m"),
        vec![Action::CSI(CSI::Sgr(Sgr::VerticalAlign(
            VerticalAlign::SubScript
        )))]
    );
    assert_eq!(
        parse_as("\x1b[?24m", "\x1b[75m"),
        vec![Action::CSI(CSI::Sgr(Sgr::VerticalAlign(
            VerticalAlign::BaseLine
        )))]
    );
    assert_eq!(
        parse_as("\x1b[?6m", "\x1b[53m"),
        vec![Action::CSI(CSI::Sgr(Sgr::Overline(true)))]
    );
    assert_eq!(
        parse_as("\x1b[?26m", "\x1b[55m"),
        vec![Action::CSI(CSI::Sgr(Sgr::Overline(false)))]
    );
}
*/

#[test]
fn decset() {
    assert_eq!(
        round_trip_parse("\x1b[?23434h"),
        vec![Action::CSI(CSI::Mode(Mode::SetDecPrivateMode(
            DecPrivateMode::Unspecified(23434),
        )))]
    );

    /*
    {
        let res = CSI::parse(&[CsiParam::Integer(2026)], &[b'?', b'$'], false, 'p').collect();
        assert_eq!(encode(&res), "\x1b[?2026$p");
    }
    */

    assert_eq!(
        round_trip_parse("\x1b[?1l"),
        vec![Action::CSI(CSI::Mode(Mode::ResetDecPrivateMode(
            DecPrivateMode::Code(DecPrivateModeCode::ApplicationCursorKeys,)
        )))]
    );

    assert_eq!(
        round_trip_parse("\x1b[?25s"),
        vec![Action::CSI(CSI::Mode(Mode::SaveDecPrivateMode(
            DecPrivateMode::Code(DecPrivateModeCode::ShowCursor,)
        )))]
    );
    assert_eq!(
        round_trip_parse("\x1b[?2004r"),
        vec![Action::CSI(CSI::Mode(Mode::RestoreDecPrivateMode(
            DecPrivateMode::Code(DecPrivateModeCode::BracketedPaste),
        )))]
    );
    assert_eq!(
        round_trip_parse("\x1b[?12h\x1b[?25h"),
        vec![
            Action::CSI(CSI::Mode(Mode::SetDecPrivateMode(DecPrivateMode::Code(
                DecPrivateModeCode::StartBlinkingCursor,
            )))),
            Action::CSI(CSI::Mode(Mode::SetDecPrivateMode(DecPrivateMode::Code(
                DecPrivateModeCode::ShowCursor,
            )))),
        ]
    );

    assert_eq!(
        round_trip_parse("\x1b[?1002h\x1b[?1003h\x1b[?1005h\x1b[?1006h"),
        vec![
            Action::CSI(CSI::Mode(Mode::SetDecPrivateMode(DecPrivateMode::Code(
                DecPrivateModeCode::ButtonEventMouse,
            )))),
            Action::CSI(CSI::Mode(Mode::SetDecPrivateMode(DecPrivateMode::Code(
                DecPrivateModeCode::AnyEventMouse,
            )))),
            Action::CSI(CSI::Mode(Mode::SetDecPrivateMode(DecPrivateMode::Code(
                DecPrivateModeCode::Utf8Mouse
            )))),
            Action::CSI(CSI::Mode(Mode::SetDecPrivateMode(DecPrivateMode::Code(
                DecPrivateModeCode::SGRMouse,
            )))),
        ]
    );
}

#[test]
fn issue_1291() {
    use crate::osc::{ITermDimension, ITermFileData, ITermProprietary};

    let mut p = Parser::new();
    // Note the empty k=v pair immediately following `File=`
    let actions = p.parse_as_vec(b"\x1b]1337;File=;size=234:aGVsbG8=\x07");
    assert_eq!(
        vec![Action::OperatingSystemCommand(Box::new(
            OperatingSystemCommand::ITermProprietary(ITermProprietary::File(Box::new(
                ITermFileData {
                    name: None,
                    size: Some(234),
                    width: ITermDimension::Automatic,
                    height: ITermDimension::Automatic,
                    preserve_aspect_ratio: true,
                    inline: false,
                    do_not_move_cursor: false,
                    data: b"hello".to_vec(),
                }
            )))
        ))],
        actions
    );
}

#[test]
fn itermfiledata_oob() {
    let mut p = Parser::new();
    p.parse_as_vec(b"\x9d1337\xff;File\x1b");
}

/// vtparse's MAX_OSC was set too low to fully parse this escape sequence.
/// This test verifies that the correct number of actions comes back.
#[test]
fn dynamic_colors() {
    let mut p = Parser::new();
    let actions = p.parse_as_vec(b"\x1b]4;0;#000000;1;#aa3731;2;#448c27;3;#cb9000;4;#325cc0;5;#7a3e9d;6;#0083b2;7;#f7f7f7;8;#777777;9;#f05050;10;#60cb00;11;#ffbc5d;12;#007acc;13;#e64ce6;14;#00aacb;15;#f7f7f7\x07");
    k9::snapshot!(
        actions,
        "
[
    OperatingSystemCommand(
        ChangeColorNumber(
            [
                ChangeColorPair {
                    palette_index: 0,
                    color: Color(
                        SrgbaTuple(
                            0.0,
                            0.0,
                            0.0,
                            1.0,
                        ),
                    ),
                },
                ChangeColorPair {
                    palette_index: 1,
                    color: Color(
                        SrgbaTuple(
                            0.6666667,
                            0.21568628,
                            0.19215687,
                            1.0,
                        ),
                    ),
                },
                ChangeColorPair {
                    palette_index: 2,
                    color: Color(
                        SrgbaTuple(
                            0.26666668,
                            0.54901963,
                            0.15294118,
                            1.0,
                        ),
                    ),
                },
                ChangeColorPair {
                    palette_index: 3,
                    color: Color(
                        SrgbaTuple(
                            0.79607844,
                            0.5647059,
                            0.0,
                            1.0,
                        ),
                    ),
                },
                ChangeColorPair {
                    palette_index: 4,
                    color: Color(
                        SrgbaTuple(
                            0.19607843,
                            0.36078432,
                            0.7529412,
                            1.0,
                        ),
                    ),
                },
                ChangeColorPair {
                    palette_index: 5,
                    color: Color(
                        SrgbaTuple(
                            0.47843137,
                            0.24313726,
                            0.6156863,
                            1.0,
                        ),
                    ),
                },
                ChangeColorPair {
                    palette_index: 6,
                    color: Color(
                        SrgbaTuple(
                            0.0,
                            0.5137255,
                            0.69803923,
                            1.0,
                        ),
                    ),
                },
                ChangeColorPair {
                    palette_index: 7,
                    color: Color(
                        SrgbaTuple(
                            0.96862745,
                            0.96862745,
                            0.96862745,
                            1.0,
                        ),
                    ),
                },
                ChangeColorPair {
                    palette_index: 8,
                    color: Color(
                        SrgbaTuple(
                            0.46666667,
                            0.46666667,
                            0.46666667,
                            1.0,
                        ),
                    ),
                },
                ChangeColorPair {
                    palette_index: 9,
                    color: Color(
                        SrgbaTuple(
                            0.9411765,
                            0.3137255,
                            0.3137255,
                            1.0,
                        ),
                    ),
                },
                ChangeColorPair {
                    palette_index: 10,
                    color: Color(
                        SrgbaTuple(
                            0.3764706,
                            0.79607844,
                            0.0,
                            1.0,
                        ),
                    ),
                },
                ChangeColorPair {
                    palette_index: 11,
                    color: Color(
                        SrgbaTuple(
                            1.0,
                            0.7372549,
                            0.3647059,
                            1.0,
                        ),
                    ),
                },
                ChangeColorPair {
                    palette_index: 12,
                    color: Color(
                        SrgbaTuple(
                            0.0,
                            0.47843137,
                            0.8,
                            1.0,
                        ),
                    ),
                },
                ChangeColorPair {
                    palette_index: 13,
                    color: Color(
                        SrgbaTuple(
                            0.9019608,
                            0.29803923,
                            0.9019608,
                            1.0,
                        ),
                    ),
                },
                ChangeColorPair {
                    palette_index: 14,
                    color: Color(
                        SrgbaTuple(
                            0.0,
                            0.6666667,
                            0.79607844,
                            1.0,
                        ),
                    ),
                },
                ChangeColorPair {
                    palette_index: 15,
                    color: Color(
                        SrgbaTuple(
                            0.96862745,
                            0.96862745,
                            0.96862745,
                            1.0,
                        ),
                    ),
                },
            ],
        ),
    ),
]
"
    );
}

/// Deterministic PRNG (splitmix64) so that the differential tests are
/// reproducible without pulling in an external `rand` dependency.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed ^ 0x9E37_79B9_7F4A_7C15)
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, bound: usize) -> usize {
        if bound == 0 {
            0
        } else {
            (self.next_u64() % bound as u64) as usize
        }
    }

    fn byte(&mut self) -> u8 {
        (self.next_u64() & 0xff) as u8
    }
}

/// Build a random byte stream mixing printable ASCII runs, C0/C1 controls,
/// escape sequences (CSI, OSC, DCS, APC, ESC and their 8-bit forms), UTF-8
/// multi-byte characters (sometimes truncated mid sequence) and
/// invalid/stray bytes.
///
/// The OSC numeric codes are drawn from a set whose semantic parsing copes
/// with arbitrary payloads; the point of these tests is the escape-sequence
/// state machine, not the OSC payload interpreters.
fn gen_bytes(rng: &mut Rng, events: usize) -> Vec<u8> {
    const OSC_CODES: &[&str] = &["0", "1", "2", "8", "52", "777"];
    let mut out = Vec::new();
    for _ in 0..events {
        match rng.below(18) {
            // A run of printable ASCII, sometimes long enough that the
            // chunked variants of the tests cut it in half.
            0..=2 => {
                let len = 1 + rng.below(64);
                for _ in 0..len {
                    out.push(0x20 + rng.below(0x60) as u8);
                }
            }
            // C0 controls (includes ESC, CAN and SUB)
            3 => out.push(rng.below(0x20) as u8),
            // DEL, which is printed in the Ground state
            4 => out.push(0x7f),
            // 8-bit controls and introducers
            5 => out.push(0x80 + rng.below(0x20) as u8),
            // Any byte at all, including invalid utf-8 lead bytes
            6 => out.push(rng.byte()),
            7 => {
                // CSI: ESC [ params intermediate final
                out.push(0x1b);
                out.push(b'[');
                for _ in 0..rng.below(4) {
                    out.extend_from_slice(format!("{}", rng.below(100)).as_bytes());
                    out.push(b';');
                }
                if rng.below(6) == 0 {
                    out.push(0x20 + rng.below(0x10) as u8);
                }
                if rng.below(8) == 0 {
                    out.push(0x3c + rng.below(4) as u8);
                }
                out.push(0x40 + rng.below(0x3f) as u8);
            }
            8 => {
                // OSC: ESC ] code ; payload, terminated by BEL or ST
                out.push(0x1b);
                out.push(b']');
                out.extend_from_slice(OSC_CODES[rng.below(OSC_CODES.len())].as_bytes());
                if rng.below(4) != 0 {
                    out.push(b';');
                    for _ in 0..rng.below(20) {
                        out.push(0x20 + rng.below(0x5f) as u8);
                    }
                }
                out.push(if rng.below(2) == 0 { 0x07 } else { 0x9c });
            }
            9 => {
                // DCS: ESC P params final payload ST
                out.push(0x1b);
                out.push(b'P');
                for _ in 0..rng.below(3) {
                    out.extend_from_slice(format!("{}", rng.below(100)).as_bytes());
                    out.push(b';');
                }
                out.push(0x40 + rng.below(0x3f) as u8);
                for _ in 0..rng.below(20) {
                    out.push(0x20 + rng.below(0x5f) as u8);
                }
                out.extend_from_slice(b"\x1b\\");
            }
            10 => {
                // APC: ESC _ payload ST
                out.push(0x1b);
                out.push(b'_');
                out.push(b'G');
                for _ in 0..rng.below(20) {
                    out.push(0x20 + rng.below(0x5f) as u8);
                }
                out.extend_from_slice(b"\x1b\\");
            }
            11 => {
                // ESC, a couple of intermediates and a final byte
                out.push(0x1b);
                for _ in 0..rng.below(3) {
                    out.push(0x20 + rng.below(0x10) as u8);
                }
                out.push(0x30 + rng.below(0x4f) as u8);
            }
            12 => out.push(0x9b), // 8-bit CSI
            13 => out.push(0x9d), // 8-bit OSC
            14 => out.push(0x90), // 8-bit DCS
            15 => {
                // A 2, 3 or 4 byte utf-8 sequence; a quarter of the time
                // the continuation bytes are dropped so that the next
                // chunk has to finish the sequence.
                let len = 2 + rng.below(3);
                let lead: u8 = match len {
                    2 => 0xc2 + rng.below(0x1e) as u8,
                    3 => 0xe0 + rng.below(0x10) as u8,
                    _ => 0xf0 + rng.below(5) as u8,
                };
                out.push(lead);
                for _ in 0..(len - 1) {
                    out.push(0x80 + rng.below(0x40) as u8);
                }
                if rng.below(4) == 0 {
                    out.truncate(out.len() - (len - 1));
                }
            }
            16 => out.extend_from_slice(b"\x1b[0m"),
            _ => match rng.below(6) {
                0 => out.push(0x98),
                1 => out.push(0x9e),
                2 => out.push(0x9f),
                3 => out.push(0x9c),
                4 => out.push(0x18),
                _ => out.push(0x1a),
            },
        }
    }
    out
}

/// Parse `bytes` one byte at a time through `VTParser::parse_byte`, which is
/// the algorithm that `Parser::parse` used before the Ground-state
/// printable-ASCII fast path was added to `VTParser::parse`.  The resulting
/// actions are coalesced with `Action::append_to`, exactly like the mux does.
fn parse_reference_as_vec(bytes: &[u8]) -> Vec<Action> {
    let mut parser = Parser::new();
    let mut actions: Vec<Action> = Vec::new();
    let mut perform = Performer {
        callback: &mut |action: Action| action.append_to(&mut actions),
        state: &mut parser.state.borrow_mut(),
    };
    for &b in bytes {
        parser.state_machine.parse_byte(b, &mut perform);
    }
    actions
}

/// Parse `bytes` through `Parser::parse` as a single call, coalescing the
/// resulting actions with `Action::append_to`.
fn parse_merged_as_vec(bytes: &[u8]) -> Vec<Action> {
    let mut actions: Vec<Action> = Vec::new();
    Parser::new().parse(bytes, |action| action.append_to(&mut actions));
    actions
}

/// Parse `bytes` through `Parser::parse` in fixed size chunks.
fn parse_merged_fixed_chunks_as_vec(bytes: &[u8], chunk: usize) -> Vec<Action> {
    let mut parser = Parser::new();
    let mut actions: Vec<Action> = Vec::new();
    for c in bytes.chunks(chunk.max(1)) {
        parser.parse(c, |action| action.append_to(&mut actions));
    }
    actions
}

/// Parse `bytes` through `Parser::parse`, cutting it at the given offsets.
fn parse_merged_random_chunks_as_vec(bytes: &[u8], plan: &[usize]) -> Vec<Action> {
    let mut parser = Parser::new();
    let mut actions: Vec<Action> = Vec::new();
    let mut pos = 0;
    for &len in plan {
        if pos >= bytes.len() {
            break;
        }
        let end = (pos + len.max(1)).min(bytes.len());
        parser.parse(&bytes[pos..end], |action| action.append_to(&mut actions));
        pos = end;
    }
    if pos < bytes.len() {
        parser.parse(&bytes[pos..], |action| action.append_to(&mut actions));
    }
    actions
}

/// The bulk printable-ASCII fast path must produce exactly the same
/// coalesced action stream as the original per-byte algorithm, whether the
/// input is parsed as a whole or cut into arbitrary chunks.
#[test]
fn bulk_print_run_matches_reference_for_random_streams() {
    for seed in 0..256u64 {
        let mut rng = Rng::new(seed);
        let events = 1 + rng.below(600);
        let stream = gen_bytes(&mut rng, events);

        let reference = parse_reference_as_vec(&stream);
        assert_eq!(
            parse_merged_as_vec(&stream),
            reference,
            "seed {}: whole-stream parse diverged",
            seed
        );

        for &chunk in &[1usize, 2, 3, 5, 7, 13, 64, 4096] {
            assert_eq!(
                parse_merged_fixed_chunks_as_vec(&stream, chunk),
                reference,
                "seed {}: chunk size {} diverged",
                seed,
                chunk
            );
        }

        // A random chunking, including single byte chunks.
        let mut plan = Vec::new();
        let mut remaining = stream.len();
        while remaining > 0 {
            let len = 1 + rng.below(128);
            plan.push(len);
            remaining = remaining.saturating_sub(len);
        }
        assert_eq!(
            parse_merged_random_chunks_as_vec(&stream, &plan),
            reference,
            "seed {}: random chunking diverged",
            seed
        );
    }
}

#[test]
fn bulk_print_run_empty_input() {
    assert_eq!(parse_merged_as_vec(b""), Vec::<Action>::new());
    assert_eq!(parse_reference_as_vec(b""), Vec::<Action>::new());
}

/// A run of printable ASCII that ends exactly at a chunk boundary, with an
/// escape sequence starting on the very next byte.
#[test]
fn bulk_print_run_ending_at_chunk_boundary() {
    let stream = b"abc\x1b[0mxyz";
    let reference = parse_reference_as_vec(stream);
    assert_eq!(
        reference,
        vec![
            Action::PrintString("abc".to_string()),
            Action::CSI(CSI::Sgr(crate::csi::Sgr::Reset)),
            Action::PrintString("xyz".to_string()),
        ]
    );

    for len in 1..=stream.len() {
        assert_eq!(
            parse_merged_fixed_chunks_as_vec(stream, len),
            reference,
            "chunk size {} diverged",
            len
        );
    }

    let plan = vec![3, 6, 9, 1, 1];
    assert_eq!(
        parse_merged_random_chunks_as_vec(stream, &plan),
        reference,
        "chunk plan {:?} diverged",
        plan
    );
}

/// DEL (0x7f) is printable in the Ground state, so it must be printed by the
/// fast path just like any other byte in 0x20..=0x7f.
#[test]
fn bulk_print_run_prints_del_in_ground() {
    assert_eq!(parse_merged_as_vec(b"\x7f"), vec![Action::Print('\x7f')]);
    let stream = b"abc\x7fdef";
    assert_eq!(parse_merged_as_vec(stream), parse_reference_as_vec(stream));
}

/// Printable bytes inside an OSC or DCS payload are not in the Ground state,
/// so they must not be printed: they belong to those sequences.
#[test]
fn bulk_print_run_not_used_inside_osc_or_dcs_payload() {
    let stream = b"\x1b]0;hello world\x07\x1bP1;2qabc def\x1b\\";
    let actions = parse_merged_as_vec(stream);
    assert_eq!(actions, parse_reference_as_vec(stream));

    // The OSC payload goes to the OSC dispatcher...
    match &actions[0] {
        Action::OperatingSystemCommand(osc) => match osc.as_ref() {
            OperatingSystemCommand::SetIconNameAndWindowTitle(title) => {
                assert_eq!(title, "hello world");
            }
            other => panic!("unexpected osc command: {:?}", other),
        },
        other => panic!("unexpected first action: {:?}", other),
    }
    // ...and nothing at all is printed: the DCS payload is consumed by the
    // sixel builder rather than by the Ground fast path.
    assert!(
        !actions
            .iter()
            .any(|a| matches!(a, Action::Print(_) | Action::PrintString(_))),
        "OSC/DCS payload must not be treated as printable text: {:?}",
        actions
    );

    // A printable run that starts after the sequences are back in the
    // Ground state still goes through the fast path.
    let stream = b"\x1b]0;hello world\x07after\x1bP1;2qabc def\x1b\\tail";
    let actions = parse_merged_as_vec(stream);
    assert_eq!(actions, parse_reference_as_vec(stream));

    let mut printed = String::new();
    for action in &actions {
        match action {
            Action::PrintString(s) => printed.push_str(s),
            Action::Print(c) => printed.push(*c),
            _ => {}
        }
    }
    assert_eq!(printed, "aftertail");
}

/// Printable ASCII following a utf-8 sequence that was split across chunk
/// boundaries must still be printed by the fast path, and the partially
/// received sequence must be completed by the next chunk.
#[test]
fn bulk_print_run_after_utf8_split_across_chunks() {
    let stream = "a\u{451}bc\u{1f915}d".as_bytes().to_vec();
    let reference = parse_reference_as_vec(&stream);
    assert_eq!(parse_merged_as_vec(&stream), reference);

    // Every chunk size, so that each possible split of the multi-byte
    // sequences is exercised.
    for len in 1..=stream.len() {
        assert_eq!(
            parse_merged_fixed_chunks_as_vec(&stream, len),
            reference,
            "chunk size {} diverged",
            len
        );
    }

    // Explicitly: "a" plus the lead byte of "ё" in one chunk, the rest of
    // the sequence plus "bc" in the next.
    let actions = parse_merged_random_chunks_as_vec(&stream, &[2, 5]);
    assert_eq!(actions, reference);
}

/// `Performer::print_run` reports a whole run of printable ASCII as a single
/// `PrintString`, which is what `Action::append_to` produces from the
/// equivalent run of `Print` actions.
#[test]
fn bulk_print_run_reports_runs_as_print_string() {
    assert_eq!(
        Parser::new().parse_as_vec(b"hello world"),
        vec![Action::PrintString("hello world".to_string())]
    );

    // A run split across two `parse` calls coalesces back into a single
    // PrintString via `Action::append_to`.
    let mut parser = Parser::new();
    let mut actions: Vec<Action> = Vec::new();
    parser.parse(b"hel", |action| action.append_to(&mut actions));
    parser.parse(b"lo world", |action| action.append_to(&mut actions));
    assert_eq!(
        actions,
        vec![Action::PrintString("hello world".to_string())]
    );

    // ...and single characters are still reported as `Print`.
    assert_eq!(Parser::new().parse_as_vec(b"x"), vec![Action::Print('x')]);
}
