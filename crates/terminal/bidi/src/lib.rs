#![no_std]
// LTR/RTL are the domain-standard acronyms the Unicode Bidirectional
// Algorithm (UAX #9) uses. Renaming the enum variants to `Ltr`/`Rtl`
// would harm readability and break the public `DirectionIter` API, so the
// `upper_case_acronyms` lint is suppressed crate-wide -- matching the
// precedent set by the `vtparse` and `onlyterm-escape-parser` crates.
#![allow(clippy::upper_case_acronyms)]

extern crate alloc;

mod bidi_context;
mod data;
mod types;

pub use bidi_context::BidiContext;
pub use data::bidi_class::BidiClass;
pub use data::bidi_class_lookup::bidi_class_for_char;
pub use data::mirror::mirror_char;
pub use types::bidi_run::BidiRun;
pub use types::direction::Direction;
pub use types::level::Level;
pub use types::paragraph_direction_hint::ParagraphDirectionHint;
pub use types::reordered_run::ReorderedRun;

/// Represents a formatting character that has been removed by the X9 rule
pub const NO_LEVEL: i8 = -1;
