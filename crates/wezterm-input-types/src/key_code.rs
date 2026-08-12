#[cfg(feature = "serde")]
use ::serde::*;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::convert::TryFrom;
use wezterm_dynamic::{FromDynamic, ToDynamic};

use crate::{Modifiers, PhysKeyCode};

/// Which key is pressed.  Not all of these are probable to appear
/// on most systems.  A lot of this list is @wez trawling docs and
/// making an entry for things that might be possible in this first pass.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd, FromDynamic, ToDynamic)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum KeyCode {
    /// The decoded unicode character
    Char(char),
    Composed(String),
    RawCode(u32),
    Physical(PhysKeyCode),

    Hyper,
    Super,
    Meta,

    /// Ctrl-break on windows
    Cancel,
    // There is no `Backspace`; use `Char('\u{8}') instead

    // There is no `Tab`; use `Char('\t')` instead
    Clear,
    // There is no `Enter`; use `Char('\r')` instead
    Shift,
    // There is no `Escape`; use `Char('\u{1b}') instead
    LeftShift,
    RightShift,
    Control,
    LeftControl,
    RightControl,
    Alt,
    LeftAlt,
    RightAlt,
    Pause,
    CapsLock,
    VoidSymbol,
    PageUp,
    PageDown,
    End,
    Home,
    LeftArrow,
    RightArrow,
    UpArrow,
    DownArrow,
    Select,
    Print,
    Execute,
    PrintScreen,
    Insert,
    // There is no `Delete`; use `Char('\u{7f}')` instead
    Help,
    LeftWindows,
    RightWindows,
    Applications,
    Sleep,
    /// Numeric keypad digits 0-9
    Numpad(u8),
    Multiply,
    Add,
    Separator,
    Subtract,
    Decimal,
    Divide,
    /// F1-F24 are possible
    Function(u8),
    NumLock,
    ScrollLock,
    Copy,
    Cut,
    Paste,
    BrowserBack,
    BrowserForward,
    BrowserRefresh,
    BrowserStop,
    BrowserSearch,
    BrowserFavorites,
    BrowserHome,
    VolumeMute,
    VolumeDown,
    VolumeUp,
    MediaNextTrack,
    MediaPrevTrack,
    MediaStop,
    MediaPlayPause,
    ApplicationLeftArrow,
    ApplicationRightArrow,
    ApplicationUpArrow,
    ApplicationDownArrow,
    KeyPadHome,
    KeyPadEnd,
    KeyPadPageUp,
    KeyPadPageDown,
    KeyPadBegin,
}

impl KeyCode {
    /// Return true if the key represents a modifier key.
    pub fn is_modifier(&self) -> bool {
        matches!(
            self,
            Self::Hyper
                | Self::CapsLock
                | Self::Super
                | Self::Meta
                | Self::Shift
                | Self::LeftShift
                | Self::RightShift
                | Self::Control
                | Self::LeftControl
                | Self::RightControl
                | Self::Alt
                | Self::LeftAlt
                | Self::RightAlt
                | Self::LeftWindows
                | Self::RightWindows
        )
    }

    pub fn normalize_shift(&self, modifiers: Modifiers) -> (KeyCode, Modifiers) {
        normalize_shift(self.clone(), modifiers)
    }

    pub fn composed(s: &str) -> Self {
        // Prefer to send along a single Char when the string
        // is just a single char, as the keymapping layer cannot
        // bind to composed key sequences
        let mut iter = s.chars();
        let first_char = iter.next();
        let next_char = iter.next();
        match (first_char, next_char) {
            (Some(c), None) => Self::Char(c),
            _ => Self::Composed(s.to_string()),
        }
    }

