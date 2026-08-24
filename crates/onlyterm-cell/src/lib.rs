#![warn(clippy::undocumented_unsafe_blocks)]
#![cfg_attr(not(feature = "std"), no_std)]
//! Model a cell in the terminal display

extern crate alloc;
// The crate is `#![no_std]`, so the std prelude and its macros (`vec!`,
// `eprintln!`, ...) are not in scope. The unit tests below rely on them, and
// the test harness always links std, so pull the macros in for test builds
// only. This is the standard pattern for `no_std` crates with unit tests.
#[cfg(test)]
#[macro_use]
extern crate std;

mod attribute_change;
mod attributes;
mod cell;
pub mod color;
#[cfg(feature = "use_image")]
pub mod image;
#[cfg(test)]
mod test;
mod unicode;

pub use attribute_change::AttributeChange;
pub use attributes::{CellAttributes, SemanticType};
pub use cell::Cell;
pub use onlyterm_char_props::emoji::Presentation;
pub use onlyterm_escape_parser::csi::{Blink, Intensity, Underline, VerticalAlign};
pub use onlyterm_escape_parser::osc::Hyperlink;
pub use unicode::{
    LATEST_UNICODE_VERSION, UnicodeVersion, grapheme_column_width, is_white_space_char,
    is_white_space_grapheme, unicode_column_width,
};
