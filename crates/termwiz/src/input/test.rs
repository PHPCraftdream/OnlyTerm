use super::*;
use crate::escape::csi::KittyKeyboardFlags;
use wezterm_input_types::KeyboardLedStatus as InputKeyboardLedStatus;

const NO_MORE: bool = false;
const MAYBE_MORE: bool = true;

#[test]
fn simple() {
    let mut p = InputParser::new();
    let inputs = p.parse_as_vec(b"hello", NO_MORE);
    assert_eq!(
        vec![
            InputEvent::Key(KeyEvent {
                modifiers: Modifiers::NONE,
                key: KeyCode::Char('h'),
            }),
            InputEvent::Key(KeyEvent {
                modifiers: Modifiers::NONE,
                key: KeyCode::Char('e'),
            }),
            InputEvent::Key(KeyEvent {
                modifiers: Modifiers::NONE,
                key: KeyCode::Char('l'),
            }),
            InputEvent::Key(KeyEvent {
                modifiers: Modifiers::NONE,
                key: KeyCode::Char('l'),
            }),
            InputEvent::Key(KeyEvent {
                modifiers: Modifiers::NONE,
                key: KeyCode::Char('o'),
            }),
        ],
        inputs
    );
}

#[test]
fn control_characters() {
    let mut p = InputParser::new();
    let inputs = p.parse_as_vec(b"\x03\x1bJ\x7f", NO_MORE);
    assert_eq!(
        vec![
            InputEvent::Key(KeyEvent {
                modifiers: Modifiers::CTRL,
                key: KeyCode::Char('c'),
            }),
            InputEvent::Key(KeyEvent {
                modifiers: Modifiers::ALT,
                key: KeyCode::Char('J'),
            }),
            InputEvent::Key(KeyEvent {
                modifiers: Modifiers::NONE,
                key: KeyCode::Backspace,
            }),
        ],
        inputs
    );
}

#[test]
fn arrow_keys() {
    let mut p = InputParser::new();
    let inputs = p.parse_as_vec(b"\x1bOA\x1bOB\x1bOC\x1bOD", NO_MORE);
    assert_eq!(
        vec![
            InputEvent::Key(KeyEvent {
                modifiers: Modifiers::NONE,
                key: KeyCode::ApplicationUpArrow,
            }),
            InputEvent::Key(KeyEvent {
                modifiers: Modifiers::NONE,
                key: KeyCode::ApplicationDownArrow,
            }),
            InputEvent::Key(KeyEvent {
                modifiers: Modifiers::NONE,
                key: KeyCode::ApplicationRightArrow,
            }),
            InputEvent::Key(KeyEvent {
                modifiers: Modifiers::NONE,
                key: KeyCode::ApplicationLeftArrow,
            }),
        ],
        inputs
    );
}

#[test]
fn partial() {
    let mut p = InputParser::new();
    let mut inputs = Vec::new();
    // Fragment this F-key sequence across two different pushes
    p.parse(b"\x1b[11", |evt| inputs.push(evt), true);
    p.parse(b"~", |evt| inputs.push(evt), true);
    // make sure we recognize it as just the F-key
    assert_eq!(
        vec![InputEvent::Key(KeyEvent {
            modifiers: Modifiers::NONE,
            key: KeyCode::Function(1),
        })],
        inputs
    );
}

#[test]
fn partial_ambig() {
    let mut p = InputParser::new();

    assert_eq!(
        vec![InputEvent::Key(KeyEvent {
            key: KeyCode::Escape,
            modifiers: Modifiers::NONE,
        })],
        p.parse_as_vec(b"\x1b", false)
    );

    let mut inputs = Vec::new();
    // An incomplete F-key sequence fragmented across two different pushes
    p.parse(b"\x1b[11", |evt| inputs.push(evt), MAYBE_MORE);
    p.parse(b"", |evt| inputs.push(evt), NO_MORE);
    // since we finish with maybe_more false (NO_MORE), the results should be the longest matching
    // parts of said f-key sequence
    assert_eq!(
        vec![
            InputEvent::Key(KeyEvent {
                modifiers: Modifiers::ALT,
                key: KeyCode::Char('['),
            }),
            InputEvent::Key(KeyEvent {
                modifiers: Modifiers::NONE,
                key: KeyCode::Char('1'),
            }),
            InputEvent::Key(KeyEvent {
                modifiers: Modifiers::NONE,
                key: KeyCode::Char('1'),
            }),
        ],
        inputs
    );
}

