use crate::bail;
use crate::error::Result;
use crate::escape::csi::KittyKeyboardFlags;
#[cfg(feature = "use_serde")]
use serde::{Deserialize, Serialize};
use std::fmt::Write;
use wezterm_input_types::{ctrl_mapping, KeyboardLedStatus as InputKeyboardLedStatus};

use super::Modifiers;

pub const CSI: &str = "\x1b[";
pub const SS3: &str = "\x1bO";

fn to_input_types_key_code(key: KeyCode) -> Option<wezterm_input_types::KeyCode> {
    use wezterm_input_types::KeyCode as IK;
    Some(match key {
        KeyCode::Char(c) => IK::Char(c),

        // termwiz has these as distinct variants; input-types represents them as Char
        KeyCode::Backspace => IK::Char('\u{8}'),
        KeyCode::Tab => IK::Char('\t'),
        KeyCode::Enter => IK::Char('\r'),
        KeyCode::Escape => IK::Char('\u{1b}'),
        KeyCode::Delete => IK::Char('\u{7f}'),

        KeyCode::Hyper => IK::Hyper,
        KeyCode::Super => IK::Super,
        KeyCode::Meta => IK::Meta,
        KeyCode::Cancel => IK::Cancel,
        KeyCode::Clear => IK::Clear,
        KeyCode::Shift => IK::Shift,
        KeyCode::LeftShift => IK::LeftShift,
        KeyCode::RightShift => IK::RightShift,
        KeyCode::Control => IK::Control,
        KeyCode::LeftControl => IK::LeftControl,
        KeyCode::RightControl => IK::RightControl,
        KeyCode::Alt => IK::Alt,
        KeyCode::LeftAlt => IK::LeftAlt,
        KeyCode::RightAlt => IK::RightAlt,

        // termwiz has Menu variants; input-types uses Applications
        KeyCode::Menu | KeyCode::LeftMenu | KeyCode::RightMenu => IK::Applications,

        KeyCode::Pause => IK::Pause,
        KeyCode::CapsLock => IK::CapsLock,
        KeyCode::PageUp => IK::PageUp,
        KeyCode::PageDown => IK::PageDown,
        KeyCode::End => IK::End,
        KeyCode::Home => IK::Home,
        KeyCode::LeftArrow => IK::LeftArrow,
        KeyCode::RightArrow => IK::RightArrow,
        KeyCode::UpArrow => IK::UpArrow,
        KeyCode::DownArrow => IK::DownArrow,
        KeyCode::Select => IK::Select,
        KeyCode::Print => IK::Print,
        KeyCode::Execute => IK::Execute,
        KeyCode::PrintScreen => IK::PrintScreen,
        KeyCode::Insert => IK::Insert,
        KeyCode::Help => IK::Help,
        KeyCode::LeftWindows => IK::LeftWindows,
        KeyCode::RightWindows => IK::RightWindows,
        KeyCode::Applications => IK::Applications,
        KeyCode::Sleep => IK::Sleep,

        KeyCode::Numpad0 => IK::Numpad(0),
        KeyCode::Numpad1 => IK::Numpad(1),
        KeyCode::Numpad2 => IK::Numpad(2),
        KeyCode::Numpad3 => IK::Numpad(3),
        KeyCode::Numpad4 => IK::Numpad(4),
        KeyCode::Numpad5 => IK::Numpad(5),
        KeyCode::Numpad6 => IK::Numpad(6),
        KeyCode::Numpad7 => IK::Numpad(7),
        KeyCode::Numpad8 => IK::Numpad(8),
        KeyCode::Numpad9 => IK::Numpad(9),
        KeyCode::Multiply => IK::Multiply,
        KeyCode::Add => IK::Add,
        KeyCode::Separator => IK::Separator,
        KeyCode::Subtract => IK::Subtract,
        KeyCode::Decimal => IK::Decimal,
        KeyCode::Divide => IK::Divide,
        KeyCode::Function(n) => IK::Function(n),
        KeyCode::NumLock => IK::NumLock,
        KeyCode::ScrollLock => IK::ScrollLock,
        KeyCode::Copy => IK::Copy,
        KeyCode::Cut => IK::Cut,
        KeyCode::Paste => IK::Paste,
        KeyCode::BrowserBack => IK::BrowserBack,
        KeyCode::BrowserForward => IK::BrowserForward,
        KeyCode::BrowserRefresh => IK::BrowserRefresh,
        KeyCode::BrowserStop => IK::BrowserStop,
        KeyCode::BrowserSearch => IK::BrowserSearch,
        KeyCode::BrowserFavorites => IK::BrowserFavorites,
        KeyCode::BrowserHome => IK::BrowserHome,
        KeyCode::VolumeMute => IK::VolumeMute,
        KeyCode::VolumeDown => IK::VolumeDown,
        KeyCode::VolumeUp => IK::VolumeUp,
        KeyCode::MediaNextTrack => IK::MediaNextTrack,
        KeyCode::MediaPrevTrack => IK::MediaPrevTrack,
        KeyCode::MediaStop => IK::MediaStop,
        KeyCode::MediaPlayPause => IK::MediaPlayPause,
        KeyCode::ApplicationLeftArrow => IK::ApplicationLeftArrow,
        KeyCode::ApplicationRightArrow => IK::ApplicationRightArrow,
        KeyCode::ApplicationUpArrow => IK::ApplicationUpArrow,
        KeyCode::ApplicationDownArrow => IK::ApplicationDownArrow,
        KeyCode::KeyPadHome => IK::KeyPadHome,
        KeyCode::KeyPadEnd => IK::KeyPadEnd,
        KeyCode::KeyPadPageUp => IK::KeyPadPageUp,
        KeyCode::KeyPadPageDown => IK::KeyPadPageDown,
        KeyCode::KeyPadBegin => IK::KeyPadBegin,

        // Internal synthetic keys; do not encode to application input
        KeyCode::InternalPasteStart | KeyCode::InternalPasteEnd => return None,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyboardEncoding {
    Xterm,
    /// <http://www.leonerd.org.uk/hacks/fixterms/>
    CsiU,
    /// <https://github.com/microsoft/terminal/blob/main/doc/specs/%234999%20-%20Improved%20keyboard%20handling%20in%20Conpty.md>
    Win32,
    /// <https://sw.kovidgoyal.net/kitty/keyboard-protocol/>
    Kitty(KittyKeyboardFlags),
}

/// Specifies terminal modes/configuration that can influence how a KeyCode
/// is encoded when being sent to and application via the pty.
#[derive(Debug, Clone, Copy)]
pub struct KeyCodeEncodeModes {
    pub encoding: KeyboardEncoding,
    pub application_cursor_keys: bool,
    pub newline_mode: bool,
    pub modify_other_keys: Option<i64>,
}

/// Which key is pressed.  Not all of these are probable to appear
/// on most systems.  A lot of this list is @wez trawling docs and
/// making an entry for things that might be possible in this first pass.
#[cfg_attr(feature = "use_serde", derive(Serialize, Deserialize))]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum KeyCode {
    /// The decoded unicode character
    Char(char),

    Hyper,
    Super,
    Meta,

    /// Ctrl-break on windows
    Cancel,
    Backspace,
    Tab,
    Clear,
    Enter,
    Shift,
    Escape,
    LeftShift,
    RightShift,
    Control,
    LeftControl,
    RightControl,
    Alt,
    LeftAlt,
    RightAlt,
    Menu,
    LeftMenu,
    RightMenu,
    Pause,
    CapsLock,
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
    Delete,
    Help,
    LeftWindows,
    RightWindows,
    Applications,
    Sleep,
    Numpad0,
    Numpad1,
    Numpad2,
    Numpad3,
    Numpad4,
    Numpad5,
    Numpad6,
    Numpad7,
    Numpad8,
    Numpad9,
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

    #[doc(hidden)]
    InternalPasteStart,
    #[doc(hidden)]
    InternalPasteEnd,
}

impl KeyCode {
    /// if SHIFT is held and we have KeyCode::Char('c') we want to normalize
    /// that keycode to KeyCode::Char('C'); that is what this function does.
    /// In theory we should give the same treatment to keys like `[` -> `{`
    /// but that assumes something about the keyboard layout and is probably
    /// better done in the gui frontend rather than this layer.
    /// In fact, this function might be better off if it lived elsewhere.
    pub fn normalize_shift_to_upper_case(self, modifiers: Modifiers) -> KeyCode {
        if modifiers.contains(Modifiers::SHIFT) {
            match self {
                KeyCode::Char(c) if c.is_ascii_lowercase() => KeyCode::Char(c.to_ascii_uppercase()),
                _ => self,
            }
        } else {
            self
        }
    }

    /// Return true if the key represents a modifier key.
    pub fn is_modifier(self) -> bool {
        matches!(
            self,
            Self::Hyper
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

    /// Returns the byte sequence that represents this KeyCode and Modifier combination,
    pub fn encode(
        &self,
        mods: Modifiers,
        modes: KeyCodeEncodeModes,
        is_down: bool,
    ) -> Result<String> {
        // We are encoding the key as an xterm-compatible sequence, which does not support
        // positional modifiers.
        let mods = mods.remove_positional_mods();

        use KeyCode::*;

        let key = self.normalize_shift_to_upper_case(mods);
        // Normalize the modifier state for Char's that are uppercase; remove
        // the SHIFT modifier so that reduce ambiguity below
        let mods = match key {
            Char(c)
                if (c.is_ascii_punctuation() || c.is_ascii_uppercase())
                    && mods.contains(Modifiers::SHIFT) =>
            {
                mods & !Modifiers::SHIFT
            }
            _ => mods,
        };

        // Normalize Backspace and Delete
        let key = match key {
            Char('\x7f') => Delete,
            Char('\x08') => Backspace,
            c => c,
        };

        if let KeyboardEncoding::Kitty(flags) = modes.encoding {
            // When in kitty mode, reuse the same encoder used by the GUI layer:
            // wezterm_input_types::KeyEvent::encode_kitty.
            if let Some(input_key) = to_input_types_key_code(key) {
                let ev = wezterm_input_types::KeyEvent {
                    key: input_key,
                    modifiers: mods,
                    leds: InputKeyboardLedStatus::default(),
                    repeat_count: 1,
                    key_is_down: is_down,
                    raw: None,
                    #[cfg(windows)]
                    win32_uni_char: None,
                };
                return Ok(ev.encode_kitty(flags));
            }
            return Ok(String::new());
        }

        if !is_down {
            // For non-kitty encodings, we only want down events
            return Ok(String::new());
        }

        let mut buf = String::new();

        // TODO: also respect self.application_keypad

        match key {
            Char(c)
                if is_ambiguous_ascii_ctrl(c)
                    && mods.contains(Modifiers::CTRL)
                    && modes.encoding == KeyboardEncoding::CsiU =>
            {
                csi_u_encode(&mut buf, c, mods, &modes)?;
            }
            Char(c) if c.is_ascii_uppercase() && mods.contains(Modifiers::CTRL) => {
                csi_u_encode(&mut buf, c, mods, &modes)?;
            }

            Char(c) if mods.contains(Modifiers::CTRL) && modes.modify_other_keys == Some(2) => {
                csi_u_encode(&mut buf, c, mods, &modes)?;
            }
            Char(c) if mods.contains(Modifiers::CTRL) && ctrl_mapping(c).is_some() => {
                let c = ctrl_mapping(c).unwrap();
                if mods.contains(Modifiers::ALT) {
                    buf.push(0x1b as char);
                }
                buf.push(c);
            }

            // When alt is pressed, send escape first to indicate to the peer that
            // ALT is pressed.  We do this only for ascii alnum characters because
            // eg: on macOS generates altgr style glyphs and keeps the ALT key
            // in the modifier set.  This confuses eg: zsh which then just displays
            // <fffffffff> as the input, so we want to avoid that.
            Char(c)
                if (c.is_ascii_alphanumeric() || c.is_ascii_punctuation())
                    && mods.contains(Modifiers::ALT) =>
            {
                buf.push(0x1b as char);
                buf.push(c);
            }

            Backspace => {
                // Backspace sends the default VERASE which is confusingly
                // the DEL ascii codepoint rather than BS.
                // We only send BS when CTRL is held.
                if mods.contains(Modifiers::CTRL) {
                    csi_u_encode(&mut buf, '\x08', mods, &modes)?;
                } else if mods.contains(Modifiers::SHIFT) {
                    csi_u_encode(&mut buf, '\x7f', mods, &modes)?;
                } else {
                    if mods.contains(Modifiers::ALT) {
                        buf.push(0x1b as char);
                    }
                    buf.push('\x7f');
                }
            }

            Enter | Escape => {
                let c = match key {
                    Enter => '\r',
                    Escape => '\x1b',
                    _ => unreachable!(),
                };
                if mods.contains(Modifiers::SHIFT) || mods.contains(Modifiers::CTRL) {
                    csi_u_encode(&mut buf, c, mods, &modes)?;
                } else {
                    if mods.contains(Modifiers::ALT) {
                        buf.push(0x1b as char);
                    }
                    buf.push(c);
                    if modes.newline_mode && key == Enter {
                        buf.push(0x0a as char);
                    }
                }
            }

            Tab if !mods.is_empty() && modes.modify_other_keys.is_some() => {
                csi_u_encode(&mut buf, '\t', mods, &modes)?;
            }

            Tab => {
                if mods.contains(Modifiers::ALT) {
                    buf.push(0x1b as char);
                }
                let mods = mods & !Modifiers::ALT;
                if mods == Modifiers::CTRL {
                    buf.push_str("\x1b[9;5u");
                } else if mods == Modifiers::CTRL | Modifiers::SHIFT {
                    buf.push_str("\x1b[1;5Z");
                } else if mods == Modifiers::SHIFT {
                    buf.push_str("\x1b[Z");
                } else {
                    buf.push('\t');
                }
            }

            Char(c) => {
                if mods.is_empty() {
                    buf.push(c);
                } else {
                    csi_u_encode(&mut buf, c, mods, &modes)?;
                }
            }

            Home
            | KeyPadHome
            | End
            | KeyPadEnd
            | UpArrow
            | DownArrow
            | RightArrow
            | LeftArrow
            | ApplicationUpArrow
            | ApplicationDownArrow
            | ApplicationRightArrow
            | ApplicationLeftArrow => {
                let (force_app, c) = match key {
                    UpArrow => (false, 'A'),
                    DownArrow => (false, 'B'),
                    RightArrow => (false, 'C'),
                    LeftArrow => (false, 'D'),
                    KeyPadHome | Home => (false, 'H'),
                    End | KeyPadEnd => (false, 'F'),
                    ApplicationUpArrow => (true, 'A'),
                    ApplicationDownArrow => (true, 'B'),
                    ApplicationRightArrow => (true, 'C'),
                    ApplicationLeftArrow => (true, 'D'),
                    _ => unreachable!(),
                };

                let csi_or_ss3 = if force_app
                    || (
                        modes.application_cursor_keys
                        // Strict reading of DECCKM suggests that application_cursor_keys
                        // only applies when DECANM and DECKPAM are active, but that seems
                        // to break unmodified cursor keys in vim
                        /* && self.dec_ansi_mode && self.application_keypad */
                    ) {
                    // Use SS3 in application mode
                    SS3
                } else {
                    // otherwise use regular CSI
                    CSI
                };

                if mods.contains(Modifiers::ALT)
                    || mods.contains(Modifiers::SHIFT)
                    || mods.contains(Modifiers::CTRL)
                {
                    write!(buf, "{}1;{}{}", CSI, 1 + mods.encode_xterm(), c)?;
                } else {
                    write!(buf, "{}{}", csi_or_ss3, c)?;
                }
            }

            PageUp | PageDown | KeyPadPageUp | KeyPadPageDown | Insert | Delete => {
                let c = match key {
                    Insert => 2,
                    Delete => 3,
                    KeyPadPageUp | PageUp => 5,
                    KeyPadPageDown | PageDown => 6,
                    _ => unreachable!(),
                };

                if mods.contains(Modifiers::ALT)
                    || mods.contains(Modifiers::SHIFT)
                    || mods.contains(Modifiers::CTRL)
                {
                    write!(buf, "\x1b[{};{}~", c, 1 + mods.encode_xterm())?;
                } else {
                    write!(buf, "\x1b[{}~", c)?;
                }
            }

            Function(n) => {
                if mods.is_empty() && n < 5 {
                    // F1-F4 are encoded using SS3 if there are no modifiers
                    write!(
                        buf,
                        "{}",
                        match n {
                            1 => "\x1bOP",
                            2 => "\x1bOQ",
                            3 => "\x1bOR",
                            4 => "\x1bOS",
                            _ => unreachable!("wat?"),
                        }
                    )?;
                } else if n < 5 {
                    // Special case for F1-F4 with modifiers
                    let code = match n {
                        1 => 'P',
                        2 => 'Q',
                        3 => 'R',
                        4 => 'S',
                        _ => unreachable!("wat?"),
                    };
                    write!(buf, "\x1b[1;{}{code}", 1 + mods.encode_xterm())?;
                } else {
                    // Higher numbered F-keys using CSI instead of SS3.
                    let intro = match n {
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
                        13 => "\x1b[25",
                        14 => "\x1b[26",
                        15 => "\x1b[28",
                        16 => "\x1b[29",
                        17 => "\x1b[31",
                        18 => "\x1b[32",
                        19 => "\x1b[33",
                        20 => "\x1b[34",
                        21 => "\x1b[42",
                        22 => "\x1b[43",
                        23 => "\x1b[44",
                        24 => "\x1b[45",
                        _ => bail!("unhandled fkey number {}", n),
                    };
                    let encoded_mods = mods.encode_xterm();
                    if encoded_mods == 0 {
                        // If no modifiers are held, don't send the modifier
                        // sequence, as the modifier encoding is a CSI-u extension.
                        write!(buf, "{}~", intro)?;
                    } else {
                        write!(buf, "{};{}~", intro, 1 + encoded_mods)?;
                    }
                }
            }

            Numpad0 | Numpad3 | Numpad9 | Decimal => {
                let intro = match key {
                    Numpad0 => "\x1b[2",
                    Numpad3 => "\x1b[6",
                    Numpad9 => "\x1b[6",
                    Decimal => "\x1b[3",
                    _ => unreachable!(),
                };

                let encoded_mods = mods.encode_xterm();
                if encoded_mods == 0 {
                    // If no modifiers are held, don't send the modifier
                    // sequence, as the modifier encoding is a CSI-u extension.
                    write!(buf, "{}~", intro)?;
                } else {
                    write!(buf, "{};{}~", intro, 1 + encoded_mods)?;
                }
            }

            Numpad1 | Numpad2 | Numpad4 | Numpad5 | KeyPadBegin | Numpad6 | Numpad7 | Numpad8 => {
                let c = match key {
                    Numpad1 => "F",
                    Numpad2 => "B",
                    Numpad4 => "D",
                    KeyPadBegin | Numpad5 => "E",
                    Numpad6 => "C",
                    Numpad7 => "H",
                    Numpad8 => "A",
                    _ => unreachable!(),
                };

                let encoded_mods = mods.encode_xterm();
                if encoded_mods == 0 {
                    // If no modifiers are held, don't send the modifier
                    write!(buf, "{}{}", CSI, c)?;
                } else {
                    write!(buf, "{}1;{}{}", CSI, 1 + encoded_mods, c)?;
                }
            }

            Multiply | Add | Separator | Subtract | Divide => {}

            // Modifier keys pressed on their own don't expand to anything
            Control | LeftControl | RightControl | Alt | LeftAlt | RightAlt | Menu | LeftMenu
            | RightMenu | Super | Hyper | Shift | LeftShift | RightShift | Meta | LeftWindows
            | RightWindows | NumLock | ScrollLock | Cancel | Clear | Pause | CapsLock | Select
            | Print | PrintScreen | Execute | Help | Applications | Sleep | Copy | Cut | Paste
            | BrowserBack | BrowserForward | BrowserRefresh | BrowserStop | BrowserSearch
            | BrowserFavorites | BrowserHome | VolumeMute | VolumeDown | VolumeUp
            | MediaNextTrack | MediaPrevTrack | MediaStop | MediaPlayPause | InternalPasteStart
            | InternalPasteEnd => {}
        };

        Ok(buf)
    }
}

/// characters that when masked for CTRL could be an ascii control character
/// or could be a key that a user legitimately wants to process in their
/// terminal application
fn is_ambiguous_ascii_ctrl(c: char) -> bool {
    matches!(c, 'i' | 'I' | 'm' | 'M' | '[' | '{' | '@')
}

fn is_ascii(c: char) -> bool {
    (c as u32) < 0x80
}

fn csi_u_encode(
    buf: &mut String,
    c: char,
    mods: Modifiers,
    modes: &KeyCodeEncodeModes,
) -> Result<()> {
    if modes.encoding == KeyboardEncoding::CsiU && is_ascii(c) {
        write!(buf, "\x1b[{};{}u", c as u32, 1 + mods.encode_xterm())?;
        return Ok(());
    }

    // <https://invisible-island.net/xterm/modified-keys.html>
    match (c, modes.modify_other_keys) {
        ('c' | 'd' | '\x1b' | '\x7f' | '\x08', Some(1)) => {
            // Exclude well-known keys from modifyOtherKeys mode 1
        }
        (c, Some(_)) => {
            write!(buf, "\x1b[27;{};{}~", 1 + mods.encode_xterm(), c as u32)?;
            return Ok(());
        }
        _ => {}
    }

    let c = if mods.contains(Modifiers::CTRL) && ctrl_mapping(c).is_some() {
        ctrl_mapping(c).unwrap()
    } else {
        c
    };
    if mods.contains(Modifiers::ALT) {
        buf.push(0x1b as char);
    }
    write!(buf, "{}", c)?;
    Ok(())
}
