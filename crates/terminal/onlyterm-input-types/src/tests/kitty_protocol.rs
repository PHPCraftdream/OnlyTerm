use crate::*;

#[test]
fn encode_issue_3474() {
    let flags = KittyKeyboardFlags::DISAMBIGUATE_ESCAPE_CODES
        | KittyKeyboardFlags::REPORT_EVENT_TYPES
        | KittyKeyboardFlags::REPORT_ALTERNATE_KEYS
        | KittyKeyboardFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES;

    assert_eq!(
        KeyEvent {
            key: KeyCode::Char('A'),
            modifiers: Modifiers::NONE,
            leds: KeyboardLedStatus::empty(),
            repeat_count: 1,
            key_is_down: true,
            raw: None,
            #[cfg(windows)]
            win32_uni_char: None,
        }
        .encode_kitty(flags),
        "\u{1b}[97:65;1u".to_string()
    );
    assert_eq!(
        KeyEvent {
            key: KeyCode::Char('A'),
            modifiers: Modifiers::NONE,
            leds: KeyboardLedStatus::empty(),
            repeat_count: 1,
            key_is_down: false,
            raw: None,
            #[cfg(windows)]
            win32_uni_char: None,
        }
        .encode_kitty(flags),
        "\u{1b}[97:65;1:3u".to_string()
    );
}

pub(super) fn make_event_with_raw(mut event: KeyEvent, phys: Option<PhysKeyCode>) -> KeyEvent {
    let phys = match phys {
        Some(phys) => Some(phys),
        None => event.key.to_phys(),
    };

    event.raw = Some(RawKeyEvent {
        key: event.key.clone(),
        modifiers: event.modifiers,
        leds: KeyboardLedStatus::empty(),
        phys_code: phys,
        raw_code: 0,
        #[cfg(windows)]
        scan_code: 0,
        repeat_count: 1,
        key_is_down: event.key_is_down,
        handled: Handled::new(),
    });

    event
}

#[test]
fn encode_issue_3476() {
    let flags = KittyKeyboardFlags::DISAMBIGUATE_ESCAPE_CODES
        | KittyKeyboardFlags::REPORT_EVENT_TYPES
        | KittyKeyboardFlags::REPORT_ALTERNATE_KEYS
        | KittyKeyboardFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES;

    assert_eq!(
        make_event_with_raw(
            KeyEvent {
                key: KeyCode::LeftShift,
                modifiers: Modifiers::NONE,
                leds: KeyboardLedStatus::empty(),
                repeat_count: 1,
                key_is_down: true,
                raw: None,
                #[cfg(windows)]
                win32_uni_char: None,
            },
            None
        )
        .encode_kitty(flags),
        "\u{1b}[57441;1u".to_string()
    );
    assert_eq!(
        make_event_with_raw(
            KeyEvent {
                key: KeyCode::LeftShift,
                modifiers: Modifiers::NONE,
                leds: KeyboardLedStatus::empty(),
                repeat_count: 1,
                key_is_down: false,
                raw: None,
                #[cfg(windows)]
                win32_uni_char: None,
            },
            None
        )
        .encode_kitty(flags),
        "\u{1b}[57441;1:3u".to_string()
    );
    assert_eq!(
        make_event_with_raw(
            KeyEvent {
                key: KeyCode::LeftControl,
                modifiers: Modifiers::NONE,
                leds: KeyboardLedStatus::empty(),
                repeat_count: 1,
                key_is_down: true,
                raw: None,
                #[cfg(windows)]
                win32_uni_char: None,
            },
            None
        )
        .encode_kitty(flags),
        "\u{1b}[57442;1u".to_string()
    );
    assert_eq!(
        make_event_with_raw(
            KeyEvent {
                key: KeyCode::LeftControl,
                modifiers: Modifiers::NONE,
                leds: KeyboardLedStatus::empty(),
                repeat_count: 1,
                key_is_down: false,
                raw: None,
                #[cfg(windows)]
                win32_uni_char: None,
            },
            None
        )
        .encode_kitty(flags),
        "\u{1b}[57442;1:3u".to_string()
    );
}