#[test]
fn partial_mouse() {
    let mut p = InputParser::new();
    let mut inputs = Vec::new();
    // Fragment this mouse sequence across two different pushes
    p.parse(b"\x1b[<0;0;0", |evt| inputs.push(evt), true);
    p.parse(b"M", |evt| inputs.push(evt), true);
    // make sure we recognize it as just the mouse event
    assert_eq!(
        vec![InputEvent::Mouse(MouseEvent {
            x: 0,
            y: 0,
            mouse_buttons: MouseButtons::LEFT,
            modifiers: Modifiers::NONE,
        })],
        inputs
    );
}

#[test]
fn partial_mouse_ambig() {
    let mut p = InputParser::new();
    let mut inputs = Vec::new();
    // Fragment this mouse sequence across two different pushes
    p.parse(b"\x1b[<", |evt| inputs.push(evt), MAYBE_MORE);
    p.parse(b"0;0;0", |evt| inputs.push(evt), NO_MORE);
    // since we finish with maybe_more false (NO_MORE), the results should be the longest matching
    // parts of said mouse sequence
    assert_eq!(
        vec![
            InputEvent::Key(KeyEvent {
                modifiers: Modifiers::ALT,
                key: KeyCode::Char('['),
            }),
            InputEvent::Key(KeyEvent {
                modifiers: Modifiers::NONE,
                key: KeyCode::Char('<'),
            }),
            InputEvent::Key(KeyEvent {
                modifiers: Modifiers::NONE,
                key: KeyCode::Char('0'),
            }),
            InputEvent::Key(KeyEvent {
                modifiers: Modifiers::NONE,
                key: KeyCode::Char(';'),
            }),
            InputEvent::Key(KeyEvent {
                modifiers: Modifiers::NONE,
                key: KeyCode::Char('0'),
            }),
            InputEvent::Key(KeyEvent {
                modifiers: Modifiers::NONE,
                key: KeyCode::Char(';'),
            }),
            InputEvent::Key(KeyEvent {
                modifiers: Modifiers::NONE,
                key: KeyCode::Char('0'),
            }),
        ],
        inputs
    );
}

#[test]
fn alt_left_bracket() {
    // tests that `Alt` + `[` is recognized as a single
    // event rather than two events (one `Esc` the second `Char('[')`)
    let mut p = InputParser::new();

    let mut inputs = Vec::new();
    p.parse(b"\x1b[", |evt| inputs.push(evt), false);

    assert_eq!(
        vec![InputEvent::Key(KeyEvent {
            modifiers: Modifiers::ALT,
            key: KeyCode::Char('['),
        }),],
        inputs
    );
}

#[test]
fn modify_other_keys_parse() {
    let mut p = InputParser::new();
    let inputs = p.parse_as_vec(
        b"\x1b[27;5;13~\x1b[27;5;9~\x1b[27;6;8~\x1b[27;2;127~\x1b[27;6;27~",
        NO_MORE,
    );
    assert_eq!(
        vec![
            InputEvent::Key(KeyEvent {
                key: KeyCode::Enter,
                modifiers: Modifiers::CTRL,
            }),
            InputEvent::Key(KeyEvent {
                key: KeyCode::Tab,
                modifiers: Modifiers::CTRL,
            }),
            InputEvent::Key(KeyEvent {
                key: KeyCode::Backspace,
                modifiers: Modifiers::CTRL | Modifiers::SHIFT,
            }),
            InputEvent::Key(KeyEvent {
                key: KeyCode::Backspace,
                modifiers: Modifiers::SHIFT,
            }),
            InputEvent::Key(KeyEvent {
                key: KeyCode::Escape,
                modifiers: Modifiers::CTRL | Modifiers::SHIFT,
            }),
        ],
        inputs
    );
}

