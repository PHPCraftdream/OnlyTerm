#[cfg(feature = "serde")]
use ::serde::*;
use alloc::format;
use alloc::string::{String, ToString};
use core::convert::TryFrom;
use onlyterm_dynamic::{FromDynamic, ToDynamic};

#[cfg(feature = "std")]
use std::collections::HashMap;
#[cfg(feature = "std")]
use std::sync::LazyLock;

use crate::KeyCode;

/// These keycodes identify keys based on their physical
/// position on an ANSI-standard US keyboard.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Copy, Ord, PartialOrd, FromDynamic, ToDynamic)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum PhysKeyCode {
    A,
    B,
    Backslash,
    C,
    CapsLock,
    Comma,
    D,
    Backspace,
    DownArrow,
    E,
    End,
    Equal,
    Escape,
    F,
    F1,
    F10,
    F11,
    F12,
    F13,
    F14,
    F15,
    F16,
    F17,
    F18,
    F19,
    F2,
    F20,
    F21,
    F22,
    F23,
    F24,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    Delete,
    Function,
    G,
    Grave,
    H,
    Help,
    Home,
    I,
    Insert,
    J,
    K,
    K0,
    K1,
    K2,
    K3,
    K4,
    K5,
    K6,
    K7,
    K8,
    K9,
    Keypad0,
    Keypad1,
    Keypad2,
    Keypad3,
    Keypad4,
    Keypad5,
    Keypad6,
    Keypad7,
    Keypad8,
    Keypad9,
    KeypadClear,
    KeypadDecimal,
    KeypadDelete,
    KeypadDivide,
    KeypadEnter,
    KeypadEquals,
    KeypadSubtract,
    KeypadMultiply,
    KeypadAdd,
    L,
    LeftAlt,
    LeftArrow,
    LeftBracket,
    LeftControl,
    LeftShift,
    LeftWindows,
    M,
    Minus,
    VolumeMute,
    N,
    NumLock,
    O,
    P,
    PageDown,
    PageUp,
    Period,
    Q,
    Quote,
    R,
    Return,
    RightAlt,
    RightArrow,
    RightBracket,
    RightControl,
    RightShift,
    RightWindows,
    S,
    Semicolon,
    Slash,
    Space,
    T,
    Tab,
    U,
    UpArrow,
    V,
    VolumeDown,
    VolumeUp,
    W,
    X,
    Y,
    Z,
}

impl PhysKeyCode {
    pub fn is_modifier(&self) -> bool {
        matches!(
            self,
            Self::LeftShift
                | Self::LeftControl
                | Self::LeftWindows
                | Self::LeftAlt
                | Self::RightShift
                | Self::RightControl
                | Self::RightWindows
                | Self::RightAlt
        )
    }

