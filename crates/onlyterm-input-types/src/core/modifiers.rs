#[cfg(feature = "serde")]
use ::serde::*;
use alloc::format;
use alloc::string::{String, ToString};
use bitflags::*;
use core::convert::TryFrom;
use onlyterm_dynamic::{FromDynamic, ToDynamic};

use crate::UIKeyCapRendering;

bitflags! {
    #[cfg_attr(feature="serde", derive(Serialize, Deserialize))]
    #[derive(Default, FromDynamic, ToDynamic)]
    #[dynamic(into="String", try_from="String")]
    pub struct Modifiers: u16 {
        const NONE = 0;
        const SHIFT = 1<<1;
        const ALT = 1<<2;
        const CTRL = 1<<3;
        const SUPER = 1<<4;
        const LEFT_ALT = 1<<5;
        const RIGHT_ALT = 1<<6;
        /// This is a virtual modifier used by onlyterm
        const LEADER = 1<<7;
        const LEFT_CTRL = 1<<8;
        const RIGHT_CTRL = 1<<9;
        const LEFT_SHIFT = 1<<10;
        const RIGHT_SHIFT = 1<<11;
        const ENHANCED_KEY = 1<<12;
    }
}

impl TryFrom<String> for Modifiers {
    type Error = String;

    fn try_from(s: String) -> Result<Modifiers, String> {
        let mut mods = Modifiers::NONE;
        for ele in s.split('|') {
            // Allow for whitespace; debug printing Modifiers includes spaces
            // around the `|` so it is desirable to be able to reverse that
            // encoding here.
            let ele = ele.trim();
            if ele == "SHIFT" {
                mods |= Modifiers::SHIFT;
            } else if ele == "ALT" || ele == "OPT" || ele == "META" {
                mods |= Modifiers::ALT;
            } else if ele == "CTRL" {
                mods |= Modifiers::CTRL;
            } else if ele == "SUPER" || ele == "CMD" || ele == "WIN" {
                mods |= Modifiers::SUPER;
            } else if ele == "LEADER" {
                mods |= Modifiers::LEADER;
            } else if ele == "NONE" || ele.is_empty() {
                mods |= Modifiers::NONE;
            } else {
                return Err(format!("invalid modifier name {} in {}", ele, s));
            }
        }
        Ok(mods)
    }
}

impl From<&Modifiers> for String {
    fn from(val: &Modifiers) -> Self {
        val.to_string()
    }
}

pub struct ModifierToStringArgs<'a> {
    /// How to join two modifier keys. Can be empty.
    pub separator: &'a str,
    /// Whether to output NONE when no modifiers are present
    pub want_none: bool,
    /// How to render the keycaps for the UI
    pub ui_key_cap_rendering: Option<UIKeyCapRendering>,
}

impl Modifiers {
    pub fn encode_xterm(self) -> u8 {
        let mut number = 0;
        if self.contains(Self::SHIFT) {
            number |= 1;
        }
        if self.contains(Self::ALT) {
            number |= 2;
        }
        if self.contains(Self::CTRL) {
            number |= 4;
        }
        number
    }

    #[allow(non_upper_case_globals)]
    pub fn to_string_with_separator(&self, args: ModifierToStringArgs) -> String {
        let mut s = String::new();
        if args.want_none && *self == Self::NONE {
            s.push_str("NONE");
        }

        // The unicode escapes here are nerdfont symbols; we use those because
        // we're guaranteed to have them available, and the symbols are
        // very legible
        const md_apple_keyboard_command: &str = "\u{f0633}"; // 󰘳
        const md_apple_keyboard_control: &str = "\u{f0634}"; // 󰘴
        const md_apple_keyboard_option: &str = "\u{f0635}"; // 󰘵
        const md_apple_keyboard_shift: &str = "\u{f0636}"; // 󰘶
        const md_microsoft_windows: &str = "\u{f05b3}"; // 󰖳

        for (value, label, unix, emacs, apple, windows, win_sym) in [
            (
                Self::SHIFT,
                "SHIFT",
                "Shift",
                "S",
                md_apple_keyboard_shift,
                "Shift",
                "Shift",
            ),
            (
                Self::ALT,
                "ALT",
                "Alt",
                "M",
                md_apple_keyboard_option,
                "Alt",
                "Alt",
            ),
            (
                Self::CTRL,
                "CTRL",
                "Ctrl",
                "C",
                md_apple_keyboard_control,
                "Ctrl",
                "Ctrl",
            ),
            (
                Self::SUPER,
                "SUPER",
                "Super",
                "Super",
                md_apple_keyboard_command,
                "Win",
                md_microsoft_windows,
            ),
            (
                Self::LEFT_ALT,
                "LEFT_ALT",
                "Alt",
                "M",
                md_apple_keyboard_option,
                "Alt",
                "Alt",
            ),
            (
                Self::RIGHT_ALT,
                "RIGHT_ALT",
                "Alt",
                "M",
                md_apple_keyboard_option,
                "Alt",
                "Alt",
            ),
            (
                Self::LEADER,
                "LEADER",
                "Leader",
                "Leader",
                "Leader",
                "Leader",
                "Leader",
            ),
            (
                Self::LEFT_CTRL,
                "LEFT_CTRL",
                "Ctrl",
                "C",
                md_apple_keyboard_control,
                "Ctrl",
                "Ctrl",
            ),
            (
                Self::RIGHT_CTRL,
                "RIGHT_CTRL",
                "Ctrl",
                "C",
                md_apple_keyboard_control,
                "Ctrl",
                "Ctrl",
            ),
            (
                Self::LEFT_SHIFT,
                "LEFT_SHIFT",
                "Shift",
                "S",
                md_apple_keyboard_shift,
                "Shift",
                "Shift",
            ),
            (
                Self::RIGHT_SHIFT,
                "RIGHT_SHIFT",
                "Shift",
                "S",
                md_apple_keyboard_shift,
                "Shift",
                "Shift",
            ),
            (
                Self::ENHANCED_KEY,
                "ENHANCED_KEY",
                "ENHANCED_KEY",
                "ENHANCED_KEY",
                "ENHANCED_KEY",
                "ENHANCED_KEY",
                "ENHANCED_KEY",
            ),
        ] {
            if !self.contains(value) {
                continue;
            }
            if !s.is_empty() {
                s.push_str(args.separator);
            }
            s.push_str(match args.ui_key_cap_rendering {
                Some(UIKeyCapRendering::UnixLong) => unix,
                Some(UIKeyCapRendering::Emacs) => emacs,
                Some(UIKeyCapRendering::AppleSymbols) => apple,
                Some(UIKeyCapRendering::WindowsLong) => windows,
                Some(UIKeyCapRendering::WindowsSymbols) => win_sym,
                None => label,
            });
        }

        s
    }
}

impl core::fmt::Display for Modifiers {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        f.write_str(&self.to_string_with_separator(ModifierToStringArgs {
            separator: "|",
            want_none: true,
            ui_key_cap_rendering: None,
        }))
    }
}

impl Modifiers {
    /// Remove positional and other "supplemental" bits that
    /// are used to carry around implementation details, but that
    /// are not bits that should be matched when matching key
    /// assignments.
    pub fn remove_positional_mods(self) -> Self {
        self - (Self::LEFT_ALT
            | Self::RIGHT_ALT
            | Self::LEFT_CTRL
            | Self::RIGHT_CTRL
            | Self::LEFT_SHIFT
            | Self::RIGHT_SHIFT
            | Self::ENHANCED_KEY)
    }
}