#[test]
fn modify_other_keys_encode() {
    let mode = KeyCodeEncodeModes {
        encoding: KeyboardEncoding::Xterm,
        newline_mode: false,
        application_cursor_keys: false,
        modify_other_keys: None,
    };
    let mode_1 = KeyCodeEncodeModes {
        encoding: KeyboardEncoding::Xterm,
        newline_mode: false,
        application_cursor_keys: false,
        modify_other_keys: Some(1),
    };
    let mode_2 = KeyCodeEncodeModes {
        encoding: KeyboardEncoding::Xterm,
        newline_mode: false,
        application_cursor_keys: false,
        modify_other_keys: Some(2),
    };

    assert_eq!(
        KeyCode::Enter.encode(Modifiers::CTRL, mode, true).unwrap(),
        "\r".to_string()
    );
    assert_eq!(
        KeyCode::Enter
            .encode(Modifiers::CTRL, mode_1, true)
            .unwrap(),
        "\x1b[27;5;13~".to_string()
    );
    assert_eq!(
        KeyCode::Enter
            .encode(Modifiers::CTRL | Modifiers::SHIFT, mode_1, true)
            .unwrap(),
        "\x1b[27;6;13~".to_string()
    );

    // This case is not conformant with xterm!
    // xterm just returns tab for CTRL-Tab when modify_other_keys
    // is not set.
    assert_eq!(
        KeyCode::Tab.encode(Modifiers::CTRL, mode, true).unwrap(),
        "\x1b[9;5u".to_string()
    );
    assert_eq!(
        KeyCode::Tab.encode(Modifiers::CTRL, mode_1, true).unwrap(),
        "\x1b[27;5;9~".to_string()
    );
    assert_eq!(
        KeyCode::Tab
            .encode(Modifiers::CTRL | Modifiers::SHIFT, mode_1, true)
            .unwrap(),
        "\x1b[27;6;9~".to_string()
    );

    assert_eq!(
        KeyCode::Char('c')
            .encode(Modifiers::CTRL, mode, true)
            .unwrap(),
        "\x03".to_string()
    );
    assert_eq!(
        KeyCode::Char('c')
            .encode(Modifiers::CTRL, mode_1, true)
            .unwrap(),
        "\x03".to_string()
    );
    assert_eq!(
        KeyCode::Char('c')
            .encode(Modifiers::CTRL, mode_2, true)
            .unwrap(),
        "\x1b[27;5;99~".to_string()
    );

    assert_eq!(
        KeyCode::Char('1')
            .encode(Modifiers::CTRL, mode, true)
            .unwrap(),
        "1".to_string()
    );
    assert_eq!(
        KeyCode::Char('1')
            .encode(Modifiers::CTRL, mode_2, true)
            .unwrap(),
        "\x1b[27;5;49~".to_string()
    );

    assert_eq!(
        KeyCode::Char(',')
            .encode(Modifiers::CTRL, mode, true)
            .unwrap(),
        ",".to_string()
    );
    assert_eq!(
        KeyCode::Char(',')
            .encode(Modifiers::CTRL, mode_2, true)
            .unwrap(),
        "\x1b[27;5;44~".to_string()
    );
}