    /// Returns the Windows virtual-key code and the IBM PC "set 1" scan code
    /// that a real hardware press of this key reports in a
    /// `KEY_EVENT_RECORD`, or `None` for keys this table doesn't cover.
    ///
    /// Both values are properties of the *physical* keyboard rather than of
    /// the active layout, which is exactly what `PhysKeyCode` denotes, so
    /// this is a fixed table rather than a `VkKeyScanW`/`MapVirtualKeyW`
    /// round-trip: under eg. a Russian layout `VkKeyScanW('j')` fails
    /// outright, yet the J *key* is still physically there and still what a
    /// Ctrl+J chord means.
    ///
    /// This exists because synthetic key events (the ones raised by key
    /// *assignments* such as `SendChar`, rather than by a real keypress)
    /// have no hardware event to copy these from, and
    /// `KeyEvent::encode_win32_input_mode` puts them straight into the
    /// `Vk`/`Sc` fields of the sequence it emits. Sending zeros there
    /// produces a record that consumers cannot resolve back to a key --
    /// ConPTY/crossterm run `ToUnicode(vk, sc, ..)` to recover the
    /// character, which fails for `vk == 0` and drops the keypress entirely.
    ///
    /// Coverage is deliberately limited to the keys `KeyCode::to_phys` can
    /// produce from a `KeyCode::Char`, since those are the ones synthetic
    /// events are built from; function keys, arrows, the keypad and the
    /// modifiers themselves return `None`.
    pub fn to_win32_key_codes(self) -> Option<(u32, u32)> {
        // Virtual-key codes: <https://learn.microsoft.com/en-us/windows/win32/inputdev/virtual-key-codes>
        let (vkey, scan_code): (u32, u32) = match self {
            Self::A => (0x41, 0x1e),
            Self::B => (0x42, 0x30),
            Self::C => (0x43, 0x2e),
            Self::D => (0x44, 0x20),
            Self::E => (0x45, 0x12),
            Self::F => (0x46, 0x21),
            Self::G => (0x47, 0x22),
            Self::H => (0x48, 0x23),
            Self::I => (0x49, 0x17),
            Self::J => (0x4a, 0x24),
            Self::K => (0x4b, 0x25),
            Self::L => (0x4c, 0x26),
            Self::M => (0x4d, 0x32),
            Self::N => (0x4e, 0x31),
            Self::O => (0x4f, 0x18),
            Self::P => (0x50, 0x19),
            Self::Q => (0x51, 0x10),
            Self::R => (0x52, 0x13),
            Self::S => (0x53, 0x1f),
            Self::T => (0x54, 0x14),
            Self::U => (0x55, 0x16),
            Self::V => (0x56, 0x2f),
            Self::W => (0x57, 0x11),
            Self::X => (0x58, 0x2d),
            Self::Y => (0x59, 0x15),
            Self::Z => (0x5a, 0x2c),
            Self::K0 => (0x30, 0x0b),
            Self::K1 => (0x31, 0x02),
            Self::K2 => (0x32, 0x03),
            Self::K3 => (0x33, 0x04),
            Self::K4 => (0x34, 0x05),
            Self::K5 => (0x35, 0x06),
            Self::K6 => (0x36, 0x07),
            Self::K7 => (0x37, 0x08),
            Self::K8 => (0x38, 0x09),
            Self::K9 => (0x39, 0x0a),
            Self::Return => (0x0d, 0x1c),       // VK_RETURN
            Self::Space => (0x20, 0x39),        // VK_SPACE
            Self::Tab => (0x09, 0x0f),          // VK_TAB
            Self::Escape => (0x1b, 0x01),       // VK_ESCAPE
            Self::Backspace => (0x08, 0x0e),    // VK_BACK
            Self::Delete => (0x2e, 0x53),       // VK_DELETE
            Self::Minus => (0xbd, 0x0c),        // VK_OEM_MINUS
            Self::Equal => (0xbb, 0x0d),        // VK_OEM_PLUS
            Self::LeftBracket => (0xdb, 0x1a),  // VK_OEM_4
            Self::RightBracket => (0xdd, 0x1b), // VK_OEM_6
            Self::Backslash => (0xdc, 0x2b),    // VK_OEM_5
            Self::Semicolon => (0xba, 0x27),    // VK_OEM_1
            Self::Quote => (0xde, 0x28),        // VK_OEM_7
            Self::Grave => (0xc0, 0x29),        // VK_OEM_3
            Self::Comma => (0xbc, 0x33),        // VK_OEM_COMMA
            Self::Period => (0xbe, 0x34),       // VK_OEM_PERIOD
            Self::Slash => (0xbf, 0x35),        // VK_OEM_2
            _ => return None,
        };
        Some((vkey, scan_code))
    }