#[test]
fn encode_issue_3478() {
    let flags = KittyKeyboardFlags::DISAMBIGUATE_ESCAPE_CODES
        | KittyKeyboardFlags::REPORT_EVENT_TYPES
        | KittyKeyboardFlags::REPORT_ALTERNATE_KEYS
        | KittyKeyboardFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES;

    assert_eq!(
        make_event_with_raw(
            KeyEvent {
                key: KeyCode::Numpad(0),
                modifiers: Modifiers::NONE,
                leds: KeyboardLedStatus::empty(),
                repeat_count: 1,
                key_is_down: true,
                raw: None,
                #[cfg(windows)]
                win32_uni_char: None,
            },
            None
        )
        .encode_kitty(flags),
        "\u{1b}[57425;1u".to_string()
    );
    assert_eq!(
        make_event_with_raw(
            KeyEvent {
                key: KeyCode::Numpad(0),
                modifiers: Modifiers::SHIFT,
                leds: KeyboardLedStatus::empty(),
                repeat_count: 1,
                key_is_down: true,
                raw: None,
                #[cfg(windows)]
                win32_uni_char: None,
            },
            None
        )
        .encode_kitty(flags),
        "\u{1b}[57425;2u".to_string()
    );

    assert_eq!(
        make_event_with_raw(
            KeyEvent {
                key: KeyCode::Numpad(1),
                modifiers: Modifiers::NONE,
                leds: KeyboardLedStatus::empty(),
                repeat_count: 1,
                key_is_down: true,
                raw: None,
                #[cfg(windows)]
                win32_uni_char: None,
            },
            None
        )
        .encode_kitty(flags),
        "\u{1b}[57424;1u".to_string()
    );
    assert_eq!(
        make_event_with_raw(
            KeyEvent {
                key: KeyCode::Numpad(1),
                modifiers: Modifiers::SHIFT,
                leds: KeyboardLedStatus::empty(),
                repeat_count: 1,
                key_is_down: true,
                raw: None,
                #[cfg(windows)]
                win32_uni_char: None,
            },
            None
        )
        .encode_kitty(flags),
        "\u{1b}[57424;2u".to_string()
    );

    assert_eq!(
        make_event_with_raw(
            KeyEvent {
                key: KeyCode::Numpad(0),
                modifiers: Modifiers::NONE,
                leds: KeyboardLedStatus::NUM_LOCK,
                repeat_count: 1,
                key_is_down: true,
                raw: None,
                #[cfg(windows)]
                win32_uni_char: None,
            },
            Some(PhysKeyCode::Keypad0)
        )
        .encode_kitty(flags),
        "\u{1b}[57399;129u".to_string()
    );
    assert_eq!(
        make_event_with_raw(
            KeyEvent {
                key: KeyCode::Numpad(0),
                modifiers: Modifiers::SHIFT,
                leds: KeyboardLedStatus::NUM_LOCK,
                repeat_count: 1,
                key_is_down: true,
                raw: None,
                #[cfg(windows)]
                win32_uni_char: None,
            },
            Some(PhysKeyCode::Keypad0)
        )
        .encode_kitty(flags),
        "\u{1b}[57399;130u".to_string()
    );

    assert_eq!(
        make_event_with_raw(
            KeyEvent {
                key: KeyCode::Numpad(5),
                modifiers: Modifiers::NONE,
                leds: KeyboardLedStatus::NUM_LOCK,
                repeat_count: 1,
                key_is_down: true,
                raw: None,
                #[cfg(windows)]
                win32_uni_char: None,
            },
            Some(PhysKeyCode::Keypad5)
        )
        .encode_kitty(flags),
        "\u{1b}[57404;129u".to_string()
    );

    assert_eq!(
        make_event_with_raw(
            KeyEvent {
                key: KeyCode::Numpad(5),
                modifiers: Modifiers::NONE,
                leds: KeyboardLedStatus::empty(),
                repeat_count: 1,
                key_is_down: true,
                raw: None,
                #[cfg(windows)]
                win32_uni_char: None,
            },
            Some(PhysKeyCode::Keypad5)
        )
        .encode_kitty(flags),
        "\u{1b}[E".to_string()
    );
}