#[test]
fn encode_issue_892() {
    let mode = KeyCodeEncodeModes {
        encoding: KeyboardEncoding::Xterm,
        newline_mode: false,
        application_cursor_keys: false,
        modify_other_keys: None,
    };

    assert_eq!(
        KeyCode::LeftArrow
            .encode(Modifiers::NONE, mode, true)
            .unwrap(),
        "\x1b[D".to_string()
    );
    assert_eq!(
        KeyCode::LeftArrow
            .encode(Modifiers::ALT, mode, true)
            .unwrap(),
        "\x1b[1;3D".to_string()
    );
    assert_eq!(
        KeyCode::Home.encode(Modifiers::NONE, mode, true).unwrap(),
        "\x1b[H".to_string()
    );
    assert_eq!(
        KeyCode::Home.encode(Modifiers::ALT, mode, true).unwrap(),
        "\x1b[1;3H".to_string()
    );
    assert_eq!(
        KeyCode::End.encode(Modifiers::NONE, mode, true).unwrap(),
        "\x1b[F".to_string()
    );
    assert_eq!(
        KeyCode::End.encode(Modifiers::ALT, mode, true).unwrap(),
        "\x1b[1;3F".to_string()
    );
    assert_eq!(
        KeyCode::Tab.encode(Modifiers::ALT, mode, true).unwrap(),
        "\x1b\t".to_string()
    );
    assert_eq!(
        KeyCode::PageUp.encode(Modifiers::ALT, mode, true).unwrap(),
        "\x1b[5;3~".to_string()
    );
    assert_eq!(
        KeyCode::Function(1)
            .encode(Modifiers::NONE, mode, true)
            .unwrap(),
        "\x1bOP".to_string()
    );
}

#[test]
fn partial_bracketed_paste() {
    let mut p = InputParser::new();

    let input = b"\x1b[200~1234";
    let input2 = b"5678\x1b[201~";

    let mut inputs = vec![];

    p.parse(input, |e| inputs.push(e), false);
    p.parse(input2, |e| inputs.push(e), false);

    assert_eq!(vec![InputEvent::Paste("12345678".to_owned())], inputs)
}

#[test]
fn mouse_horizontal_scroll() {
    let mut p = InputParser::new();

    let input = b"\x1b[<66;42;12M\x1b[<67;42;12M";
    let res = p.parse_as_vec(input, MAYBE_MORE);

    assert_eq!(
        vec![
            InputEvent::Mouse(MouseEvent {
                x: 42,
                y: 12,
                mouse_buttons: MouseButtons::HORZ_WHEEL | MouseButtons::WHEEL_POSITIVE,
                modifiers: Modifiers::NONE,
            }),
            InputEvent::Mouse(MouseEvent {
                x: 42,
                y: 12,
                mouse_buttons: MouseButtons::HORZ_WHEEL,
                modifiers: Modifiers::NONE,
            })
        ],
        res
    );
}

#[test]
fn encode_issue_3478_xterm() {
    let mode = KeyCodeEncodeModes {
        encoding: KeyboardEncoding::Xterm,
        newline_mode: false,
        application_cursor_keys: false,
        modify_other_keys: None,
    };

    assert_eq!(
        KeyCode::Numpad0
            .encode(Modifiers::NONE, mode, true)
            .unwrap(),
        "\u{1b}[2~".to_string()
    );
    assert_eq!(
        KeyCode::Numpad0
            .encode(Modifiers::SHIFT, mode, true)
            .unwrap(),
        "\u{1b}[2;2~".to_string()
    );

    assert_eq!(
        KeyCode::Numpad1
            .encode(Modifiers::NONE, mode, true)
            .unwrap(),
        "\u{1b}[F".to_string()
    );
    assert_eq!(
        KeyCode::Numpad1
            .encode(Modifiers::NONE | Modifiers::SHIFT, mode, true)
            .unwrap(),
        "\u{1b}[1;2F".to_string()
    );
}

