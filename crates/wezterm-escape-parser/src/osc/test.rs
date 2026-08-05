use super::*;

fn encode(osc: &OperatingSystemCommand) -> String {
    format!("{}", osc)
}

fn parse(osc: &[&str], expected: &str) -> OperatingSystemCommand {
    let mut v = Vec::new();
    for s in osc {
        v.push(s.as_bytes());
    }
    let result = OperatingSystemCommand::parse(&v);

    assert_eq!(encode(&result), expected);

    result
}

#[test]
fn reset_colors() {
    assert_eq!(
        parse(&["104"], "\x1b]104\x1b\\"),
        OperatingSystemCommand::ResetColors(vec![])
    );
    assert_eq!(
        parse(&["104", ""], "\x1b]104\x1b\\"),
        OperatingSystemCommand::ResetColors(vec![])
    );
    assert_eq!(
        parse(&["104", "1"], "\x1b]104;1\x1b\\"),
        OperatingSystemCommand::ResetColors(vec![1])
    );
    assert_eq!(
        parse(&["112"], "\x1b]112\x1b\\"),
        OperatingSystemCommand::ResetDynamicColor(DynamicColorNumber::TextCursorColor)
    );
}

#[test]
fn title() {
    assert_eq!(
        parse(&["0", "hello"], "\x1b]0;hello\x1b\\"),
        OperatingSystemCommand::SetIconNameAndWindowTitle("hello".into())
    );

    assert_eq!(
        parse(&["0", "hello \u{1f915}"], "\x1b]0;hello \u{1f915}\x1b\\"),
        OperatingSystemCommand::SetIconNameAndWindowTitle("hello \u{1f915}".into())
    );

    assert_eq!(
        parse(
            &["0", "hello \u{1f915}", " world"],
            "\x1b]0;hello \u{1f915}; world\x1b\\"
        ),
        OperatingSystemCommand::SetIconNameAndWindowTitle("hello \u{1f915}; world".into())
    );

    // Missing title parameter
    assert_eq!(
        parse(&["0"], "\x1b]0\x1b\\"),
        OperatingSystemCommand::Unspecified(vec![b"0".to_vec()])
    );

    // parsing legacy sun OSC; why bother? This format is used in response
    // to the CSI ReportWindowTitle sequence
    assert_eq!(
        parse(&["lhello"], "\x1b]lhello\x1b\\"),
        OperatingSystemCommand::SetWindowTitleSun("hello".into())
    );
}

#[test]
fn hyperlink() {
    assert_eq!(
        parse(
            &["8", "id=foo", "http://example.com"],
            "\x1b]8;id=foo;http://example.com\x1b\\"
        ),
        OperatingSystemCommand::SetHyperlink(Some(Hyperlink::new_with_id(
            "http://example.com",
            "foo"
        )))
    );

    assert_eq!(
        parse(&["8", "", ""], "\x1b]8;;\x1b\\"),
        OperatingSystemCommand::SetHyperlink(None)
    );

    // too many params
    assert_eq!(
        parse(&["8", "1", "2"], "\x1b]8;1;2\x1b\\"),
        OperatingSystemCommand::Unspecified(vec![b"8".to_vec(), b"1".to_vec(), b"2".to_vec()])
    );

    assert_eq!(
        Hyperlink::parse(&[b"8", b"", b"x"]).unwrap(),
        Some(Hyperlink::new("x"))
    );
}

