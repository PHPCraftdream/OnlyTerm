use alloc::string::ToString;
use wezterm_dynamic::{FromDynamic, ToDynamic};

#[derive(Debug, Default, FromDynamic, ToDynamic, Clone, Copy, PartialEq, Eq)]
pub enum UIKeyCapRendering {
    /// Super, Meta, Ctrl, Shift
    UnixLong,
    /// Super, M, C, S
    Emacs,
    /// Apple macOS style symbols
    AppleSymbols,
    /// Win, Alt, Ctrl, Shift
    WindowsLong,
    /// Like WindowsLong, but using a logo for the Win key
    #[default]
    WindowsSymbols,
}
