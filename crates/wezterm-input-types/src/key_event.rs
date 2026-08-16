use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt::Write;

use crate::ctrl_mapping::ctrl_mapping;
use crate::key_code::normalize_shift;
use crate::{KeyCode, KeyboardLedStatus, KittyKeyboardFlags, Modifiers, PhysKeyCode, RawKeyEvent};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyEvent {
    /// Which key was pressed.
    /// This is the potentially processed/composed version
    /// of the input.
    pub key: KeyCode,
    /// Which modifiers are down
    pub modifiers: Modifiers,

    pub leds: KeyboardLedStatus,

    /// How many times this key repeats
    pub repeat_count: u16,

    /// If true, this is a key down rather than a key up event
    pub key_is_down: bool,

    /// If triggered from a raw key event, here it is.
    pub raw: Option<RawKeyEvent>,

    #[cfg(windows)]
    pub win32_uni_char: Option<char>,
}

fn normalize_ctrl(key: KeyCode, modifiers: Modifiers) -> (KeyCode, Modifiers) {
    if modifiers.contains(Modifiers::CTRL) {
        if let KeyCode::Char(c) = key {
            if (c as u32) < 0x20 {
                let de_ctrl = ((c as u8) | 0x40) as char;
                return (KeyCode::Char(de_ctrl.to_ascii_lowercase()), modifiers);
            }
        }
    }
    (key, modifiers)
}

impl KeyEvent {
    /// if SHIFT is held and we have KeyCode::Char('c') we want to normalize
    /// that keycode to KeyCode::Char('C'); that is what this function does.
    pub fn normalize_shift(mut self) -> Self {
        let (key, modifiers) = normalize_shift(self.key, self.modifiers);
        self.key = key;
        self.modifiers = modifiers;

        self
    }

    /// If the key code is a modifier key (Control, Alt, Shift), check
    /// the underlying raw event to see if we had a positional version
    /// of that key.
    /// If so, switch to the positional version.
    pub fn resurface_positional_modifier_key(mut self) -> Self {
        match self.key {
            KeyCode::Control
                if matches!(
                    self.raw,
                    Some(RawKeyEvent {
                        key: KeyCode::LeftControl | KeyCode::Physical(PhysKeyCode::LeftControl),
                        ..
                    })
                ) =>
            {
                self.key = KeyCode::LeftControl;
            }
            KeyCode::Control
                if matches!(
                    self.raw,
                    Some(RawKeyEvent {
                        key: KeyCode::RightControl | KeyCode::Physical(PhysKeyCode::RightControl),
                        ..
                    })
                ) =>
            {
                self.key = KeyCode::RightControl;
            }
            KeyCode::Alt
                if matches!(
                    self.raw,
                    Some(RawKeyEvent {
                        key: KeyCode::LeftAlt | KeyCode::Physical(PhysKeyCode::LeftAlt),
                        ..
                    })
                ) =>
            {
                self.key = KeyCode::LeftAlt;
            }
            KeyCode::Alt
                if matches!(
                    self.raw,
                    Some(RawKeyEvent {
                        key: KeyCode::RightAlt | KeyCode::Physical(PhysKeyCode::RightAlt),
                        ..
                    })
                ) =>
            {
                self.key = KeyCode::RightAlt;
            }
            KeyCode::Shift
                if matches!(
                    self.raw,
                    Some(RawKeyEvent {
                        key: KeyCode::LeftShift | KeyCode::Physical(PhysKeyCode::LeftShift),
                        ..
                    })
                ) =>
            {
                self.key = KeyCode::LeftShift;
            }
            KeyCode::Shift
                if matches!(
                    self.raw,
                    Some(RawKeyEvent {
                        key: KeyCode::RightShift | KeyCode::Physical(PhysKeyCode::RightShift),
                        ..
                    })
                ) =>
            {
                self.key = KeyCode::RightShift;
            }
            _ => {}
        }

        self
    }