#[test]
fn finalterm() {
    assert_eq!(
        parse(&["133", "L"], "\x1b]133;L\x1b\\"),
        OperatingSystemCommand::FinalTermSemanticPrompt(FinalTermSemanticPrompt::FreshLine)
    );
    assert_eq!(
        parse(&["133", "C"], "\x1b]133;C\x1b\\"),
        OperatingSystemCommand::FinalTermSemanticPrompt(
            FinalTermSemanticPrompt::MarkEndOfInputAndStartOfOutput { aid: None }
        )
    );

    assert_eq!(
        parse(&["133", "C", "aid=123"], "\x1b]133;C;aid=123\x1b\\"),
        OperatingSystemCommand::FinalTermSemanticPrompt(
            FinalTermSemanticPrompt::MarkEndOfInputAndStartOfOutput {
                aid: Some("123".to_string())
            }
        )
    );

    assert_eq!(
        parse(&["133", "D", "1"], "\x1b]133;D;1\x1b\\"),
        OperatingSystemCommand::FinalTermSemanticPrompt(FinalTermSemanticPrompt::CommandStatus {
            status: 1,
            aid: None
        })
    );

    assert_eq!(
        parse(&["133", "D", "0"], "\x1b]133;D;0\x1b\\"),
        OperatingSystemCommand::FinalTermSemanticPrompt(FinalTermSemanticPrompt::CommandStatus {
            status: 0,
            aid: None
        })
    );

    assert_eq!(
        parse(
            &["133", "D", "0", "aid=23"],
            "\x1b]133;D;0;err=0;aid=23\x1b\\"
        ),
        OperatingSystemCommand::FinalTermSemanticPrompt(FinalTermSemanticPrompt::CommandStatus {
            status: 0,
            aid: Some("23".to_owned())
        })
    );

    assert_eq!(
        parse(
            &["133", "D", "1", "aid=23"],
            "\x1b]133;D;1;err=1;aid=23\x1b\\"
        ),
        OperatingSystemCommand::FinalTermSemanticPrompt(FinalTermSemanticPrompt::CommandStatus {
            status: 1,
            aid: Some("23".to_owned())
        })
    );

    assert_eq!(
        parse(&["133", "P"], "\x1b]133;P;k=i\x1b\\"),
        OperatingSystemCommand::FinalTermSemanticPrompt(FinalTermSemanticPrompt::StartPrompt(
            FinalTermPromptKind::Initial
        ))
    );

    assert_eq!(
        parse(&["133", "P", "k=i"], "\x1b]133;P;k=i\x1b\\"),
        OperatingSystemCommand::FinalTermSemanticPrompt(FinalTermSemanticPrompt::StartPrompt(
            FinalTermPromptKind::Initial
        ))
    );

    assert_eq!(
        parse(&["133", "P", "k=r"], "\x1b]133;P;k=r\x1b\\"),
        OperatingSystemCommand::FinalTermSemanticPrompt(FinalTermSemanticPrompt::StartPrompt(
            FinalTermPromptKind::RightSide
        ))
    );

    assert_eq!(
        parse(&["133", "P", "k=c"], "\x1b]133;P;k=c\x1b\\"),
        OperatingSystemCommand::FinalTermSemanticPrompt(FinalTermSemanticPrompt::StartPrompt(
            FinalTermPromptKind::Continuation
        ))
    );
    assert_eq!(
        parse(&["133", "P", "k=s"], "\x1b]133;P;k=s\x1b\\"),
        OperatingSystemCommand::FinalTermSemanticPrompt(FinalTermSemanticPrompt::StartPrompt(
            FinalTermPromptKind::Secondary
        ))
    );

    assert_eq!(
        parse(&["133", "B"], "\x1b]133;B\x1b\\"),
        OperatingSystemCommand::FinalTermSemanticPrompt(
            FinalTermSemanticPrompt::MarkEndOfPromptAndStartOfInputUntilNextMarker
        ),
    );

    assert_eq!(
        parse(&["133", "I"], "\x1b]133;I\x1b\\"),
        OperatingSystemCommand::FinalTermSemanticPrompt(
            FinalTermSemanticPrompt::MarkEndOfPromptAndStartOfInputUntilEndOfLine
        ),
    );

    assert_eq!(
        parse(&["133", "N"], "\x1b]133;N\x1b\\"),
        OperatingSystemCommand::FinalTermSemanticPrompt(
            FinalTermSemanticPrompt::MarkEndOfCommandWithFreshLine {
                aid: None,
                cl: None,
            }
        ),
    );

    assert_eq!(
        parse(&["133", "N", "aid=12"], "\x1b]133;N;aid=12\x1b\\"),
        OperatingSystemCommand::FinalTermSemanticPrompt(
            FinalTermSemanticPrompt::MarkEndOfCommandWithFreshLine {
                aid: Some("12".to_owned()),
                cl: None,
            }
        ),
    );

    assert_eq!(
        parse(
            &["133", "N", "aid=12", "cl=line"],
            "\x1b]133;N;aid=12;cl=line\x1b\\"
        ),
        OperatingSystemCommand::FinalTermSemanticPrompt(
            FinalTermSemanticPrompt::MarkEndOfCommandWithFreshLine {
                aid: Some("12".to_owned()),
                cl: Some(FinalTermClick::Line),
            }
        ),
    );

    assert_eq!(
        parse(
            &["133", "N", "aid=12", "cl=m"],
            "\x1b]133;N;aid=12;cl=m\x1b\\"
        ),
        OperatingSystemCommand::FinalTermSemanticPrompt(
            FinalTermSemanticPrompt::MarkEndOfCommandWithFreshLine {
                aid: Some("12".to_owned()),
                cl: Some(FinalTermClick::MultipleLine),
            }
        ),
    );

    assert_eq!(
        parse(
            &["133", "N", "aid=12", "cl=v"],
            "\x1b]133;N;aid=12;cl=v\x1b\\"
        ),
        OperatingSystemCommand::FinalTermSemanticPrompt(
            FinalTermSemanticPrompt::MarkEndOfCommandWithFreshLine {
                aid: Some("12".to_owned()),
                cl: Some(FinalTermClick::ConservativeVertical),
            }
        ),
    );
    assert_eq!(
        parse(
            &["133", "N", "aid=12", "cl=w"],
            "\x1b]133;N;aid=12;cl=w\x1b\\"
        ),
        OperatingSystemCommand::FinalTermSemanticPrompt(
            FinalTermSemanticPrompt::MarkEndOfCommandWithFreshLine {
                aid: Some("12".to_owned()),
                cl: Some(FinalTermClick::SmartVertical),
            }
        ),
    );

    assert_eq!(
        parse(
            &["133", "A", "aid=12", "cl=w"],
            "\x1b]133;A;aid=12;cl=w\x1b\\"
        ),
        OperatingSystemCommand::FinalTermSemanticPrompt(
            FinalTermSemanticPrompt::FreshLineAndStartPrompt {
                aid: Some("12".to_owned()),
                cl: Some(FinalTermClick::SmartVertical),
            }
        ),
    );
}