    pub fn to_key_code(self) -> KeyCode {
        match self {
            Self::LeftShift => KeyCode::LeftShift,
            Self::LeftControl => KeyCode::LeftControl,
            Self::LeftWindows => KeyCode::LeftWindows,
            Self::LeftAlt => KeyCode::LeftAlt,
            Self::RightShift => KeyCode::RightShift,
            Self::RightControl => KeyCode::RightControl,
            Self::RightWindows => KeyCode::RightWindows,
            Self::RightAlt => KeyCode::RightAlt,
            Self::LeftArrow => KeyCode::LeftArrow,
            Self::RightArrow => KeyCode::RightArrow,
            Self::UpArrow => KeyCode::UpArrow,
            Self::DownArrow => KeyCode::DownArrow,
            Self::CapsLock => KeyCode::CapsLock,
            Self::F1 => KeyCode::Function(1),
            Self::F2 => KeyCode::Function(2),
            Self::F3 => KeyCode::Function(3),
            Self::F4 => KeyCode::Function(4),
            Self::F5 => KeyCode::Function(5),
            Self::F6 => KeyCode::Function(6),
            Self::F7 => KeyCode::Function(7),
            Self::F8 => KeyCode::Function(8),
            Self::F9 => KeyCode::Function(9),
            Self::F10 => KeyCode::Function(10),
            Self::F11 => KeyCode::Function(11),
            Self::F12 => KeyCode::Function(12),
            Self::F13 => KeyCode::Function(13),
            Self::F14 => KeyCode::Function(14),
            Self::F15 => KeyCode::Function(15),
            Self::F16 => KeyCode::Function(16),
            Self::F17 => KeyCode::Function(17),
            Self::F18 => KeyCode::Function(18),
            Self::F19 => KeyCode::Function(19),
            Self::F20 => KeyCode::Function(20),
            Self::F21 => KeyCode::Function(21),
            Self::F22 => KeyCode::Function(22),
            Self::F23 => KeyCode::Function(23),
            Self::F24 => KeyCode::Function(24),
            Self::Keypad0 => KeyCode::Numpad(0),
            Self::Keypad1 => KeyCode::Numpad(1),
            Self::Keypad2 => KeyCode::Numpad(2),
            Self::Keypad3 => KeyCode::Numpad(3),
            Self::Keypad4 => KeyCode::Numpad(4),
            Self::Keypad5 => KeyCode::Numpad(5),
            Self::Keypad6 => KeyCode::Numpad(6),
            Self::Keypad7 => KeyCode::Numpad(7),
            Self::Keypad8 => KeyCode::Numpad(8),
            Self::Keypad9 => KeyCode::Numpad(9),
            Self::KeypadClear => KeyCode::Clear,
            Self::KeypadMultiply => KeyCode::Multiply,
            Self::KeypadDecimal => KeyCode::Decimal,
            Self::KeypadDivide => KeyCode::Divide,
            Self::KeypadAdd => KeyCode::Add,
            Self::KeypadSubtract => KeyCode::Subtract,
            Self::A => KeyCode::Char('a'),
            Self::B => KeyCode::Char('b'),
            Self::C => KeyCode::Char('c'),
            Self::D => KeyCode::Char('d'),
            Self::E => KeyCode::Char('e'),
            Self::F => KeyCode::Char('f'),
            Self::G => KeyCode::Char('g'),
            Self::H => KeyCode::Char('h'),
            Self::I => KeyCode::Char('i'),
            Self::J => KeyCode::Char('j'),
            Self::K => KeyCode::Char('k'),
            Self::L => KeyCode::Char('l'),
            Self::M => KeyCode::Char('m'),
            Self::N => KeyCode::Char('n'),
            Self::O => KeyCode::Char('o'),
            Self::P => KeyCode::Char('p'),
            Self::Q => KeyCode::Char('q'),
            Self::R => KeyCode::Char('r'),
            Self::S => KeyCode::Char('s'),
            Self::T => KeyCode::Char('t'),
            Self::U => KeyCode::Char('u'),
            Self::V => KeyCode::Char('v'),
            Self::W => KeyCode::Char('w'),
            Self::X => KeyCode::Char('x'),
            Self::Y => KeyCode::Char('y'),
            Self::Z => KeyCode::Char('z'),
            Self::Backslash => KeyCode::Char('\\'),
            Self::Comma => KeyCode::Char(','),
            Self::Backspace => KeyCode::Char('\u{8}'),
            Self::KeypadDelete | Self::Delete => KeyCode::Char('\u{7f}'),
            Self::End => KeyCode::End,
            Self::Home => KeyCode::Home,
            Self::KeypadEquals | Self::Equal => KeyCode::Char('='),
            Self::Escape => KeyCode::Char('\u{1b}'),
            Self::Function => KeyCode::Physical(self),
            Self::Grave => KeyCode::Char('`'),
            Self::Help => KeyCode::Help,
            Self::Insert => KeyCode::Insert,
            Self::K0 => KeyCode::Char('0'),
            Self::K1 => KeyCode::Char('1'),
            Self::K2 => KeyCode::Char('2'),
            Self::K3 => KeyCode::Char('3'),
            Self::K4 => KeyCode::Char('4'),
            Self::K5 => KeyCode::Char('5'),
            Self::K6 => KeyCode::Char('6'),
            Self::K7 => KeyCode::Char('7'),
            Self::K8 => KeyCode::Char('8'),
            Self::K9 => KeyCode::Char('9'),
            Self::Return | Self::KeypadEnter => KeyCode::Char('\r'),
            Self::LeftBracket => KeyCode::Char('['),
            Self::RightBracket => KeyCode::Char(']'),
            Self::Minus => KeyCode::Char('-'),
            Self::VolumeMute => KeyCode::VolumeMute,
            Self::VolumeUp => KeyCode::VolumeUp,
            Self::VolumeDown => KeyCode::VolumeDown,
            Self::NumLock => KeyCode::NumLock,
            Self::PageUp => KeyCode::PageUp,
            Self::PageDown => KeyCode::PageDown,
            Self::Period => KeyCode::Char('.'),
            Self::Quote => KeyCode::Char('\''),
            Self::Semicolon => KeyCode::Char(';'),
            Self::Slash => KeyCode::Char('/'),
            Self::Space => KeyCode::Char(' '),
            Self::Tab => KeyCode::Char('\t'),
        }
    }