#[test]
fn encode_issue_3478_extra() {
    let flags = KittyKeyboardFlags::DISAMBIGUATE_ESCAPE_CODES
        | KittyKeyboardFlags::REPORT_EVENT_TYPES
        | KittyKeyboardFlags::REPORT_ALTERNATE_KEYS
        | KittyKeyboardFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES
        | KittyKeyboardFlags::REPORT_ASSOCIATED_TEXT;

    assert_eq!(
        make_event_with_raw(
            KeyEvent {
                key: KeyCode::Numpad(5),
                modifiers: Modifiers::NONE,
                leds: KeyboardLedStatus::NUM_LOCK,
                repeat_count: 1,
                key_is_down: true,
                raw: None,
                #[cfg(windows)]
                win32_uni_char: None,
            },
            Some(PhysKeyCode::Keypad5)
        )
        .encode_kitty(flags),
        "\u{1b}[57404;129;53u".to_string()
    );
    assert_eq!(
        make_event_with_raw(
            KeyEvent {
                key: KeyCode::Numpad(5),
                modifiers: Modifiers::NONE,
                leds: KeyboardLedStatus::NUM_LOCK,
                repeat_count: 1,
                key_is_down: false,
                raw: None,
                #[cfg(windows)]
                win32_uni_char: None,
            },
            Some(PhysKeyCode::Keypad5)
        )
        .encode_kitty(flags),
        "\u{1b}[57404;129:3u".to_string()
    );

    assert_eq!(
        make_event_with_raw(
            KeyEvent {
                key: KeyCode::Numpad(5),
                modifiers: Modifiers::NONE,
                leds: KeyboardLedStatus::empty(),
                repeat_count: 1,
                key_is_down: true,
                raw: None,
                #[cfg(windows)]
                win32_uni_char: None,
            },
            Some(PhysKeyCode::Keypad5)
        )
        .encode_kitty(flags),
        "\u{1b}[E".to_string()
    );

    assert_eq!(
        make_event_with_raw(
            KeyEvent {
                key: KeyCode::Numpad(5),
                modifiers: Modifiers::NONE,
                leds: KeyboardLedStatus::empty(),
                repeat_count: 1,
                key_is_down: false,
                raw: None,
                #[cfg(windows)]
                win32_uni_char: None,
            },
            Some(PhysKeyCode::Keypad5)
        )
        .encode_kitty(flags),
        "\u{1b}[1;1:3E".to_string()
    );
}

#[test]
fn encode_issue_3315() {
    let flags = KittyKeyboardFlags::DISAMBIGUATE_ESCAPE_CODES;

    assert_eq!(
        KeyEvent {
            key: KeyCode::Char('"'),
            modifiers: Modifiers::NONE,
            leds: KeyboardLedStatus::empty(),
            repeat_count: 1,
            key_is_down: true,
            raw: None,
            #[cfg(windows)]
            win32_uni_char: None,
        }
        .encode_kitty(flags),
        "\"".to_string()
    );

    assert_eq!(
        KeyEvent {
            key: KeyCode::Char('"'),
            modifiers: Modifiers::SHIFT,
            leds: KeyboardLedStatus::empty(),
            repeat_count: 1,
            key_is_down: true,
            raw: None,
            #[cfg(windows)]
            win32_uni_char: None,
        }
        .encode_kitty(flags),
        "\"".to_string()
    );

    assert_eq!(
        KeyEvent {
            key: KeyCode::Char('!'),
            modifiers: Modifiers::SHIFT,
            leds: KeyboardLedStatus::empty(),
            repeat_count: 1,
            key_is_down: true,
            raw: None,
            #[cfg(windows)]
            win32_uni_char: None,
        }
        .encode_kitty(flags),
        "!".to_string()
    );

    assert_eq!(
        KeyEvent {
            key: KeyCode::LeftShift,
            modifiers: Modifiers::NONE,
            leds: KeyboardLedStatus::empty(),
            repeat_count: 1,
            key_is_down: true,
            raw: None,
            #[cfg(windows)]
            win32_uni_char: None,
        }
        .encode_kitty(flags),
        "".to_string()
    );
}