    /// Convert to a PhysKeyCode.
    /// Note that by the nature of PhysKeyCode being defined in terms
    /// of a US ANSI standard layout, essentially "latinizes" the keycode,
    /// so the results may not make as much sense for non-latin keyboards.
    /// It also loses the shifted state of alphabetical characters.
    pub fn to_phys(&self) -> Option<PhysKeyCode> {
        Some(match self {
            Self::Char('a') | Self::Char('A') => PhysKeyCode::A,
            Self::Char('b') | Self::Char('B') => PhysKeyCode::B,
            Self::Char('c') | Self::Char('C') => PhysKeyCode::C,
            Self::Char('d') | Self::Char('D') => PhysKeyCode::D,
            Self::Char('e') | Self::Char('E') => PhysKeyCode::E,
            Self::Char('f') | Self::Char('F') => PhysKeyCode::F,
            Self::Char('g') | Self::Char('G') => PhysKeyCode::G,
            Self::Char('h') | Self::Char('H') => PhysKeyCode::H,
            Self::Char('i') | Self::Char('I') => PhysKeyCode::I,
            Self::Char('j') | Self::Char('J') => PhysKeyCode::J,
            Self::Char('k') | Self::Char('K') => PhysKeyCode::K,
            Self::Char('l') | Self::Char('L') => PhysKeyCode::L,
            Self::Char('m') | Self::Char('M') => PhysKeyCode::M,
            Self::Char('n') | Self::Char('N') => PhysKeyCode::N,
            Self::Char('o') | Self::Char('O') => PhysKeyCode::O,
            Self::Char('p') | Self::Char('P') => PhysKeyCode::P,
            Self::Char('q') | Self::Char('Q') => PhysKeyCode::Q,
            Self::Char('r') | Self::Char('R') => PhysKeyCode::R,
            Self::Char('s') | Self::Char('S') => PhysKeyCode::S,
            Self::Char('t') | Self::Char('T') => PhysKeyCode::T,
            Self::Char('u') | Self::Char('U') => PhysKeyCode::U,
            Self::Char('v') | Self::Char('V') => PhysKeyCode::V,
            Self::Char('w') | Self::Char('W') => PhysKeyCode::W,
            Self::Char('x') | Self::Char('X') => PhysKeyCode::X,
            Self::Char('y') | Self::Char('Y') => PhysKeyCode::Y,
            Self::Char('z') | Self::Char('Z') => PhysKeyCode::Z,
            Self::Char('0') => PhysKeyCode::K0,
            Self::Char('1') => PhysKeyCode::K1,
            Self::Char('2') => PhysKeyCode::K2,
            Self::Char('3') => PhysKeyCode::K3,
            Self::Char('4') => PhysKeyCode::K4,
            Self::Char('5') => PhysKeyCode::K5,
            Self::Char('6') => PhysKeyCode::K6,
            Self::Char('7') => PhysKeyCode::K7,
            Self::Char('8') => PhysKeyCode::K8,
            Self::Char('9') => PhysKeyCode::K9,
            Self::Char('\\') => PhysKeyCode::Backslash,
            Self::Char(',') => PhysKeyCode::Comma,
            Self::Char('\u{8}') => PhysKeyCode::Backspace,
            Self::Char('\u{7f}') => PhysKeyCode::Delete,
            Self::Char('=') => PhysKeyCode::Equal,
            Self::Char('\u{1b}') => PhysKeyCode::Escape,
            Self::Char('`') => PhysKeyCode::Grave,
            Self::Char('\r') => PhysKeyCode::Return,
            Self::Char('[') => PhysKeyCode::LeftBracket,
            Self::Char(']') => PhysKeyCode::RightBracket,
            Self::Char('-') => PhysKeyCode::Minus,
            Self::Char('.') => PhysKeyCode::Period,
            Self::Char('\'') => PhysKeyCode::Quote,
            Self::Char(';') => PhysKeyCode::Semicolon,
            Self::Char('/') => PhysKeyCode::Slash,
            Self::Char(' ') => PhysKeyCode::Space,
            Self::Char('\t') => PhysKeyCode::Tab,
            Self::Numpad(0) => PhysKeyCode::Keypad0,
            Self::Numpad(1) => PhysKeyCode::Keypad1,
            Self::Numpad(2) => PhysKeyCode::Keypad2,
            Self::Numpad(3) => PhysKeyCode::Keypad3,
            Self::Numpad(4) => PhysKeyCode::Keypad4,
            Self::Numpad(5) => PhysKeyCode::Keypad5,
            Self::Numpad(6) => PhysKeyCode::Keypad6,
            Self::Numpad(7) => PhysKeyCode::Keypad7,
            Self::Numpad(8) => PhysKeyCode::Keypad8,
            Self::Numpad(9) => PhysKeyCode::Keypad9,
            Self::Function(1) => PhysKeyCode::F1,
            Self::Function(2) => PhysKeyCode::F2,
            Self::Function(3) => PhysKeyCode::F3,
            Self::Function(4) => PhysKeyCode::F4,
            Self::Function(5) => PhysKeyCode::F5,
            Self::Function(6) => PhysKeyCode::F6,
            Self::Function(7) => PhysKeyCode::F7,
            Self::Function(8) => PhysKeyCode::F8,
            Self::Function(9) => PhysKeyCode::F9,
            Self::Function(10) => PhysKeyCode::F10,
            Self::Function(11) => PhysKeyCode::F11,
            Self::Function(12) => PhysKeyCode::F12,
            Self::Function(13) => PhysKeyCode::F13,
            Self::Function(14) => PhysKeyCode::F14,
            Self::Function(15) => PhysKeyCode::F15,
            Self::Function(16) => PhysKeyCode::F16,
            Self::Function(17) => PhysKeyCode::F17,
            Self::Function(18) => PhysKeyCode::F18,
            Self::Function(19) => PhysKeyCode::F19,
            Self::Function(20) => PhysKeyCode::F20,
            Self::Physical(p) => *p,
            Self::Shift | Self::LeftShift => PhysKeyCode::LeftShift,
            Self::RightShift => PhysKeyCode::RightShift,
            Self::Alt | Self::LeftAlt => PhysKeyCode::LeftAlt,
            Self::RightAlt => PhysKeyCode::RightAlt,
            Self::LeftWindows => PhysKeyCode::LeftWindows,
            Self::RightWindows => PhysKeyCode::RightWindows,
            Self::Control | Self::LeftControl => PhysKeyCode::LeftControl,
            Self::RightControl => PhysKeyCode::RightControl,
            Self::CapsLock => PhysKeyCode::CapsLock,
            Self::PageUp => PhysKeyCode::PageUp,
            Self::PageDown => PhysKeyCode::PageDown,
            Self::Home => PhysKeyCode::Home,
            Self::End => PhysKeyCode::End,
            Self::LeftArrow => PhysKeyCode::LeftArrow,
            Self::RightArrow => PhysKeyCode::RightArrow,
            Self::UpArrow => PhysKeyCode::UpArrow,
            Self::DownArrow => PhysKeyCode::DownArrow,
            Self::Insert => PhysKeyCode::Insert,
            Self::Help => PhysKeyCode::Help,
            Self::Multiply => PhysKeyCode::KeypadMultiply,
            Self::Clear => PhysKeyCode::KeypadClear,
            Self::Decimal => PhysKeyCode::KeypadDecimal,
            Self::Divide => PhysKeyCode::KeypadDivide,
            Self::Add => PhysKeyCode::KeypadAdd,
            Self::Subtract => PhysKeyCode::KeypadSubtract,
            Self::NumLock => PhysKeyCode::NumLock,
            Self::VolumeUp => PhysKeyCode::VolumeUp,
            Self::VolumeDown => PhysKeyCode::VolumeDown,
            Self::VolumeMute => PhysKeyCode::VolumeMute,
            Self::ApplicationLeftArrow
            | Self::ApplicationRightArrow
            | Self::ApplicationUpArrow
            | Self::ApplicationDownArrow
            | Self::KeyPadHome
            | Self::KeyPadEnd
            | Self::KeyPadPageUp
            | Self::KeyPadPageDown
            | Self::KeyPadBegin
            | Self::MediaNextTrack
            | Self::MediaPrevTrack
            | Self::MediaStop
            | Self::MediaPlayPause
            | Self::Copy
            | Self::Cut
            | Self::Paste
            | Self::BrowserBack
            | Self::BrowserForward
            | Self::BrowserRefresh
            | Self::BrowserStop
            | Self::BrowserSearch
            | Self::BrowserFavorites
            | Self::BrowserHome
            | Self::ScrollLock
            | Self::Separator
            | Self::Sleep
            | Self::Applications
            | Self::Execute
            | Self::PrintScreen
            | Self::Print
            | Self::Select
            | Self::VoidSymbol
            | Self::Pause
            | Self::Cancel
            | Self::Hyper
            | Self::Super
            | Self::Meta
            | Self::Composed(_)
            | Self::RawCode(_)
            | Self::Char(_)
            | Self::Numpad(_)
            | Self::Function(_) => return None,
        })
    }
}

