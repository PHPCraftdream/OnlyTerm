#[cfg(feature = "use_serde")]
use serde::{Deserialize, Serialize};

use super::{KeyCode, Modifiers, MouseButtons};

#[cfg_attr(feature = "use_serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputEvent {
    Key(KeyEvent),
    Mouse(MouseEvent),
    PixelMouse(PixelMouseEvent),
    /// Detected that the user has resized the terminal
    Resized {
        cols: usize,
        rows: usize,
    },
    /// For terminals that support Bracketed Paste mode,
    /// pastes are collected and reported as this variant.
    Paste(String),
    /// The program has woken the input thread.
    Wake,
}

#[cfg_attr(feature = "use_serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MouseEvent {
    pub x: u16,
    pub y: u16,
    pub mouse_buttons: MouseButtons,
    pub modifiers: Modifiers,
}

#[cfg_attr(feature = "use_serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PixelMouseEvent {
    pub x_pixels: u16,
    pub y_pixels: u16,
    pub mouse_buttons: MouseButtons,
    pub modifiers: Modifiers,
}

#[cfg_attr(feature = "use_serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyEvent {
    /// Which key was pressed
    pub key: KeyCode,

    /// Which modifiers are down
    pub modifiers: Modifiers,
}
