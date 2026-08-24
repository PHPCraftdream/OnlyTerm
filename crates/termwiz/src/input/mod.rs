//! This module provides an InputParser struct to help with parsing
//! input received from a terminal.

mod events;
mod key_code;
mod parser;

pub use events::{InputEvent, KeyEvent, MouseEvent, PixelMouseEvent};
pub use key_code::{KeyCode, KeyCodeEncodeModes, KeyboardEncoding, CSI, SS3};
pub use parser::InputParser;

pub use onlyterm_escape_parser::csi::MouseButtons;
pub use onlyterm_input_types::Modifiers;

#[cfg(test)]
mod test;