    fn for_each_code(mut func: impl FnMut(&str, Self) -> bool) {
        macro_rules! m {
            ($($val:ident),* $(,)?) => {
                $(
                    let key = stringify!($val);
                    if key.len() == 1 {
                        if (func)(&key.to_ascii_lowercase(), PhysKeyCode::$val) {
                            return;
                        }
                    }
                    if (func)(key, PhysKeyCode::$val) {
                        return;
                    }
                )*
            }
        }

        m!(
            A,
            B,
            Backslash,
            C,
            CapsLock,
            Comma,
            D,
            Backspace,
            DownArrow,
            E,
            End,
            Equal,
            Escape,
            F,
            F1,
            F10,
            F11,
            F12,
            F13,
            F14,
            F15,
            F16,
            F17,
            F18,
            F19,
            F2,
            F20,
            F3,
            F4,
            F5,
            F6,
            F7,
            F8,
            F9,
            Delete,
            Function,
            G,
            Grave,
            H,
            Help,
            Home,
            I,
            Insert,
            J,
            K,
            Keypad0,
            Keypad1,
            Keypad2,
            Keypad3,
            Keypad4,
            Keypad5,
            Keypad6,
            Keypad7,
            Keypad8,
            Keypad9,
            KeypadClear,
            KeypadDecimal,
            KeypadDelete,
            KeypadDivide,
            KeypadEnter,
            KeypadEquals,
            KeypadSubtract,
            KeypadMultiply,
            KeypadAdd,
            L,
            LeftAlt,
            LeftArrow,
            LeftBracket,
            LeftControl,
            LeftShift,
            LeftWindows,
            M,
            Minus,
            VolumeMute,
            N,
            NumLock,
            O,
            P,
            PageDown,
            PageUp,
            Period,
            Q,
            Quote,
            R,
            Return,
            RightAlt,
            RightArrow,
            RightBracket,
            RightControl,
            RightShift,
            RightWindows,
            S,
            Semicolon,
            Slash,
            Space,
            T,
            Tab,
            U,
            UpArrow,
            V,
            VolumeDown,
            VolumeUp,
            W,
            X,
            Y,
            Z,
        );

        for (label, value) in [
            ("0", PhysKeyCode::K0),
            ("1", PhysKeyCode::K1),
            ("2", PhysKeyCode::K2),
            ("3", PhysKeyCode::K3),
            ("4", PhysKeyCode::K4),
            ("5", PhysKeyCode::K5),
            ("6", PhysKeyCode::K6),
            ("7", PhysKeyCode::K7),
            ("8", PhysKeyCode::K8),
            ("9", PhysKeyCode::K9),
        ] {
            if (func)(label, value) {
                return;
            }
        }
    }

    #[cfg(feature = "std")]
    fn make_map() -> HashMap<String, Self> {
        let mut map = HashMap::new();

        Self::for_each_code(|label, code| {
            map.insert(label.to_string(), code);
            false
        });

        map
    }

    #[cfg(feature = "std")]
    fn make_inv_map() -> HashMap<Self, String> {
        let mut map = HashMap::new();
        for (k, v) in PHYSKEYCODE_MAP.iter() {
            map.insert(*v, k.clone());
        }
        map
    }

    fn name_to_code(name: &str) -> Option<Self> {
        #[cfg(feature = "std")]
        {
            PHYSKEYCODE_MAP.get(name).copied()
        }
        #[cfg(not(feature = "std"))]
        {
            let mut result = None;
            Self::for_each_code(|label, code| {
                if label == name {
                    result.replace(code);
                    true
                } else {
                    false
                }
            });
            result
        }
    }

    fn name(&self) -> Option<String> {
        #[cfg(feature = "std")]
        {
            INV_PHYSKEYCODE_MAP.get(self).cloned()
        }
        #[cfg(not(feature = "std"))]
        {
            let mut result = None;
            Self::for_each_code(|label, code| {
                if code == *self {
                    result.replace(label.to_string());
                    true
                } else {
                    false
                }
            });
            result
        }
    }
}

#[cfg(feature = "std")]
static PHYSKEYCODE_MAP: LazyLock<HashMap<String, PhysKeyCode>> =
    LazyLock::new(PhysKeyCode::make_map);
#[cfg(feature = "std")]
static INV_PHYSKEYCODE_MAP: LazyLock<HashMap<PhysKeyCode, String>> =
    LazyLock::new(PhysKeyCode::make_inv_map);

impl TryFrom<&str> for PhysKeyCode {
    type Error = String;
    fn try_from(s: &str) -> core::result::Result<PhysKeyCode, String> {
        if let Some(code) = Self::name_to_code(s) {
            Ok(code)
        } else {
            Err(format!("invalid PhysKeyCode '{}'", s))
        }
    }
}

impl core::fmt::Display for PhysKeyCode {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        if let Some(s) = self.name() {
            write!(f, "{}", s)
        } else {
            write!(f, "{:?}", self)
        }
    }
}