#[test]
fn rxvt() {
    assert_eq!(
        parse(
            &["777", "notify", "alert user", "the tea is ready"],
            "\x1b]777;notify;alert user;the tea is ready\x1b\\"
        ),
        OperatingSystemCommand::RxvtExtension(vec![
            "notify".into(),
            "alert user".into(),
            "the tea is ready".into()
        ]),
    )
}

#[test]
fn conemu() {
    assert_eq!(
        parse(&["9", "4", "1", "42"], "\x1b]9;4;1;42\x1b\\"),
        OperatingSystemCommand::ConEmuProgress(Progress::SetPercentage(42))
    );
    assert_eq!(
        parse(&["9", "4", "2", "64"], "\x1b]9;4;2;64\x1b\\"),
        OperatingSystemCommand::ConEmuProgress(Progress::SetError(64))
    );
    assert_eq!(
        parse(&["9", "4", "3"], "\x1b]9;4;3\x1b\\"),
        OperatingSystemCommand::ConEmuProgress(Progress::SetIndeterminate)
    );
    assert_eq!(
        parse(&["9", "4", "4"], "\x1b]9;4;4\x1b\\"),
        OperatingSystemCommand::ConEmuProgress(Progress::Paused)
    );
}

#[test]
fn iterm() {
    assert_eq!(
        parse(&["1337", "SetMark"], "\x1b]1337;SetMark\x1b\\"),
        OperatingSystemCommand::ITermProprietary(ITermProprietary::SetMark)
    );

    assert_eq!(
        parse(
            &["1337", "CurrentDir=woot"],
            "\x1b]1337;CurrentDir=woot\x1b\\"
        ),
        OperatingSystemCommand::ITermProprietary(ITermProprietary::CurrentDir("woot".into()))
    );

    assert_eq!(
        parse(
            &["1337", "HighlightCursorLine=yes"],
            "\x1b]1337;HighlightCursorLine=yes\x1b\\"
        ),
        OperatingSystemCommand::ITermProprietary(ITermProprietary::HighlightCursorLine(true))
    );

    assert_eq!(
        parse(
            &["1337", "Copy=", "aGVsbG8="],
            "\x1b]1337;Copy=;aGVsbG8=\x1b\\"
        ),
        OperatingSystemCommand::ITermProprietary(ITermProprietary::Copy("hello".into()))
    );

    assert_eq!(
        parse(
            &["1337", "SetUserVar=foo=aGVsbG8="],
            "\x1b]1337;SetUserVar=foo=aGVsbG8=\x1b\\"
        ),
        OperatingSystemCommand::ITermProprietary(ITermProprietary::SetUserVar {
            name: "foo".into(),
            value: "hello".into()
        })
    );

    assert_eq!(
        parse(
            &["1337", "SetBadgeFormat=", "aGVsbG8="],
            "\x1b]1337;SetBadgeFormat=aGVsbG8=\x1b\\"
        ),
        OperatingSystemCommand::ITermProprietary(ITermProprietary::SetBadgeFormat("hello".into()))
    );

    assert_eq!(
        parse(
            &["1337", "ReportCellSize=12.0", "15.5"],
            "\x1b]1337;ReportCellSize=12.0;15.5\x1b\\"
        ),
        OperatingSystemCommand::ITermProprietary(ITermProprietary::ReportCellSize {
            height_pixels: NotNan::new(12.0).unwrap(),
            width_pixels: NotNan::new(15.5).unwrap(),
            scale: None,
        })
    );

    assert_eq!(
        parse(
            &["1337", "ReportCellSize=12.0", "15.5", "2.0"],
            "\x1b]1337;ReportCellSize=12.0;15.5;2.0\x1b\\"
        ),
        OperatingSystemCommand::ITermProprietary(ITermProprietary::ReportCellSize {
            height_pixels: NotNan::new(12.0).unwrap(),
            width_pixels: NotNan::new(15.5).unwrap(),
            scale: Some(NotNan::new(2.0).unwrap()),
        })
    );

    assert_eq!(
        parse(
            &["1337", "File=:aGVsbG8="],
            "\x1b]1337;File=:aGVsbG8=\x1b\\"
        ),
        OperatingSystemCommand::ITermProprietary(ITermProprietary::File(Box::new(ITermFileData {
            name: None,
            size: None,
            width: ITermDimension::Automatic,
            height: ITermDimension::Automatic,
            preserve_aspect_ratio: true,
            inline: false,
            do_not_move_cursor: false,
            data: b"hello".to_vec(),
        })))
    );

    assert_eq!(
        parse(
            &["1337", "File=name=bXluYW1l:aGVsbG8="],
            "\x1b]1337;File=name=bXluYW1l:aGVsbG8=\x1b\\"
        ),
        OperatingSystemCommand::ITermProprietary(ITermProprietary::File(Box::new(ITermFileData {
            name: Some("myname".into()),
            size: None,
            width: ITermDimension::Automatic,
            height: ITermDimension::Automatic,
            preserve_aspect_ratio: true,
            inline: false,
            do_not_move_cursor: false,
            data: b"hello".to_vec(),
        })))
    );

    assert_eq!(
        parse(
            &["1337", "File=size=123", "name=bXluYW1l:aGVsbG8="],
            "\x1b]1337;File=size=123;name=bXluYW1l:aGVsbG8=\x1b\\"
        ),
        OperatingSystemCommand::ITermProprietary(ITermProprietary::File(Box::new(ITermFileData {
            name: Some("myname".into()),
            size: Some(123),
            width: ITermDimension::Automatic,
            height: ITermDimension::Automatic,
            preserve_aspect_ratio: true,
            inline: false,
            do_not_move_cursor: false,
            data: b"hello".to_vec(),
        })))
    );

    assert_eq!(
        parse(
            &["1337", "File=name=bXluYW1l", "size=234:aGVsbG8="],
            "\x1b]1337;File=size=234;name=bXluYW1l:aGVsbG8=\x1b\\"
        ),
        OperatingSystemCommand::ITermProprietary(ITermProprietary::File(Box::new(ITermFileData {
            name: Some("myname".into()),
            size: Some(234),
            width: ITermDimension::Automatic,
            height: ITermDimension::Automatic,
            preserve_aspect_ratio: true,
            inline: false,
            do_not_move_cursor: false,
            data: b"hello".to_vec(),
        })))
    );

    assert_eq!(
        parse(
            &[
                "1337",
                "File=name=bXluYW1l",
                "width=auto",
                "size=234:aGVsbG8="
            ],
            "\x1b]1337;File=size=234;name=bXluYW1l:aGVsbG8=\x1b\\"
        ),
        OperatingSystemCommand::ITermProprietary(ITermProprietary::File(Box::new(ITermFileData {
            name: Some("myname".into()),
            size: Some(234),
            width: ITermDimension::Automatic,
            height: ITermDimension::Automatic,
            preserve_aspect_ratio: true,
            inline: false,
            do_not_move_cursor: false,
            data: b"hello".to_vec(),
        })))
    );

    assert_eq!(
        parse(
            &["1337", "File=name=bXluYW1l", "width=5", "size=234:aGVsbG8="],
            "\x1b]1337;File=size=234;name=bXluYW1l;width=5:aGVsbG8=\x1b\\"
        ),
        OperatingSystemCommand::ITermProprietary(ITermProprietary::File(Box::new(ITermFileData {
            name: Some("myname".into()),
            size: Some(234),
            width: ITermDimension::Cells(5),
            height: ITermDimension::Automatic,
            preserve_aspect_ratio: true,
            inline: false,
            do_not_move_cursor: false,
            data: b"hello".to_vec(),
        })))
    );

    assert_eq!(
        parse(
            &[
                "1337",
                "File=name=bXluYW1l",
                "width=5",
                "height=10%",
                "size=234:aGVsbG8="
            ],
            "\x1b]1337;File=size=234;name=bXluYW1l;width=5;height=10%:aGVsbG8=\x1b\\"
        ),
        OperatingSystemCommand::ITermProprietary(ITermProprietary::File(Box::new(ITermFileData {
            name: Some("myname".into()),
            size: Some(234),
            width: ITermDimension::Cells(5),
            height: ITermDimension::Percent(10),
            preserve_aspect_ratio: true,
            inline: false,
            do_not_move_cursor: false,
            data: b"hello".to_vec(),
        })))
    );

    assert_eq!(
        parse(
            &[
                "1337",
                "File=name=bXluYW1l",
                "preserveAspectRatio=0",
                "width=5",
                "inline=1",
                "height=10px",
                "size=234:aGVsbG8="
            ],
            "\x1b]1337;File=size=234;name=bXluYW1l;width=5;height=10px;preserveAspectRatio=0;inline=1:aGVsbG8=\x1b\\"
        ),
        OperatingSystemCommand::ITermProprietary(ITermProprietary::File(Box::new(ITermFileData {
            name: Some("myname".into()),
            size: Some(234),
            width: ITermDimension::Cells(5),
            height: ITermDimension::Pixels(10),
            preserve_aspect_ratio: false,
            inline: true,
            do_not_move_cursor: false,
            data: b"hello".to_vec(),
        })))
    );

    assert_eq!(
        parse(
            &[
                "1337",
                "File=name=bXluYW1l",
                "preserveAspectRatio=0",
                "width=5",
                "inline=1",
                "doNotMoveCursor=1",
                "height=10px",
                "size=234:aGVsbG8="
            ],
            "\x1b]1337;File=size=234;name=bXluYW1l;width=5;height=10px;preserveAspectRatio=0;inline=1;doNotMoveCursor=1:aGVsbG8=\x1b\\"
        ),
        OperatingSystemCommand::ITermProprietary(ITermProprietary::File(Box::new(ITermFileData {
            name: Some("myname".into()),
            size: Some(234),
            width: ITermDimension::Cells(5),
            height: ITermDimension::Pixels(10),
            preserve_aspect_ratio: false,
            inline: true,
            do_not_move_cursor: true,
            data: b"hello".to_vec(),
        })))
    );
}
