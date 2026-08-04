    use super::*;
    use k9::assert_equal as assert_eq;

    #[test]
    fn test_parse_line() {
        let _ = env_logger::Builder::new()
            .is_test(true)
            .filter_level(log::LevelFilter::Trace)
            .try_init();

        assert_eq!(
            Event::Begin {
                timestamp: 12345,
                number: 321,
                flags: 0,
            },
            parse_line(b"%begin 12345 321 0").unwrap()
        );

        assert_eq!(
            Event::End {
                timestamp: 12345,
                number: 321,
                flags: 0,
            },
            parse_line(b"%end 12345 321 0").unwrap()
        );
    }

    #[test]
    fn test_parse_sequence() {
        let input = b"%sessions-changed
%pane-mode-changed %0
%begin 1604279270 310 0
stuff
in
here
%end 1604279270 310 0
%window-add @1
%window-close @38
%unlinked-window-close @39
%sessions-changed
%session-changed $1 1
%client-session-changed /dev/pts/5 $1 home
%client-detached /dev/pts/10
%layout-change @1 b25d,80x24,0,0,0
%layout-change @1 cafd,120x29,0,0,0 cafd,120x29,0,0,0 *
%output %1 \\033[1m\\033[7m%\\033[27m\\033[1m\\033[0m    \\015 \\015
%output %1 \\033kwez@cube-localdomain:~\\033\\134\\033]2;wez@cube-localdomain:~\\033\\134
%output %1 \\033]7;file://cube-localdomain/home/wez\\033\\134
%output %1 \\033[K\\033[?2004h
%exit
%exit I said so
%config-error /home/joe/.tmux.conf:1: unknown command: dadsafafasdf
%continue %2
%extended-output %1 \\033[1m\\033[7m%\\033[27m\\033[1m\\033[0m    \\015 \\015
%message message text
%unlinked-window-add @40
%unlinked-window-renamed @41
%paste-buffer-changed just something
%paste-buffer-deleted just something else
%pause %3
%subscription-changed something we don't handle so far
";

        let mut p = Parser::new();
        let events = p.advance_bytes(input).unwrap();
        assert_eq!(
            vec![
                Event::SessionsChanged,
                Event::PaneModeChanged { pane: 0 },
                Event::Guarded(Guarded {
                    timestamp: 1604279270,
                    number: 310,
                    flags: 0,
                    error: false,
                    output: "stuff\nin\nhere\n".to_owned()
                }),
                Event::WindowAdd { window: 1 },
                Event::WindowClose { window: 38 },
                Event::UnlinkedWindowClose { window: 39 },
                Event::SessionsChanged,
                Event::SessionChanged {
                    session: 1,
                    name: "1".to_owned(),
                },
                Event::ClientSessionChanged {
                    client_name: "/dev/pts/5".to_owned(),
                    session: 1,
                    session_name: "home".to_owned()
                },
                Event::ClientDetached {
                    client_name: "/dev/pts/10".to_owned()
                },
                Event::LayoutChange {
                    window: 1,
                    layout: "b25d,80x24,0,0,0".to_owned(),
                    visible_layout: None,
                    raw_flags: None
                },
                Event::LayoutChange {
                    window: 1,
                    layout: "cafd,120x29,0,0,0".to_owned(),
                    visible_layout: Some("cafd,120x29,0,0,0".to_owned()),
                    raw_flags: Some("*".to_owned())
                },
                Event::Output {
                    pane: 1,
                    text: "\x1b[1m\x1b[7m%\x1b[27m\x1b[1m\x1b[0m    \r \r"
                        .to_owned()
                        .as_bytes()
                        .to_vec()
                },
                Event::Output {
                    pane: 1,
                    text: "\x1bkwez@cube-localdomain:~\x1b\\\x1b]2;wez@cube-localdomain:~\x1b\\"
                        .to_owned()
                        .as_bytes()
                        .to_vec()
                },
                Event::Output {
                    pane: 1,
                    text: "\x1b]7;file://cube-localdomain/home/wez\x1b\\"
                        .to_owned()
                        .as_bytes()
                        .to_vec(),
                },
                Event::Output {
                    pane: 1,
                    text: "\x1b[K\x1b[?2004h".to_owned().as_bytes().to_vec(),
                },
                Event::Exit { reason: None },
                Event::Exit {
                    reason: Some("I said so".to_owned())
                },
                Event::ConfigError {
                    error: "/home/joe/.tmux.conf:1: unknown command: dadsafafasdf".to_owned()
                },
                Event::Continue { pane: 2 },
                Event::ExtendedOutput {
                    pane: 1,
                    text: "\x1b[1m\x1b[7m%\x1b[27m\x1b[1m\x1b[0m    \r \r"
                        .to_owned()
                        .as_bytes()
                        .to_vec()
                },
                Event::Message {
                    message: "message text".to_owned()
                },
                Event::UnlinkedWindowAdd { window: 40 },
                Event::UnlinkedWindowRenamed { window: 41 },
                Event::PasteBufferChanged {
                    buffer: "just something".to_owned()
                },
                Event::PasteBufferDeleted {
                    buffer: "just something else".to_owned()
                },
                Event::Pause { pane: 3 },
                Event::SubscriptionChanged,
            ],
            events
        );
    }

    #[test]
    fn test_parse_layout() {
        let layout_case1 = "158x40,0,0,72".to_string();
        let layout_case2 = "158x40,0,0[158x20,0,0,69,158x19,0,21{79x19,0,21,70,78x19,80,21[78x9,80,21,71,78x9,80,31,73]}]".to_string();
        let layout_case3 = "158x40,0,0{79x40,0,0[79x20,0,0,74,79x19,0,21{39x19,0,21,76,39x19,40,21,77}],78x40,80,0,75}".to_string();

        let mut layout = parse_layout(&layout_case1).unwrap();
        let l = layout.pop().unwrap();
        assert!(if let WindowLayout::SinglePane(p) = l {
            assert_eq!(p.pane_width, 158);
            assert_eq!(p.pane_height, 40);
            assert_eq!(p.pane_left, 0);
            assert_eq!(p.pane_top, 0);
            assert_eq!(p.pane_id, 72);
            true
        } else {
            false
        });

        layout = parse_layout(&layout_case2).unwrap();
        assert!(matches!(&layout[0], WindowLayout::SplitVertical(_x)));
        assert!(matches!(&layout[1], WindowLayout::SplitHorizontal(_x)));
        assert!(matches!(&layout[2], WindowLayout::SplitVertical(_x)));
        layout = parse_layout(&layout_case3).unwrap();
        assert!(matches!(&layout[0], WindowLayout::SplitHorizontal(_x)));
        assert!(matches!(&layout[1], WindowLayout::SplitVertical(_x)));
        assert!(matches!(&layout[2], WindowLayout::SplitHorizontal(_x)));
    }