#[test]
fn encode_issue_3479() {
    let flags = KittyKeyboardFlags::DISAMBIGUATE_ESCAPE_CODES
        | KittyKeyboardFlags::REPORT_EVENT_TYPES
        | KittyKeyboardFlags::REPORT_ALTERNATE_KEYS
        | KittyKeyboardFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES;

    assert_eq!(
        make_event_with_raw(
            KeyEvent {
                key: KeyCode::Char('ф'),
                modifiers: Modifiers::CTRL,
                leds: KeyboardLedStatus::empty(),
                repeat_count: 1,
                key_is_down: true,
                raw: None,
                #[cfg(windows)]
                win32_uni_char: None,
            },
            Some(PhysKeyCode::A)
        )
        .encode_kitty(flags),
        "\x1b[1092::97;5u".to_string()
    );

    assert_eq!(
        make_event_with_raw(
            KeyEvent {
                key: KeyCode::Char('Ф'),
                modifiers: Modifiers::CTRL | Modifiers::SHIFT,
                leds: KeyboardLedStatus::empty(),
                repeat_count: 1,
                key_is_down: true,
                raw: None,
                #[cfg(windows)]
                win32_uni_char: None,
            },
            Some(PhysKeyCode::A)
        )
        .encode_kitty(flags),
        "\x1b[1092:1060:97;6u".to_string()
    );
}

#[test]
fn encode_issue_3484() {
    let flags = KittyKeyboardFlags::DISAMBIGUATE_ESCAPE_CODES
        | KittyKeyboardFlags::REPORT_EVENT_TYPES
        | KittyKeyboardFlags::REPORT_ALTERNATE_KEYS
        | KittyKeyboardFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES
        | KittyKeyboardFlags::REPORT_ASSOCIATED_TEXT;

    assert_eq!(
        make_event_with_raw(
            KeyEvent {
                key: KeyCode::Char('ф'),
                modifiers: Modifiers::CTRL,
                leds: KeyboardLedStatus::empty(),
                repeat_count: 1,
                key_is_down: true,
                raw: None,
                #[cfg(windows)]
                win32_uni_char: None,
            },
            Some(PhysKeyCode::A)
        )
        .encode_kitty(flags),
        "\x1b[1092::97;5;1092u".to_string()
    );

    assert_eq!(
        make_event_with_raw(
            KeyEvent {
                key: KeyCode::Char('Ф'),
                modifiers: Modifiers::CTRL | Modifiers::SHIFT,
                leds: KeyboardLedStatus::empty(),
                repeat_count: 1,
                key_is_down: true,
                raw: None,
                #[cfg(windows)]
                win32_uni_char: None,
            },
            Some(PhysKeyCode::A)
        )
        .encode_kitty(flags),
        "\x1b[1092:1060:97;6;1060u".to_string()
    );
}

#[test]
fn encode_issue_3526() {
    let flags = KittyKeyboardFlags::DISAMBIGUATE_ESCAPE_CODES;

    assert_eq!(
        KeyEvent {
            key: KeyCode::Char(' '),
            modifiers: Modifiers::NONE,
            leds: KeyboardLedStatus::NUM_LOCK,
            repeat_count: 1,
            key_is_down: true,
            raw: None,
            #[cfg(windows)]
            win32_uni_char: None,
        }
        .encode_kitty(flags),
        " ".to_string()
    );

    assert_eq!(
        KeyEvent {
            key: KeyCode::Char(' '),
            modifiers: Modifiers::NONE,
            leds: KeyboardLedStatus::CAPS_LOCK,
            repeat_count: 1,
            key_is_down: true,
            raw: None,
            #[cfg(windows)]
            win32_uni_char: None,
        }
        .encode_kitty(flags),
        " ".to_string()
    );

    assert_eq!(
        make_event_with_raw(
            KeyEvent {
                key: KeyCode::NumLock,
                modifiers: Modifiers::NONE,
                leds: KeyboardLedStatus::empty(),
                repeat_count: 1,
                key_is_down: true,
                raw: None,
                #[cfg(windows)]
                win32_uni_char: None,
            },
            Some(PhysKeyCode::NumLock)
        )
        .encode_kitty(flags),
        "".to_string()
    );

    assert_eq!(
        make_event_with_raw(
            KeyEvent {
                key: KeyCode::CapsLock,
                modifiers: Modifiers::NONE,
                leds: KeyboardLedStatus::empty(),
                repeat_count: 1,
                key_is_down: true,
                raw: None,
                #[cfg(windows)]
                win32_uni_char: None,
            },
            Some(PhysKeyCode::CapsLock)
        )
        .encode_kitty(flags),
        "".to_string()
    );
}