    /// If CTRL is held down and we have KeyCode::Char(_) with the
    /// ASCII control value encoded, decode it back to the ASCII
    /// alpha keycode instead.
    pub fn normalize_ctrl(mut self) -> Self {
        let (key, modifiers) = normalize_ctrl(self.key, self.modifiers);
        self.key = key;
        self.modifiers = modifiers;

        self
    }

    /// <https://github.com/microsoft/terminal/blob/main/doc/specs/%234999%20-%20Improved%20keyboard%20handling%20in%20Conpty.md>
    #[cfg(windows)]
    /// `ctrl_letter_as_char` is an application-compatibility switch decided
    /// by the caller from the pane's foreground process; see the comment on
    /// its use in the `KeyCode::Char` arm below. Pass `false` for the
    /// faithful encoding.
    pub fn encode_win32_input_mode(&self, ctrl_letter_as_char: bool) -> Option<String> {
        let phys = self.raw.as_ref()?;

        let vkey = phys.raw_code;
        let scan_code = phys.scan_code;
        // <https://docs.microsoft.com/en-us/windows/console/key-event-record-str>
        // defines the dwControlKeyState values
        let mut control_key_state = 0;
        const SHIFT_PRESSED: usize = 0x10;
        const ENHANCED_KEY: usize = 0x100;
        const RIGHT_ALT_PRESSED: usize = 0x01;
        const LEFT_ALT_PRESSED: usize = 0x02;
        const LEFT_CTRL_PRESSED: usize = 0x08;
        const RIGHT_CTRL_PRESSED: usize = 0x04;

        if self
            .modifiers
            .intersects(Modifiers::SHIFT | Modifiers::LEFT_SHIFT | Modifiers::RIGHT_SHIFT)
        {
            control_key_state |= SHIFT_PRESSED;
        }

        if self.modifiers.contains(Modifiers::RIGHT_ALT) {
            control_key_state |= RIGHT_ALT_PRESSED;
        } else if self.modifiers.contains(Modifiers::ALT) {
            control_key_state |= LEFT_ALT_PRESSED;
        }
        if self.modifiers.contains(Modifiers::LEFT_ALT) {
            control_key_state |= LEFT_ALT_PRESSED;
        }
        if self.modifiers.contains(Modifiers::RIGHT_CTRL) {
            control_key_state |= RIGHT_CTRL_PRESSED;
        } else if self.modifiers.contains(Modifiers::CTRL) {
            control_key_state |= LEFT_CTRL_PRESSED;
        }
        if self.modifiers.contains(Modifiers::LEFT_CTRL) {
            control_key_state |= LEFT_CTRL_PRESSED;
        }
        if self.modifiers.contains(Modifiers::ENHANCED_KEY) {
            control_key_state |= ENHANCED_KEY;
        }

        let key_down = if self.key_is_down { 1 } else { 0 };

        match &self.key {
            KeyCode::Composed(_) => None,
            KeyCode::Char(c) => {
                // CTRL and (non-AltGr) ALT combinations are keybinding
                // modifiers, not text input: `self.key` was already resolved
                // to the layout-independent physical key identity for these
                // (see the equivalent logic in the Windows `key()` handler),
                // so prefer that over `win32_uni_char`, which is always the
                // raw `ToUnicode` result and stays layout-translated (eg.
                // physical "V" still reports Cyrillic 'м' under a Russian
                // layout) even when a modifier is held.
                //
                // The `UnicodeChar` field of the KEY_EVENT_RECORD still has
                // to carry the actual control code (eg. 0x0a for Ctrl+J),
                // not the bare physical letter -- conhost/OpenConsole (and
                // anything reading console input, eg. Node's libuv) use that
                // field verbatim. `ctrl_mapping` derives the control code
                // from the layout-independent physical letter, so this keeps
                // both properties: layout independence and a correct
                // control byte.
                //
                // Plain ALT-only chords used to send `uni=0` here on the
                // (empirically wrong) assumption that a real KEY_EVENT_RECORD
                // never carries a character for those. Measured behavior
                // says otherwise: with `uni=0`, ConPTY/OpenConsole re-derives
                // `UnicodeChar` itself from `vkey`+`dwControlKeyState` using
                // the system's *current* keyboard layout -- so eg. Alt+V's
                // passthrough to a child process (a shortcut with no
                // matching keybinding, eg. Claude Code's paste-image chord)
                // came out layout-dependent again despite `vkey` itself
                // being correct in both cases: under a Russian layout the
                // re-derived character was Cyrillic 'м', so an app matching
                // on the character (not just the VK code) never recognized
                // it as Alt+V, while under a US-like layout it happened to
                // re-derive as 'v' and work. Sending the already
                // layout-normalized `*c` here (instead of 0) tells ConPTY
                // the character directly, so it has nothing left to
                // re-derive from the live layout.
                let prefer_physical = self.modifiers.contains(Modifiers::CTRL)
                    || self.modifiers.contains(Modifiers::ALT);
                let uni = if prefer_physical {
                    if self.modifiers.contains(Modifiers::CTRL) {
                        // `ctrl_mapping` only covers letter/punctuation keys
                        // (it derives the classic Ctrl+<letter> control byte,
                        // eg. 0x0a for Ctrl+J) -- it does not know `*c` when
                        // `*c` is already a control character itself, eg.
                        // '\r' for the synthetic Ctrl+Enter event
                        // `SendEnterOrNewline` constructs (see
                        // termwindow/actions.rs). A real physical Ctrl+Enter
                        // keypress still reports UnicodeChar = 0x0d in a
                        // genuine Windows KEY_EVENT_RECORD (the same byte as
                        // bare Enter -- Ctrl held or not doesn't change CR's
                        // own control code, exactly like Ctrl+M), so falling
                        // back to the character itself here matches real
                        // console behavior instead of the previous
                        // `unwrap_or(0)`, which sent UnicodeChar = 0 --
                        // "no character at all" -- for any Ctrl+<char> whose
                        // char isn't a letter ctrl_mapping recognizes. That
                        // false "no character" record is what a real Ctrl+
                        // Enter never actually produces, and is a plausible
                        // reason a receiving app fails to recognize the
                        // record as a modified-Enter at all.
                        // `ctrl_letter_as_char` sends the base letter (0x6A
                        // 'j') here instead of the control code (0x0A), for
                        // the narrow set of applications whose console
                        // reader ignores this field when it is below 0x20
                        // and re-derives the character from the virtual key
                        // via ToUnicodeEx under the active layout -- which
                        // resolves VK_J to 'о' under a Cyrillic layout and
                        // loses the chord. Measured: with the letter here,
                        // Codex CLI accepts Ctrl+J/Ctrl+Enter/Ctrl+C under a
                        // Cyrillic layout; with the control code it does
                        // not.
                        //
                        // It is off by default because it is wrong in
                        // general -- a genuine KEY_EVENT_RECORD carries the
                        // control code, conhost passes this field to
                        // byte-stream readers verbatim, and Claude Code was
                        // measured to break (typing a literal 'j') the
                        // moment it receives the letter. The caller decides
                        // per pane; see
                        // `Config::win32_input_ctrl_letter_as_char_processes`.
                        if ctrl_letter_as_char {
                            *c as u32
                        } else {
                            ctrl_mapping(*c).map(|c| c as u32).unwrap_or(*c as u32)
                        }
                    } else {
                        *c as u32
                    }
                } else {
                    self.win32_uni_char.unwrap_or(*c) as u32
                };
                Some(format!(
                    "\u{1b}[{};{};{};{};{};{}_",
                    vkey, scan_code, uni, key_down, control_key_state, self.repeat_count
                ))
            }
            _ => {
                let uni = 0;
                Some(format!(
                    "\u{1b}[{};{};{};{};{};{}_",
                    vkey, scan_code, uni, key_down, control_key_state, self.repeat_count
                ))
            }
        }
    }

