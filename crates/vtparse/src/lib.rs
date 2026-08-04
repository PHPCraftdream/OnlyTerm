#![warn(clippy::undocumented_unsafe_blocks)]
//! An implementation of the state machine described by
//! [DEC ANSI Parser](https://vt100.net/emu/dec_ansi_parser), modified to support UTF-8.
//!
//! This is sufficient to broadly categorize ANSI/ECMA-48 escape sequences that are
//! commonly used in terminal emulators.  It does not ascribe semantic meaning to
//! those escape sequences; for example, if you wish to parse the SGR sequence
//! that makes text bold, you will need to know which codes correspond to bold
//! in your implementation of `VTActor`.
//!
//! You may wish to use `termwiz::escape::parser::Parser` in the
//! [termwiz](https://docs.rs/termwiz/) crate if you don't want to have to research
//! all those possible escape sequences for yourself.
#![allow(clippy::upper_case_acronyms)]
#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(any(feature = "std", feature = "alloc"))]
extern crate alloc;

mod actor;
mod collecting;
mod csi_param;
mod enums;
mod parser;
mod transitions;

pub use actor::VTActor;
#[cfg(any(feature = "std", feature = "alloc"))]
pub use collecting::{CollectingVTActor, VTAction};
pub use csi_param::CsiParam;
pub use parser::VTParser;
