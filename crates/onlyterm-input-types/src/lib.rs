#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

mod geometry;
pub use geometry::*;

mod keyboard_led_status;
pub use keyboard_led_status::*;

mod modifiers;
pub use modifiers::*;

mod ui_key_cap_rendering;
pub use ui_key_cap_rendering::*;

mod key_code;
pub use key_code::*;

mod phys_key_code;
pub use phys_key_code::*;

mod handled;
pub use handled::*;

mod mouse;
pub use mouse::*;

mod raw_key_event;
pub use raw_key_event::*;

mod kitty_keyboard_flags;
pub use kitty_keyboard_flags::*;

mod is_ascii_control;
pub use is_ascii_control::*;

mod ctrl_mapping;
pub use ctrl_mapping::*;

mod key_event;
pub use key_event::*;

mod window_decorations;
pub use window_decorations::*;

mod integrated_title_button;
pub use integrated_title_button::*;

mod integrated_title_button_alignment;
pub use integrated_title_button_alignment::*;

mod integrated_title_button_style;
pub use integrated_title_button_style::*;

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn encode_issue_3220() {
        let flags =
            KittyKeyboardFlags::DISAMBIGUATE_ESCAPE_CODES | KittyKeyboardFlags::REPORT_EVENT_TYPES;

        assert_eq!(
            KeyEvent {
                key: KeyCode::Char('o'),
                modifiers: Modifiers::NONE,
                leds: KeyboardLedStatus::empty(),
                repeat_count: 1,
                key_is_down: true,
                raw: None,
                #[cfg(windows)]
                win32_uni_char: None,
            }
            .encode_kitty(flags),
            "o".to_string()
        );
        assert_eq!(
            KeyEvent {
                key: KeyCode::Char('o'),
                modifiers: Modifiers::NONE,
                leds: KeyboardLedStatus::empty(),
                repeat_count: 1,
                key_is_down: false,
                raw: None,
                #[cfg(windows)]
                win32_uni_char: None,
            }
            .encode_kitty(flags),
            "\x1b[111;1:3u".to_string()
        );
    }

    #[test]
    fn encode_kitty_single_char_composed_matches_char() {
        // A `Composed` string containing exactly one char happens whenever
        // the OS delivers synthetic/IME-style text input with no backing
        // hardware key event (`raw: None`) -- for example the Windows emoji
        // picker (Win+.). Prior to this fix, `encode_kitty` had no match arm
        // for `Composed` at all, so it fell through to the catch-all `_` arm,
        // which requires `raw` to resolve a kitty function code; with `raw`
        // always `None` for this kind of input, the function silently
        // returned an empty string and the keystroke was dropped outright
        // under any protocol with REPORT_ALL_KEYS_AS_ESCAPE_CODES set.
        let flags = KittyKeyboardFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES;

        let composed = KeyEvent {
            key: KeyCode::Composed("\u{2615}".to_string()), // \u{2615} == HOT BEVERAGE
            modifiers: Modifiers::NONE,
            leds: KeyboardLedStatus::empty(),
            repeat_count: 1,
            key_is_down: true,
            raw: None,
            #[cfg(windows)]
            win32_uni_char: None,
        };
        let equivalent_char = KeyEvent {
            key: KeyCode::Char('\u{2615}'),
            ..composed.clone()
        };

        assert_eq!(composed.encode_kitty(flags), "\x1b[9749;1u".to_string());
        assert_eq!(
            composed.encode_kitty(flags),
            equivalent_char.encode_kitty(flags),
            "a single-char Composed key must encode identically to the equivalent Char key"
        );

        // With REPORT_ASSOCIATED_TEXT also negotiated, the associated-text
        // field should be present, matching the real CSI-u sequence OnlyTerm's
        // kitty keyboard protocol emits for this input in practice.
        let flags_with_text = flags | KittyKeyboardFlags::REPORT_ASSOCIATED_TEXT;
        assert_eq!(
            composed.encode_kitty(flags_with_text),
            "\x1b[9749;1;9749u".to_string()
        );
    }

    #[test]
    fn encode_kitty_multi_char_composed_is_unaffected() {
        // A genuinely multi-char Composed string (e.g. a ZWJ emoji sequence)
        // never reaches this function via OnlyTerm's normal routing -- it's
        // written directly to the pane as raw text in keyevent.rs, bypassing
        // Kitty encoding entirely. This test locks in that encode_kitty's own
        // behavior for that case is untouched by the single-char fix above.
        let flags = KittyKeyboardFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES;
        let multi = KeyEvent {
            key: KeyCode::Composed("ab".to_string()),
            modifiers: Modifiers::NONE,
            leds: KeyboardLedStatus::empty(),
            repeat_count: 1,
            key_is_down: true,
            raw: None,
            #[cfg(windows)]
            win32_uni_char: None,
        };
        assert_eq!(multi.encode_kitty(flags), String::new());
    }

    #[cfg(windows)]
    #[test]
    fn encode_win32_input_mode_ctrl_chords_carry_the_control_code() {
        // Regression test: `UnicodeChar` in the synthesized KEY_EVENT_RECORD
        // must carry the actual control code (eg. 0x0a for Ctrl+J), not the
        // bare physical letter -- conhost/OpenConsole (and anything reading
        // console input via ReadConsoleInputW, eg. Node's libuv) use that
        // field verbatim. A prior change that made the physical-key identity
        // layout-independent for CTRL/ALT chords accidentally carried the
        // bare letter into this field too.
        fn win32_input_mode_uni_char(c: char, phys: PhysKeyCode, vkey: u32, scan: u32) -> u32 {
            let event = KeyEvent {
                key: KeyCode::Char(c),
                modifiers: Modifiers::CTRL,
                leds: KeyboardLedStatus::empty(),
                repeat_count: 1,
                key_is_down: true,
                raw: Some(RawKeyEvent {
                    key: KeyCode::Char(c),
                    modifiers: Modifiers::CTRL,
                    leds: KeyboardLedStatus::empty(),
                    phys_code: Some(phys),
                    raw_code: vkey,
                    scan_code: scan,
                    repeat_count: 1,
                    key_is_down: true,
                    handled: Handled::new(),
                }),
                win32_uni_char: None,
            };
            let encoded = event.encode_win32_input_mode(false).expect("raw is Some");
            // Format is ESC [ vkey;scan;uni;down;ctrlstate;repeat _
            let uni_field = encoded
                .trim_start_matches('\u{1b}')
                .trim_start_matches('[')
                .trim_end_matches('_')
                .split(';')
                .nth(2)
                .expect("uni field present");
            uni_field.parse::<u32>().unwrap()
        }

        assert_eq!(
            win32_input_mode_uni_char('c', PhysKeyCode::C, 0x43, 0x2e),
            3,
            "Ctrl+C must carry 0x03, not 'c' (99)"
        );
        assert_eq!(
            win32_input_mode_uni_char('j', PhysKeyCode::J, 0x4a, 0x24),
            10,
            "Ctrl+J must carry 0x0a, not 'j' (106)"
        );
        assert_eq!(
            win32_input_mode_uni_char('v', PhysKeyCode::V, 0x56, 0x2f),
            22,
            "Ctrl+V must carry 0x16, not 'v' (118)"
        );
        assert_eq!(
            win32_input_mode_uni_char('\r', PhysKeyCode::Return, 0x0d, 0x1c),
            13,
            "Ctrl+Enter must carry 0x0d (the same control code as bare Enter, \
             exactly like a real Windows KEY_EVENT_RECORD reports it -- ctrl_mapping \
             only covers letter/punctuation keys, so falling back to '\\0' for a \
             char it doesn't recognize (as this used to) silently produced a \
             UnicodeChar of 0, an event no real Ctrl+Enter keypress ever sends"
        );
    }

    /// The `ctrl_letter_as_char` compatibility switch swaps the control code
    /// for the plain letter, and must do so ONLY when asked.
    ///
    /// Both halves are asserted deliberately. The "on" half is the fix for
    /// applications whose console reader re-derives the character from the
    /// virtual key under the active layout (Codex CLI; see
    /// docs/codex-cyrillic-ctrl-chords.md). The "off" half is the part that
    /// protects everyone else: this substitution was measured to break
    /// Claude Code, which reads the byte stream and receives this field
    /// verbatim, so a regression that made it unconditional would trade one
    /// broken application for another.
    #[cfg(windows)]
    #[test]
    fn encode_win32_input_mode_ctrl_letter_as_char_is_opt_in() {
        fn uni_char(c: char, phys: PhysKeyCode, vkey: u32, scan: u32, as_char: bool) -> u32 {
            let event = KeyEvent {
                key: KeyCode::Char(c),
                modifiers: Modifiers::CTRL,
                leds: KeyboardLedStatus::empty(),
                repeat_count: 1,
                key_is_down: true,
                raw: Some(RawKeyEvent {
                    key: KeyCode::Char(c),
                    modifiers: Modifiers::CTRL,
                    leds: KeyboardLedStatus::empty(),
                    phys_code: Some(phys),
                    raw_code: vkey,
                    scan_code: scan,
                    repeat_count: 1,
                    key_is_down: true,
                    handled: Handled::new(),
                }),
                win32_uni_char: None,
            };
            event
                .encode_win32_input_mode(as_char)
                .expect("raw is Some")
                .trim_start_matches('\u{1b}')
                .trim_start_matches('[')
                .trim_end_matches('_')
                .split(';')
                .nth(2)
                .expect("uni field present")
                .parse::<u32>()
                .unwrap()
        }

        assert_eq!(
            uni_char('j', PhysKeyCode::J, 0x4a, 0x24, true),
            0x6a,
            "with the switch on, Ctrl+J must carry the letter 'j' (0x6a) so that a \
             reader which only re-derives from the virtual key for UnicodeChar < 0x20 \
             takes it verbatim instead"
        );
        assert_eq!(
            uni_char('c', PhysKeyCode::C, 0x43, 0x2e, true),
            0x63,
            "with the switch on, Ctrl+C must carry the letter 'c' (0x63)"
        );

        assert_eq!(
            uni_char('j', PhysKeyCode::J, 0x4a, 0x24, false),
            0x0a,
            "with the switch off, the faithful control code must be unchanged"
        );
        assert_eq!(
            uni_char('c', PhysKeyCode::C, 0x43, 0x2e, false),
            0x03,
            "with the switch off, the faithful control code must be unchanged"
        );

        // Vk and Sc stay honest either way: they name the physical key, and
        // nothing about this switch may make them lie.
        let encoded_on = KeyEvent {
            key: KeyCode::Char('j'),
            modifiers: Modifiers::CTRL,
            leds: KeyboardLedStatus::empty(),
            repeat_count: 1,
            key_is_down: true,
            raw: Some(RawKeyEvent {
                key: KeyCode::Char('j'),
                modifiers: Modifiers::CTRL,
                leds: KeyboardLedStatus::empty(),
                phys_code: Some(PhysKeyCode::J),
                raw_code: 0x4a,
                scan_code: 0x24,
                repeat_count: 1,
                key_is_down: true,
                handled: Handled::new(),
            }),
            win32_uni_char: None,
        }
        .encode_win32_input_mode(true)
        .expect("raw is Some");
        assert_eq!(encoded_on, "\u{1b}[74;36;106;1;8;1_");
    }

    #[cfg(windows)]
    #[test]
    fn encode_win32_input_mode_alt_only_chords_carry_the_normalized_char() {
        // Regression test: ConPTY/OpenConsole re-derives UnicodeChar from
        // vkey+dwControlKeyState using the system's *current* keyboard
        // layout whenever it receives uni=0 -- a plain ALT-only chord does
        // NOT skip character synthesis the way CTRL does. Sending 0 here
        // (as this code used to, on the wrong assumption that ALT-only
        // never carries a character) let that re-derivation reintroduce
        // layout-dependence for passthrough shortcuts with no matching
        // keybinding, eg. Alt+V for "paste image" in Claude Code: under a
        // Russian layout the re-derived char came out as Cyrillic 'м'
        // (not matching), while under a US-like layout it happened to
        // re-derive as 'v' (matching) -- purely by layout coincidence, not
        // because OnlyTerm sent anything layout-dependent itself. Sending
        // the already-normalized `*c` (which key.rs's Windows handler set
        // to the physical key's US-layout character) removes ConPTY's own
        // re-derivation step from the picture entirely.
        let event = KeyEvent {
            key: KeyCode::Char('v'),
            modifiers: Modifiers::ALT,
            leds: KeyboardLedStatus::empty(),
            repeat_count: 1,
            key_is_down: true,
            raw: Some(RawKeyEvent {
                key: KeyCode::Char('v'),
                modifiers: Modifiers::ALT,
                leds: KeyboardLedStatus::empty(),
                phys_code: Some(PhysKeyCode::V),
                raw_code: 0x56,
                scan_code: 0x2f,
                repeat_count: 1,
                key_is_down: true,
                handled: Handled::new(),
            }),
            win32_uni_char: None,
        };
        let encoded = event.encode_win32_input_mode(false).expect("raw is Some");
        let uni_field = encoded
            .trim_start_matches('\u{1b}')
            .trim_start_matches('[')
            .trim_end_matches('_')
            .split(';')
            .nth(2)
            .expect("uni field present");
        assert_eq!(
            uni_field.parse::<u32>().unwrap(),
            'v' as u32,
            "Alt+V must carry 'v' (118), not 0, regardless of active keyboard layout"
        );
    }

    #[test]
    fn encode_kitty_ctrl_c_stays_legacy_under_disambiguate() {
        // Ctrl+C has no collision with any other named key's legacy byte,
        // so it must keep its traditional 0x03 encoding even when an app
        // has requested DISAMBIGUATE_ESCAPE_CODES - apps that only
        // understand the legacy byte for Ctrl+C (eg. to interrupt/exit)
        // must keep working alongside Enter-disambiguation.
        let flags = KittyKeyboardFlags::DISAMBIGUATE_ESCAPE_CODES;
        let ctrl_c = KeyEvent {
            key: KeyCode::Char('c'),
            modifiers: Modifiers::CTRL,
            leds: KeyboardLedStatus::empty(),
            repeat_count: 1,
            key_is_down: true,
            raw: None,
            #[cfg(windows)]
            win32_uni_char: None,
        };
        assert_eq!(ctrl_c.encode_kitty(flags), "\x03".to_string());
    }

    #[test]
    fn encode_kitty_ctrl_m_disambiguates_from_enter() {
        // Ctrl+M shares Enter's legacy byte (0x0D) - it must still be
        // escape-encoded under DISAMBIGUATE_ESCAPE_CODES so the two remain
        // distinguishable, unlike Ctrl+C.
        let flags = KittyKeyboardFlags::DISAMBIGUATE_ESCAPE_CODES;
        let ctrl_m = KeyEvent {
            key: KeyCode::Char('m'),
            modifiers: Modifiers::CTRL,
            leds: KeyboardLedStatus::empty(),
            repeat_count: 1,
            key_is_down: true,
            raw: None,
            #[cfg(windows)]
            win32_uni_char: None,
        };
        assert_ne!(ctrl_m.encode_kitty(flags), "\x0d".to_string());
        assert!(ctrl_m.encode_kitty(flags).starts_with("\x1b["));
    }

    #[test]
    fn encode_kitty_ctrl_enter_disambiguates_from_plain_enter() {
        // Enter itself is represented as KeyCode::Char('\r'); plain Enter
        // with no modifiers must stay as the raw byte, but Ctrl+Enter
        // (same KeyCode, CTRL held) must be escape-encoded so the two are
        // distinguishable - this is the actual feature enabling this
        // config option in the first place.
        let flags = KittyKeyboardFlags::DISAMBIGUATE_ESCAPE_CODES;
        let plain_enter = KeyEvent {
            key: KeyCode::Char('\r'),
            modifiers: Modifiers::NONE,
            leds: KeyboardLedStatus::empty(),
            repeat_count: 1,
            key_is_down: true,
            raw: None,
            #[cfg(windows)]
            win32_uni_char: None,
        };
        let ctrl_enter = KeyEvent {
            modifiers: Modifiers::CTRL,
            ..plain_enter.clone()
        };
        assert_eq!(plain_enter.encode_kitty(flags), "\r".to_string());
        assert!(ctrl_enter.encode_kitty(flags).starts_with("\x1b["));
        assert_ne!(
            plain_enter.encode_kitty(flags),
            ctrl_enter.encode_kitty(flags)
        );
    }

    #[test]
    fn encode_issue_3473() {
        let flags = KittyKeyboardFlags::DISAMBIGUATE_ESCAPE_CODES
            | KittyKeyboardFlags::REPORT_EVENT_TYPES
            | KittyKeyboardFlags::REPORT_ALTERNATE_KEYS
            | KittyKeyboardFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES;

        assert_eq!(
            KeyEvent {
                key: KeyCode::Function(1),
                modifiers: Modifiers::NONE,
                leds: KeyboardLedStatus::empty(),
                repeat_count: 1,
                key_is_down: true,
                raw: None,
                #[cfg(windows)]
                win32_uni_char: None,
            }
            .encode_kitty(flags),
            "\x1b[11;1~".to_string()
        );
        assert_eq!(
            KeyEvent {
                key: KeyCode::Function(1),
                modifiers: Modifiers::NONE,
                leds: KeyboardLedStatus::empty(),
                repeat_count: 1,
                key_is_down: false,
                raw: None,
                #[cfg(windows)]
                win32_uni_char: None,
            }
            .encode_kitty(flags),
            "\x1b[11;1:3~".to_string()
        );
    }

    #[test]
    fn encode_issue_2546() {
        let flags = KittyKeyboardFlags::DISAMBIGUATE_ESCAPE_CODES;

        assert_eq!(
            KeyEvent {
                key: KeyCode::Char('i'),
                modifiers: Modifiers::ALT | Modifiers::SHIFT,
                leds: KeyboardLedStatus::empty(),
                repeat_count: 1,
                key_is_down: true,
                raw: None,
                #[cfg(windows)]
                win32_uni_char: None,
            }
            .encode_kitty(flags),
            "\x1b[105;4u".to_string()
        );
        assert_eq!(
            KeyEvent {
                key: KeyCode::Char('I'),
                modifiers: Modifiers::ALT | Modifiers::SHIFT,
                leds: KeyboardLedStatus::empty(),
                repeat_count: 1,
                key_is_down: true,
                raw: None,
                #[cfg(windows)]
                win32_uni_char: None,
            }
            .encode_kitty(flags),
            "\x1b[105;4u".to_string()
        );

        assert_eq!(
            KeyEvent {
                key: KeyCode::Char('1'),
                modifiers: Modifiers::ALT | Modifiers::SHIFT,
                leds: KeyboardLedStatus::empty(),
                repeat_count: 1,
                key_is_down: true,
                raw: None,
                #[cfg(windows)]
                win32_uni_char: None,
            }
            .encode_kitty(flags),
            "\x1b[49;4u".to_string()
        );

        assert_eq!(
            make_event_with_raw(
                KeyEvent {
                    key: KeyCode::Char('!'),
                    modifiers: Modifiers::ALT | Modifiers::SHIFT,
                    leds: KeyboardLedStatus::empty(),
                    repeat_count: 1,
                    key_is_down: true,
                    raw: None,
                    #[cfg(windows)]
                    win32_uni_char: None,
                },
                Some(PhysKeyCode::K1)
            )
            .encode_kitty(flags),
            "\x1b[49;4u".to_string()
        );

        assert_eq!(
            KeyEvent {
                key: KeyCode::Char('i'),
                modifiers: Modifiers::SHIFT | Modifiers::CTRL,
                leds: KeyboardLedStatus::empty(),
                repeat_count: 1,
                key_is_down: true,
                raw: None,
                #[cfg(windows)]
                win32_uni_char: None,
            }
            .encode_kitty(flags),
            "\x1b[105;6u".to_string()
        );
        assert_eq!(
            KeyEvent {
                key: KeyCode::Char('I'),
                modifiers: Modifiers::SHIFT | Modifiers::CTRL,
                leds: KeyboardLedStatus::empty(),
                repeat_count: 1,
                key_is_down: true,
                raw: None,
                #[cfg(windows)]
                win32_uni_char: None,
            }
            .encode_kitty(flags),
            "\x1b[105;6u".to_string()
        );

        assert_eq!(
            KeyEvent {
                key: KeyCode::Char('I'),
                modifiers: Modifiers::CTRL,
                leds: KeyboardLedStatus::empty(),
                repeat_count: 1,
                key_is_down: true,
                raw: Some(RawKeyEvent {
                    key: KeyCode::Char('I'),
                    modifiers: Modifiers::SHIFT | Modifiers::CTRL,
                    handled: Handled::new(),
                    key_is_down: true,
                    raw_code: 0,
                    leds: KeyboardLedStatus::empty(),
                    phys_code: Some(PhysKeyCode::I),
                    #[cfg(windows)]
                    scan_code: 0,
                    repeat_count: 1,
                }),
                #[cfg(windows)]
                win32_uni_char: None,
            }
            .encode_kitty(flags),
            "\x1b[105;6u".to_string()
        );

        assert_eq!(
            KeyEvent {
                key: KeyCode::Char('i'),
                modifiers: Modifiers::ALT | Modifiers::SHIFT | Modifiers::CTRL,
                leds: KeyboardLedStatus::empty(),
                repeat_count: 1,
                key_is_down: true,
                raw: None,
                #[cfg(windows)]
                win32_uni_char: None,
            }
            .encode_kitty(flags),
            "\x1b[105;8u".to_string()
        );
        assert_eq!(
            KeyEvent {
                key: KeyCode::Char('I'),
                modifiers: Modifiers::ALT | Modifiers::SHIFT | Modifiers::CTRL,
                leds: KeyboardLedStatus::empty(),
                repeat_count: 1,
                key_is_down: true,
                raw: None,
                #[cfg(windows)]
                win32_uni_char: None,
            }
            .encode_kitty(flags),
            "\x1b[105;8u".to_string()
        );

        assert_eq!(
            KeyEvent {
                key: KeyCode::Char('\x08'),
                modifiers: Modifiers::NONE,
                leds: KeyboardLedStatus::empty(),
                repeat_count: 1,
                key_is_down: true,
                raw: None,
                #[cfg(windows)]
                win32_uni_char: None,
            }
            .encode_kitty(flags),
            "\x7f".to_string()
        );

        assert_eq!(
            KeyEvent {
                key: KeyCode::Char('\x08'),
                modifiers: Modifiers::CTRL,
                leds: KeyboardLedStatus::empty(),
                repeat_count: 1,
                key_is_down: true,
                raw: None,
                #[cfg(windows)]
                win32_uni_char: None,
            }
            .encode_kitty(flags),
            "\x1b[127;5u".to_string()
        );
    }

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

    fn make_event_with_raw(mut event: KeyEvent, phys: Option<PhysKeyCode>) -> KeyEvent {
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
}