    pub fn encode_kitty(&self, flags: KittyKeyboardFlags) -> String {
        use KeyCode::*;

        // `Physical(_)` has no dedicated arm below either, for the same
        // reason `Composed` doesn't (see next comment): it falls through to
        // the `_` catch-all and silently produces no output unless
        // REPORT_ALL_KEYS_AS_ESCAPE_CODES is set. The Windows key handler
        // resolves Ctrl/Alt+<letter> chords to the layout-independent
        // physical key for keybinding matching (so bindings resolve the
        // same way regardless of active keyboard layout) even when no
        // binding actually claims the chord, so an unbound eg. Alt+V never
        // reaches here as a plain `Char('v')`. Normalize it back to the
        // character/named key a raw hardware keypress would report, so the
        // passthrough encoding below still fires.
        if let Physical(phys) = &self.key {
            let mut normalized = self.clone();
            normalized.key = phys.to_key_code();
            return normalized.encode_kitty(flags);
        }

        // A `Composed` string containing exactly one char is functionally a
        // plain character keypress (delivered by an IME, or e.g. the Windows
        // emoji picker). It never carries a `raw` hardware key event, and
        // unlike every other key it has no dedicated arm below, so it falls
        // through to the final `_` catch-all, which requires `raw` to resolve
        // a kitty function code. With no raw event that's always `None`, so
        // the whole function silently returns an empty string and the
        // keypress is dropped outright under Kitty protocols with
        // REPORT_ALL_KEYS_AS_ESCAPE_CODES set. Normalize it to `Char` up
        // front so the rest of this function treats it like any other
        // single character; a genuinely multi-char Composed string (e.g. a
        // ZWJ emoji sequence) is unaffected and keeps falling through as
        // before, exactly as-is, since it never reaches encode_kitty via the
        // Composed match arm in keyevent.rs anyway.
        if let Composed(s) = &self.key {
            let mut chars = s.chars();
            if let (Some(c), None) = (chars.next(), chars.next()) {
                let mut normalized = self.clone();
                normalized.key = Char(c);
                return normalized.encode_kitty(flags);
            }
        }

        if !flags.contains(KittyKeyboardFlags::REPORT_EVENT_TYPES) && !self.key_is_down {
            return String::new();
        }

        if self.modifiers.is_empty()
            && !flags.contains(KittyKeyboardFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES)
            && self.key_is_down
        {
            // Check for simple text generating keys
            match &self.key {
                Char('\x08') => return '\x7f'.to_string(),
                Char('\x7f') => return '\x08'.to_string(),
                // With DISAMBIGUATE_ESCAPE_CODES, ESC must not be sent as a
                // raw \x1b byte — the entire point of the flag is to eliminate
                // that ambiguity.  Let it fall through to the CSI-u path below
                // which will produce \x1b[27;1u as required by the spec.
                // https://sw.kovidgoyal.net/kitty/keyboard-protocol/#disambiguate
                Char('\x1b') if flags.contains(KittyKeyboardFlags::DISAMBIGUATE_ESCAPE_CODES) => {}
                Char(c) => return c.to_string(),
                _ => {}
            }
        }

        let raw_modifiers = self
            .raw
            .as_ref()
            .map(|raw| raw.modifiers)
            .unwrap_or(self.modifiers);

        let mut modifiers = 0;
        if raw_modifiers.contains(Modifiers::SHIFT) {
            modifiers |= 1;
        }
        if raw_modifiers.contains(Modifiers::ALT) {
            modifiers |= 2;
        }
        if raw_modifiers.contains(Modifiers::CTRL) {
            modifiers |= 4;
        }
        if raw_modifiers.contains(Modifiers::SUPER) {
            modifiers |= 8;
        }
        // TODO: Hyper and Meta are not handled yet.
        // We should somehow detect this?
        // See: https://github.com/wezterm/wezterm/pull/4605#issuecomment-1823604708
        if self.leds.contains(KeyboardLedStatus::CAPS_LOCK) {
            modifiers |= 64;
        }
        if self.leds.contains(KeyboardLedStatus::NUM_LOCK) {
            modifiers |= 128;
        }
        modifiers += 1;

        let event_type =
            if flags.contains(KittyKeyboardFlags::REPORT_EVENT_TYPES) && !self.key_is_down {
                ":3"
            } else {
                ""
            };

        let is_legacy_key = match &self.key {
            Char(c) => c.is_ascii_alphanumeric() || c.is_ascii_punctuation(),
            _ => false,
        };

        let generated_text =
            if self.key_is_down && flags.contains(KittyKeyboardFlags::REPORT_ASSOCIATED_TEXT) {
                match &self.key {
                    Char(c) => format!(";{}", *c as u32),
                    KeyCode::Numpad(n) => format!(";{}", '0' as u32 + *n as u32),
                    Composed(s) => {
                        let mut codepoints = ";".to_string();
                        for c in s.chars() {
                            if codepoints.len() > 1 {
                                codepoints.push(':');
                            }
                            write!(&mut codepoints, "{}", c as u32).ok();
                        }
                        codepoints
                    }
                    _ => String::new(),
                }
            } else {
                String::new()
            };

        let guess_phys = self
            .raw
            .as_ref()
            .and_then(|raw| raw.phys_code)
            .or_else(|| self.key.to_phys());

        let is_numpad = guess_phys.and_then(|phys| match phys {
                PhysKeyCode::Keypad0
                | PhysKeyCode::Keypad1
                | PhysKeyCode::Keypad2
                | PhysKeyCode::Keypad3
                | PhysKeyCode::Keypad4
                | PhysKeyCode::Keypad5
                | PhysKeyCode::Keypad6
                | PhysKeyCode::Keypad7
                | PhysKeyCode::Keypad8
                | PhysKeyCode::Keypad9
                // | PhysKeyCode::KeypadClear not a physical numpad key?
                | PhysKeyCode::KeypadDecimal
                | PhysKeyCode::KeypadDelete
                | PhysKeyCode::KeypadDivide
                | PhysKeyCode::KeypadEnter
                | PhysKeyCode::KeypadEquals
                | PhysKeyCode::KeypadSubtract
                | PhysKeyCode::KeypadMultiply
                | PhysKeyCode::KeypadAdd
             => Some(phys),
            _ => None,
        });

        if let Some(numpad) = is_numpad {
            let code = match (numpad, self.leds.contains(KeyboardLedStatus::NUM_LOCK)) {
                (PhysKeyCode::Keypad0, true) => 57399,
                (PhysKeyCode::Keypad0, false) => 57425,
                (PhysKeyCode::Keypad1, true) => 57400,
                (PhysKeyCode::Keypad1, false) => 57424,
                (PhysKeyCode::Keypad2, true) => 57401,
                (PhysKeyCode::Keypad2, false) => 57420,
                (PhysKeyCode::Keypad3, true) => 57402,
                (PhysKeyCode::Keypad3, false) => 57422,
                (PhysKeyCode::Keypad4, true) => 57403,
                (PhysKeyCode::Keypad4, false) => 57417,
                (PhysKeyCode::Keypad5, true) => 57404,
                (PhysKeyCode::Keypad5, false) => {
                    let xt_mods = self.modifiers.encode_xterm();
                    return if xt_mods == 0 && self.key_is_down {
                        "\x1b[E".to_string()
                    } else {
                        format!("\x1b[1;{}{event_type}E", 1 + xt_mods)
                    };
                }
                (PhysKeyCode::Keypad6, true) => 57405,
                (PhysKeyCode::Keypad6, false) => 57418,
                (PhysKeyCode::Keypad7, true) => 57406,
                (PhysKeyCode::Keypad7, false) => 57423,
                (PhysKeyCode::Keypad8, true) => 57407,
                (PhysKeyCode::Keypad8, false) => 57419,
                (PhysKeyCode::Keypad9, true) => 57408,
                (PhysKeyCode::Keypad9, false) => 57421,
                (PhysKeyCode::KeypadDecimal, _) => 57409,
                (PhysKeyCode::KeypadDelete, _) => 57426,
                (PhysKeyCode::KeypadDivide, _) => 57410,
                (PhysKeyCode::KeypadEnter, _) => 57414,
                (PhysKeyCode::KeypadEquals, _) => 57415,
                (PhysKeyCode::KeypadSubtract, _) => 57412,
                (PhysKeyCode::KeypadMultiply, _) => 57411,
                (PhysKeyCode::KeypadAdd, _) => 57413,
                _ => unreachable!(),
            };
            return format!("\x1b[{code};{modifiers}{event_type}{generated_text}u");
        }

        match &self.key {
            PageUp | PageDown | Insert | Char('\x7f') => {
                let c = match &self.key {
                    Insert => 2,
                    Char('\x7f') => 3, // Delete
                    PageUp => 5,
                    PageDown => 6,
                    _ => unreachable!(),
                };

                format!("\x1b[{c};{modifiers}{event_type}~")
            }
            Char(shifted_key) => {
                let shifted_key = if *shifted_key == '\x08' {
                    // Backspace is really VERASE -> ASCII DEL
                    '\x7f'
                } else {
                    *shifted_key
                };

                // Per the kitty spec, DISAMBIGUATE_ESCAPE_CODES alone is
                // enough to require CSI-u encoding for every Ctrl+<letter>
                // combo. In practice, only combos whose legacy control byte
                // collides with a dedicated named key (Ctrl+H = Backspace's
                // 0x08, Ctrl+I = Tab's 0x09, Ctrl+M = Enter's 0x0D, Ctrl+[ =
                // Escape's 0x1B, Ctrl+8/Ctrl+? = Delete's 0x7F) are actually
                // ambiguous - everything else (eg. Ctrl+C, Ctrl+D, Ctrl+A)
                // has always meant exactly one thing and plenty of
                // real-world software (readline, and at least one CLI tool
                // tested against OnlyTerm) only understands the legacy byte
                // for those, not the CSI-u form. Keep legacy encoding for
                // the unambiguous combos even when an app has requested
                // disambiguation, so eg. Ctrl+C still works alongside an
                // app also using this protocol to distinguish Ctrl+Enter
                // from a plain Enter.
                let ctrl_collides_with_named_key = matches!(
                    shifted_key.to_ascii_lowercase(),
                    'h' | 'i' | 'm' | '[' | '{' | '3' | '8' | '?'
                );

                let use_legacy = !flags.contains(KittyKeyboardFlags::REPORT_ALTERNATE_KEYS)
                    && event_type.is_empty()
                    && is_legacy_key
                    && !(flags.contains(KittyKeyboardFlags::DISAMBIGUATE_ESCAPE_CODES)
                        && ((self.modifiers.contains(Modifiers::CTRL)
                            && ctrl_collides_with_named_key)
                            || self.modifiers.contains(Modifiers::ALT)))
                    && !self.modifiers.intersects(
                        Modifiers::SUPER, /* TODO: Hyper and Meta should be added here. */
                    );

                if use_legacy {
                    // Legacy text key
                    // https://sw.kovidgoyal.net/kitty/keyboard-protocol/#legacy-text-keys
                    let mut output = String::new();
                    if self.modifiers.contains(Modifiers::ALT) {
                        output.push('\x1b');
                    }

                    if self.modifiers.contains(Modifiers::CTRL) {
                        csi_u_encode(
                            &mut output,
                            shifted_key.to_ascii_uppercase(),
                            self.modifiers,
                        );
                    } else {
                        output.push(shifted_key);
                    }

                    return output;
                }

                // FIXME: ideally we'd get the correct unshifted key from
                // the OS based on the current keyboard layout. That needs
                // more plumbing, so for now, we're assuming the US layout.
                let c = us_layout_unshift(shifted_key);

                let base_layout = self
                    .raw
                    .as_ref()
                    .and_then(|raw| raw.phys_code.as_ref())
                    .and_then(|phys| match phys.to_key_code() {
                        KeyCode::Char(base) if base != c => Some(base),
                        _ => None,
                    });

                let mut key_code = format!("{}", (c as u32));

                if flags.contains(KittyKeyboardFlags::REPORT_ALTERNATE_KEYS)
                    && (c != shifted_key || base_layout.is_some())
                {
                    key_code.push(':');
                    if c != shifted_key {
                        key_code.push_str(&format!("{}", (shifted_key as u32)));
                    }
                    if let Some(base) = base_layout {
                        key_code.push_str(&format!(":{}", (base as u32)));
                    }
                }

                format!("\x1b[{key_code};{modifiers}{event_type}{generated_text}u")
            }
            LeftArrow | RightArrow | UpArrow | DownArrow | Home | End => {
                let c = match &self.key {
                    UpArrow => 'A',
                    DownArrow => 'B',
                    RightArrow => 'C',
                    LeftArrow => 'D',
                    Home => 'H',
                    End => 'F',
                    _ => unreachable!(),
                };
                format!("\x1b[1;{modifiers}{event_type}{c}")
            }
            Function(n) if *n < 25 => {
                // The spec says that kitty prefers an SS3 form for F1-F4,
                // but then has some variance in the encoding and cites a
                // compatibility issue with a cursor position report.
                // Since it allows reporting these all unambiguously with
                // the same general scheme, that is what we're using here.
                let intro = match *n {
                    1 => "\x1b[11",
                    2 => "\x1b[12",
                    3 => "\x1b[13",
                    4 => "\x1b[14",
                    5 => "\x1b[15",
                    6 => "\x1b[17",
                    7 => "\x1b[18",
                    8 => "\x1b[19",
                    9 => "\x1b[20",
                    10 => "\x1b[21",
                    11 => "\x1b[23",
                    12 => "\x1b[24",
                    13 => "\x1b[57376",
                    14 => "\x1b[57377",
                    15 => "\x1b[57378",
                    16 => "\x1b[57379",
                    17 => "\x1b[57380",
                    18 => "\x1b[57381",
                    19 => "\x1b[57382",
                    20 => "\x1b[57383",
                    21 => "\x1b[57384",
                    22 => "\x1b[57385",
                    23 => "\x1b[57386",
                    24 => "\x1b[57387",
                    _ => unreachable!(),
                };
                // for F1-F12 the spec says we should terminate with ~
                // for F13 and up the spec says we should terminate with u
                let end_char = if *n < 13 { '~' } else { 'u' };

                format!("{intro};{modifiers}{event_type}{end_char}")
            }

            _ => {
                let code = self.raw.as_ref().and_then(|raw| raw.kitty_function_code());

                match (
                    code,
                    flags.contains(KittyKeyboardFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES),
                ) {
                    (Some(code), true) => {
                        format!("\x1b[{code};{modifiers}{event_type}{generated_text}u")
                    }
                    _ => String::new(),
                }
            }
        }
    }
}