#[test]
fn encode_tab_with_modifiers() {
    let mode = KeyCodeEncodeModes {
        encoding: KeyboardEncoding::Xterm,
        newline_mode: false,
        application_cursor_keys: false,
        modify_other_keys: None,
    };

    let mods_to_result = [
        (Modifiers::SHIFT, "\u{1b}[Z"),
        (Modifiers::SHIFT | Modifiers::LEFT_SHIFT, "\u{1b}[Z"),
        (Modifiers::SHIFT | Modifiers::RIGHT_SHIFT, "\u{1b}[Z"),
        (Modifiers::CTRL, "\u{1b}[9;5u"),
        (Modifiers::CTRL | Modifiers::LEFT_CTRL, "\u{1b}[9;5u"),
        (Modifiers::CTRL | Modifiers::RIGHT_CTRL, "\u{1b}[9;5u"),
        (
            Modifiers::SHIFT | Modifiers::CTRL | Modifiers::LEFT_CTRL | Modifiers::LEFT_SHIFT,
            "\u{1b}[1;5Z",
        ),
    ];
    for (mods, result) in mods_to_result {
        assert_eq!(
            KeyCode::Tab.encode(mods, mode, true).unwrap(),
            result,
            "{:?}",
            mods
        );
    }
}

#[test]
fn encode_kitty_matches_input_types_encode_kitty() {
    // In Kitty mode, termwiz's KeyCode::encode must delegate to the same
    // wezterm_input_types::KeyEvent::encode_kitty encoder used by the GUI
    // layer (this is what makes the kitty keyboard protocol work for
    // panes reached via the mux / `wezterm connect`). Verify the wiring --
    // the to_input_types_key_code mapping plus the encode_kitty call --
    // by comparing against a direct encode_kitty call on the equivalent
    // input-types KeyEvent, for both key-down and key-up.
    let flags = KittyKeyboardFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES
        | KittyKeyboardFlags::REPORT_EVENT_TYPES;
    let mode = KeyCodeEncodeModes {
        encoding: KeyboardEncoding::Kitty(flags),
        newline_mode: false,
        application_cursor_keys: false,
        modify_other_keys: None,
    };

    // (termwiz KeyCode, equivalent wezterm_input_types::KeyCode)
    let cases: &[(KeyCode, wezterm_input_types::KeyCode)] = &[
        (KeyCode::Char('a'), wezterm_input_types::KeyCode::Char('a')),
        (KeyCode::Enter, wezterm_input_types::KeyCode::Char('\r')),
        (
            KeyCode::Escape,
            wezterm_input_types::KeyCode::Char('\u{1b}'),
        ),
    ];

    for (tw_key, it_key) in cases {
        for is_down in [true, false] {
            let via_encode = tw_key.encode(Modifiers::NONE, mode, is_down).unwrap();

            let direct = wezterm_input_types::KeyEvent {
                key: it_key.clone(),
                modifiers: Modifiers::NONE,
                leds: InputKeyboardLedStatus::default(),
                repeat_count: 1,
                key_is_down: is_down,
                raw: None,
                #[cfg(windows)]
                win32_uni_char: None,
            }
            .encode_kitty(flags);

            assert_eq!(
                via_encode, direct,
                "termwiz encode({:?}, down={}) must match input-types \
                 encode_kitty on the equivalent key",
                tw_key, is_down
            );

            // With REPORT_EVENT_TYPES negotiated, the mux path must not
            // drop key-up events (the old code unconditionally returned
            // an empty string for !is_down *before* checking the mode).
            assert!(
                !via_encode.is_empty(),
                "kitty key-up for {:?} must produce output, not be dropped",
                tw_key
            );
        }
    }

    // Regression guard: for non-kitty encodings the !is_down short-circuit
    // must still apply (it was relocated to *after* the kitty branch).
    let xterm_mode = KeyCodeEncodeModes {
        encoding: KeyboardEncoding::Xterm,
        ..mode
    };
    assert_eq!(
        KeyCode::Char('a')
            .encode(Modifiers::NONE, xterm_mode, false)
            .unwrap(),
        String::new(),
        "xterm encoding must still drop key-up events"
    );

    // Internal synthetic keys never reach application input.
    let internal_mode = KeyCodeEncodeModes {
        encoding: KeyboardEncoding::Kitty(flags),
        ..mode
    };
    assert_eq!(
        KeyCode::InternalPasteStart
            .encode(Modifiers::NONE, internal_mode, true)
            .unwrap(),
        String::new(),
        "InternalPasteStart must not be encoded to application input"
    );
}