impl TryFrom<&str> for KeyCode {
    type Error = String;
    fn try_from(s: &str) -> core::result::Result<Self, String> {
        macro_rules! m {
            ($($val:ident),* $(,)?) => {
                match s {
                $(
                    core::stringify!($val) => return Ok(Self::$val),
                )*
                    _ => {}
                }
            }
        }

        m!(
            Hyper,
            Super,
            Meta,
            Cancel,
            Clear,
            Shift,
            LeftShift,
            RightShift,
            Control,
            LeftControl,
            RightControl,
            Alt,
            LeftAlt,
            RightAlt,
            Pause,
            CapsLock,
            VoidSymbol,
            PageUp,
            PageDown,
            End,
            Home,
            LeftArrow,
            RightArrow,
            UpArrow,
            DownArrow,
            Select,
            Print,
            Execute,
            PrintScreen,
            Insert,
            Help,
            LeftWindows,
            RightWindows,
            Applications,
            Sleep,
            Multiply,
            Add,
            Separator,
            Subtract,
            Decimal,
            Divide,
            NumLock,
            ScrollLock,
            Copy,
            Cut,
            Paste,
            BrowserBack,
            BrowserForward,
            BrowserRefresh,
            BrowserStop,
            BrowserSearch,
            BrowserFavorites,
            BrowserHome,
            VolumeMute,
            VolumeDown,
            VolumeUp,
            MediaNextTrack,
            MediaPrevTrack,
            MediaStop,
            MediaPlayPause,
            ApplicationLeftArrow,
            ApplicationRightArrow,
            ApplicationUpArrow,
            ApplicationDownArrow,
        );

        match s {
            "Backspace" => return Ok(KeyCode::Char('\u{8}')),
            "Tab" => return Ok(KeyCode::Char('\t')),
            "Return" | "Enter" => return Ok(KeyCode::Char('\r')),
            "Escape" => return Ok(KeyCode::Char('\u{1b}')),
            "Delete" => return Ok(KeyCode::Char('\u{7f}')),
            _ => {}
        };

        if let Some(n) = s.strip_prefix("Numpad") {
            let n: u8 = n
                .parse()
                .map_err(|err| format!("parsing Numpad<NUMBER>: {:#}", err))?;
            if n > 9 {
                return Err("Numpad numbers must be in range 0-9".to_string());
            }
            return Ok(KeyCode::Numpad(n));
        }

        // Don't consider "F" to be an invalid F key!
        if s.len() > 1 {
            if let Some(n) = s.strip_prefix("F") {
                let n: u8 = n
                    .parse()
                    .map_err(|err| format!("parsing F<NUMBER>: {:#}", err))?;
                if n == 0 || n > 24 {
                    return Err("Function key numbers must be in range 1-24".to_string());
                }
                return Ok(KeyCode::Function(n));
            }
        }

        let chars: Vec<char> = s.chars().collect();
        if chars.len() == 1 {
            let k = KeyCode::Char(chars[0]);
            Ok(k)
        } else {
            Err(format!("invalid KeyCode string {}", s))
        }
    }
}

impl core::fmt::Display for KeyCode {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        match self {
            Self::RawCode(n) => write!(f, "raw:{}", n),
            Self::Char(c) => write!(f, "mapped:{}", c),
            Self::Physical(phys) => write!(f, "{}", phys),
            Self::Composed(s) => write!(f, "{}", s),
            Self::Numpad(n) => write!(f, "Numpad{}", n),
            Self::Function(n) => write!(f, "F{}", n),
            other => write!(f, "{:?}", other),
        }
    }
}

pub(crate) fn normalize_shift(key: KeyCode, modifiers: Modifiers) -> (KeyCode, Modifiers) {
    if modifiers.contains(Modifiers::SHIFT) {
        match key {
            KeyCode::Char(c) if c.is_ascii_uppercase() => (key, modifiers - Modifiers::SHIFT),
            KeyCode::Char(c) if c.is_ascii_lowercase() => (
                KeyCode::Char(c.to_ascii_uppercase()),
                modifiers - Modifiers::SHIFT,
            ),
            _ => (key, modifiers),
        }
    } else {
        (key, modifiers)
    }
}