fn csi_u_encode(buf: &mut String, c: char, mods: Modifiers) {
    let c = if mods.contains(Modifiers::CTRL) && ctrl_mapping(c).is_some() {
        ctrl_mapping(c).unwrap()
    } else {
        c
    };
    if mods.contains(Modifiers::ALT) {
        buf.push(0x1b as char);
    }
    write!(buf, "{}", c).ok();
}

/// Kitty wants us to report the un-shifted version of a key.
/// It's a PITA to obtain that from the OS-dependent keyboard
/// layout stuff. For the moment, we'll do the slightly gross
/// thing and make an assumption that a US ANSI layout is in
/// use; this function encodes that mapping.
fn us_layout_unshift(c: char) -> char {
    match c {
        '!' => '1',
        '@' => '2',
        '#' => '3',
        '$' => '4',
        '%' => '5',
        '^' => '6',
        '&' => '7',
        '*' => '8',
        '(' => '9',
        ')' => '0',
        '_' => '-',
        '+' => '=',
        '~' => '`',
        '{' => '[',
        '}' => ']',
        '|' => '\\',
        ':' => ';',
        '"' => '\'',
        '<' => ',',
        '>' => '.',
        '?' => '/',
        c => {
            let s: Vec<char> = c.to_lowercase().collect();
            if s.len() == 1 {
                s[0]
            } else {
                c
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PhysKeyCode;

    fn make_event(key: KeyCode, modifiers: Modifiers) -> KeyEvent {
        KeyEvent {
            key,
            modifiers,
            leds: KeyboardLedStatus::empty(),
            repeat_count: 1,
            key_is_down: true,
            raw: None,
            #[cfg(windows)]
            win32_uni_char: None,
        }
    }

    #[test]
    fn unbound_alt_physical_letter_still_reaches_kitty_encoding() {
        let physical = make_event(KeyCode::Physical(PhysKeyCode::V), Modifiers::ALT);
        let encoded = physical.encode_kitty(KittyKeyboardFlags::NONE);
        assert!(
            !encoded.is_empty(),
            "an Alt+<letter> chord with no matching keybinding must still be sent to the \
             pane as a physical key event, not silently dropped"
        );

        let char_equivalent = make_event(KeyCode::Char('v'), Modifiers::ALT);
        assert_eq!(
            encoded,
            char_equivalent.encode_kitty(KittyKeyboardFlags::NONE),
            "Physical(V)+ALT must encode identically to Char('v')+ALT"
        );
    }

    #[test]
    fn unbound_ctrl_physical_letter_still_reaches_kitty_encoding() {
        let physical = make_event(KeyCode::Physical(PhysKeyCode::C), Modifiers::CTRL);
        let encoded = physical.encode_kitty(KittyKeyboardFlags::NONE);
        assert!(
            !encoded.is_empty(),
            "an unbound Ctrl+<letter> chord must still reach the pane as a physical key event"
        );

        let char_equivalent = make_event(KeyCode::Char('c'), Modifiers::CTRL);
        assert_eq!(
            encoded,
            char_equivalent.encode_kitty(KittyKeyboardFlags::NONE),
        );
    }

    #[test]
    fn unbound_ctrl_alt_physical_letter_still_reaches_kitty_encoding() {
        // Plain (non-AltGr) Ctrl+Alt held together: the fix must not be
        // specific to any one modifier combination.
        let physical = make_event(
            KeyCode::Physical(PhysKeyCode::D),
            Modifiers::CTRL | Modifiers::ALT,
        );
        let encoded = physical.encode_kitty(KittyKeyboardFlags::REPORT_ALTERNATE_KEYS);
        assert!(!encoded.is_empty());

        let char_equivalent = make_event(KeyCode::Char('d'), Modifiers::CTRL | Modifiers::ALT);
        assert_eq!(
            encoded,
            char_equivalent.encode_kitty(KittyKeyboardFlags::REPORT_ALTERNATE_KEYS),
        );
    }

    #[test]
    fn unbound_alt_physical_non_letter_still_reaches_kitty_encoding() {
        // The fix must be generic across every PhysKeyCode, not just letters.
        let physical = make_event(KeyCode::Physical(PhysKeyCode::LeftBracket), Modifiers::ALT);
        let encoded = physical.encode_kitty(KittyKeyboardFlags::NONE);
        assert!(!encoded.is_empty());

        let char_equivalent = make_event(KeyCode::Char('['), Modifiers::ALT);
        assert_eq!(
            encoded,
            char_equivalent.encode_kitty(KittyKeyboardFlags::NONE),
        );
    }

    #[test]
    fn physical_letter_encodes_via_csi_u_when_alternate_keys_requested() {
        let physical = make_event(KeyCode::Physical(PhysKeyCode::V), Modifiers::ALT);
        let encoded = physical.encode_kitty(KittyKeyboardFlags::REPORT_ALTERNATE_KEYS);
        assert!(!encoded.is_empty());

        let char_equivalent = make_event(KeyCode::Char('v'), Modifiers::ALT);
        assert_eq!(
            encoded,
            char_equivalent.encode_kitty(KittyKeyboardFlags::REPORT_ALTERNATE_KEYS),
        );
    }
}
