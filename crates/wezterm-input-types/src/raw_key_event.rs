use crate::Handled;
use crate::KeyCode;
use crate::KeyboardLedStatus;
use crate::Modifiers;
use crate::PhysKeyCode;

/// A key event prior to any dead key or IME composition
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct RawKeyEvent {
    pub key: KeyCode,
    pub modifiers: Modifiers,
    pub leds: KeyboardLedStatus,

    /// The physical location of the key on an ANSI-Standard US layout
    pub phys_code: Option<PhysKeyCode>,
    /// The OS and hardware dependent key code for the key
    pub raw_code: u32,

    /// The *other* OS and hardware dependent key code for the key
    #[cfg(windows)]
    pub scan_code: u32,

    /// How many times this key repeats
    pub repeat_count: u16,

    /// If true, this is a key down rather than a key up event
    pub key_is_down: bool,
    pub handled: Handled,
}

impl RawKeyEvent {
    /// Mark the event as handled, in order to prevent additional
    /// processing.
    pub fn set_handled(&self) {
        self.handled.set_handled();
    }

    /// <https://sw.kovidgoyal.net/kitty/keyboard-protocol/#functional-key-definitions>
    #[deny(warnings)]
    pub(crate) fn kitty_function_code(&self) -> Option<u32> {
        use KeyCode::*;
        Some(match self.key {
            // Tab => 9,
            // Backspace => 127,
            // CapsLock => 57358,
            // ScrollLock => 57359,
            // NumLock => 57360,
            // PrintScreen => 57361,
            // Pause => 57362,
            // Menu => 57363,
            Function(n) if (13..=35).contains(&n) => 57376 + n as u32 - 13,
            Numpad(n) => n as u32 + 57399,
            Decimal => 57409,
            Divide => 57410,
            Multiply => 57411,
            Subtract => 57412,
            Add => 57413,
            // KeypadEnter => 57414,
            // KeypadEquals => 57415,
            Separator => 57416,
            ApplicationLeftArrow => 57417,
            ApplicationRightArrow => 57418,
            ApplicationUpArrow => 57419,
            ApplicationDownArrow => 57420,
            KeyPadHome => 57423,
            KeyPadEnd => 57424,
            KeyPadBegin => 57427,
            KeyPadPageUp => 57421,
            KeyPadPageDown => 57422,
            Insert => 57425,
            // KeypadDelete => 57426,
            MediaPlayPause => 57430,
            MediaStop => 57432,
            MediaNextTrack => 57435,
            MediaPrevTrack => 57436,
            VolumeDown => 57438,
            VolumeUp => 57439,
            VolumeMute => 57440,
            LeftShift => 57441,
            LeftControl => 57442,
            LeftAlt => 57443,
            LeftWindows => 57444,
            RightShift => 57447,
            RightControl => 57448,
            RightAlt => 57449,
            RightWindows => 57450,
            _ => match &self.phys_code {
                Some(phys) => {
                    use PhysKeyCode::*;

                    match *phys {
                        Escape => 27,
                        Return => 13,
                        Tab => 9,
                        Backspace => 127,
                        CapsLock => 57358,
                        // ScrollLock => 57359,
                        NumLock => 57360,
                        // PrintScreen => 57361,
                        // Pause => 57362,
                        // Menu => 57363,
                        F13 => 57376,
                        F14 => 57377,
                        F15 => 57378,
                        F16 => 57379,
                        F17 => 57380,
                        F18 => 57381,
                        F19 => 57382,
                        F20 => 57383,
                        F21 => 57384,
                        F22 => 57385,
                        F23 => 57386,
                        F24 => 57387,
                        /*
                        F25 => 57388,
                        F26 => 57389,
                        F27 => 57390,
                        F28 => 57391,
                        F29 => 57392,
                        F30 => 57393,
                        F31 => 57394,
                        F32 => 57395,
                        F33 => 57396,
                        F34 => 57397,
                        */
                        Keypad0 => 57399,
                        Keypad1 => 57400,
                        Keypad2 => 57401,
                        Keypad3 => 57402,
                        Keypad4 => 57403,
                        Keypad5 => 57404,
                        Keypad6 => 57405,
                        Keypad7 => 57406,
                        Keypad8 => 57407,
                        Keypad9 => 57408,
                        KeypadDecimal => 57409,
                        KeypadDivide => 57410,
                        KeypadMultiply => 57411,
                        KeypadSubtract => 57412,
                        KeypadAdd => 57413,
                        KeypadEnter => 57414,
                        KeypadEquals => 57415,
                        // KeypadSeparator => 57416,
                        // ApplicationLeftArrow => 57417,
                        // ApplicationRightArrow => 57418,
                        // ApplicationUpArrow => 57419,
                        // ApplicationDownArrow => 57420,
                        // KeyPadHome => 57423,
                        // KeyPadEnd => 57424,
                        // KeyPadBegin => 57427,
                        // KeyPadPageUp => 57421,
                        // KeyPadPageDown => 57422,
                        Insert => 57425,
                        // KeypadDelete => 57426,
                        // MediaPlayPause => 57430,
                        // MediaStop => 57432,
                        // MediaNextTrack => 57435,
                        // MediaPrevTrack => 57436,
                        VolumeDown => 57438,
                        VolumeUp => 57439,
                        VolumeMute => 57440,
                        LeftShift => 57441,
                        LeftControl => 57442,
                        LeftAlt => 57443,
                        LeftWindows => 57444,
                        RightShift => 57447,
                        RightControl => 57448,
                        RightAlt => 57449,
                        RightWindows => 57450,
                        _ => return None,
                    }
                }
                _ => return None,
            },
        })
    }
}
