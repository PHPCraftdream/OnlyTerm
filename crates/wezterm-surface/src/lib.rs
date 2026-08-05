#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;
// The crate is `#![no_std]`; the std prelude and its macros (`format!`,
// `vec!`, ...) are not in scope. The unit tests rely on them, and the test
// harness always links std, so pull the macros in for test builds only.
#[cfg(test)]
#[macro_use]
extern crate std;

pub mod cellcluster;
pub mod change;
mod cursor;
pub mod hyperlink;
pub mod line;
mod position;
mod surface;

pub use cursor::{CursorShape, CursorVisibility};
pub use position::Position;
pub use surface::Surface;

pub use self::change::{Change, LineAttribute};
#[cfg(feature = "use_image")]
pub use self::change::{Image, TextureCoordinate};
pub use self::line::Line;

/// SequenceNo indicates a logical position within a stream of changes.
/// The sequence is only meaningful within a given `Surface` instance.
pub type SequenceNo = usize;
pub const SEQ_ZERO: SequenceNo = 0;