#[test]
fn encode_issue_4436() {
    let flags = KittyKeyboardFlags::DISAMBIGUATE_ESCAPE_CODES;

    assert_eq!(
        KeyEvent {
            key: KeyCode::Char('q'),
            modifiers: Modifiers::NONE,
            leds: KeyboardLedStatus::empty(),
            repeat_count: 1,
            key_is_down: true,
            raw: None,
            #[cfg(windows)]
            win32_uni_char: None,
        }
        .encode_kitty(flags),
        "q".to_string()
    );

    assert_eq!(
        KeyEvent {
            key: KeyCode::Char('f'),
            modifiers: Modifiers::SUPER,
            leds: KeyboardLedStatus::empty(),
            repeat_count: 1,
            key_is_down: true,
            raw: None,
            #[cfg(windows)]
            win32_uni_char: None,
        }
        .encode_kitty(flags),
        "\u{1b}[102;9u".to_string()
    );

    assert_eq!(
        KeyEvent {
            key: KeyCode::Char('f'),
            modifiers: Modifiers::SUPER | Modifiers::SHIFT,
            leds: KeyboardLedStatus::empty(),
            repeat_count: 1,
            key_is_down: true,
            raw: None,
            #[cfg(windows)]
            win32_uni_char: None,
        }
        .encode_kitty(flags),
        "\u{1b}[102;10u".to_string()
    );

    assert_eq!(
        KeyEvent {
            key: KeyCode::Char('f'),
            modifiers: Modifiers::SUPER | Modifiers::SHIFT | Modifiers::CTRL,
            leds: KeyboardLedStatus::empty(),
            repeat_count: 1,
            key_is_down: true,
            raw: None,
            #[cfg(windows)]
            win32_uni_char: None,
        }
        .encode_kitty(flags),
        "\u{1b}[102;14u".to_string()
    );
}

/// ESC with DISAMBIGUATE_ESCAPE_CODES must produce \x1b[27;1u, not a raw \x1b.
/// https://sw.kovidgoyal.net/kitty/keyboard-protocol/#disambiguate
#[test]
fn encode_escape_disambiguate() {
    // Flag 1 only: ESC on key-down → \x1b[27;1u
    let flags = KittyKeyboardFlags::DISAMBIGUATE_ESCAPE_CODES;
    assert_eq!(
        KeyEvent {
            key: KeyCode::Char('\x1b'),
            modifiers: Modifiers::NONE,
            leds: KeyboardLedStatus::empty(),
            repeat_count: 1,
            key_is_down: true,
            raw: None,
            #[cfg(windows)]
            win32_uni_char: None,
        }
        .encode_kitty(flags),
        "\x1b[27;1u".to_string()
    );

    // No flags at all: ESC on key-down must still be sent as a raw \x1b
    // (legacy behaviour is unchanged).
    let flags = KittyKeyboardFlags::NONE;
    assert_eq!(
        KeyEvent {
            key: KeyCode::Char('\x1b'),
            modifiers: Modifiers::NONE,
            leds: KeyboardLedStatus::empty(),
            repeat_count: 1,
            key_is_down: true,
            raw: None,
            #[cfg(windows)]
            win32_uni_char: None,
        }
        .encode_kitty(flags),
        "\x1b".to_string()
    );

    // DISAMBIGUATE + REPORT_EVENT_TYPES: key-up must produce \x1b[27;1:3u
    let flags =
        KittyKeyboardFlags::DISAMBIGUATE_ESCAPE_CODES | KittyKeyboardFlags::REPORT_EVENT_TYPES;
    assert_eq!(
        KeyEvent {
            key: KeyCode::Char('\x1b'),
            modifiers: Modifiers::NONE,
            leds: KeyboardLedStatus::empty(),
            repeat_count: 1,
            key_is_down: false,
            raw: None,
            #[cfg(windows)]
            win32_uni_char: None,
        }
        .encode_kitty(flags),
        "\x1b[27;1:3u".to_string()
    );
}
