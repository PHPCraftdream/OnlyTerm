#[cfg(feature = "serde")]
use ::serde::*;
use alloc::string::{String, ToString};
use alloc::{format, vec};
use bitflags::*;
use core::convert::TryFrom;
use onlyterm_dynamic::{FromDynamic, ToDynamic};

bitflags! {
    #[derive(FromDynamic, ToDynamic)]
    #[cfg_attr(feature="serde", derive(Serialize, Deserialize), serde(try_from = "String"))]
    #[dynamic(try_from = "String", into = "String")]
    pub struct WindowDecorations: u8 {
        const TITLE = 1;
        const RESIZE = 2;
        const NONE = 0;
        // Reserve two bits for this enable/disable shadow,
        // so that we effective have Option<bool>
        const MACOS_FORCE_DISABLE_SHADOW = 4;
        const MACOS_FORCE_ENABLE_SHADOW = 4|8;
        const INTEGRATED_BUTTONS = 16;
        const MACOS_FORCE_SQUARE_CORNERS = 32;
        const MACOS_USE_BACKGROUND_COLOR_AS_TITLEBAR_COLOR = 64;
    }
}

impl From<&WindowDecorations> for String {
    fn from(val: &WindowDecorations) -> Self {
        let mut s = vec![];
        if val.contains(WindowDecorations::TITLE) {
            s.push("TITLE");
        }
        if val.contains(WindowDecorations::RESIZE) {
            s.push("RESIZE");
        }
        if val.contains(WindowDecorations::INTEGRATED_BUTTONS) {
            s.push("INTEGRATED_BUTTONS");
        }
        if val.contains(WindowDecorations::MACOS_USE_BACKGROUND_COLOR_AS_TITLEBAR_COLOR) {
            s.push("MACOS_USE_BACKGROUND_COLOR_AS_TITLEBAR_COLOR")
        }
        if val.contains(WindowDecorations::MACOS_FORCE_ENABLE_SHADOW) {
            s.push("MACOS_FORCE_ENABLE_SHADOW");
        } else if val.contains(WindowDecorations::MACOS_FORCE_DISABLE_SHADOW) {
            s.push("MACOS_FORCE_DISABLE_SHADOW");
        } else if val.contains(WindowDecorations::MACOS_FORCE_SQUARE_CORNERS) {
            s.push("MACOS_FORCE_SQUARE_CORNERS");
        }
        if s.is_empty() {
            "NONE".to_string()
        } else {
            s.join("|")
        }
    }
}

impl TryFrom<String> for WindowDecorations {
    type Error = String;
    fn try_from(s: String) -> core::result::Result<WindowDecorations, String> {
        let mut flags = Self::NONE;
        for ele in s.split('|') {
            let ele = ele.trim();
            if ele == "TITLE" {
                flags |= Self::TITLE;
            } else if ele == "NONE" || ele == "None" {
                flags = Self::NONE;
            } else if ele == "RESIZE" {
                flags |= Self::RESIZE;
            } else if ele == "MACOS_USE_BACKGROUND_COLOR_AS_TITLEBAR_COLOR" {
                flags |= Self::MACOS_USE_BACKGROUND_COLOR_AS_TITLEBAR_COLOR;
            } else if ele == "MACOS_FORCE_DISABLE_SHADOW" {
                flags |= Self::MACOS_FORCE_DISABLE_SHADOW;
            } else if ele == "MACOS_FORCE_ENABLE_SHADOW" {
                flags |= Self::MACOS_FORCE_ENABLE_SHADOW;
            } else if ele == "MACOS_FORCE_SQUARE_CORNERS" {
                flags |= Self::MACOS_FORCE_SQUARE_CORNERS;
            } else if ele == "INTEGRATED_BUTTONS" {
                flags |= Self::INTEGRATED_BUTTONS;
            } else {
                return Err(format!("invalid WindowDecoration name {} in {}", ele, s));
            }
        }
        Ok(flags)
    }
}

impl Default for WindowDecorations {
    fn default() -> Self {
        WindowDecorations::TITLE | WindowDecorations::RESIZE
    }
}
