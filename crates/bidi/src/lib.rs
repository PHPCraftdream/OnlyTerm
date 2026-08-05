#![no_std]
// LTR/RTL are the domain-standard acronyms the Unicode Bidirectional
// Algorithm (UAX #9) uses. Renaming the enum variants to `Ltr`/`Rtl`
// would harm readability and break the public `DirectionIter` API, so the
// `upper_case_acronyms` lint is suppressed crate-wide -- matching the
// precedent set by the `vtparse` and `wezterm-escape-parser` crates.
#![allow(clippy::upper_case_acronyms)]

extern crate alloc;

// Generated from Unicode data files (BidiBrackets.txt); do not edit by
// hand. Suppress the redundant `'static` lifetime at the module boundary
// so the generated source stays byte-identical to its template.
#[allow(clippy::redundant_static_lifetimes)]
mod bidi_brackets;
// Generated from Unicode data files (DerivedBidiClass.txt); do not edit.
#[allow(clippy::redundant_static_lifetimes)]
mod bidi_class;
mod bidi_class_lookup;
// Generated from Unicode data files (BidiMirroring.txt); do not edit.
mod bidi_context;
#[allow(clippy::redundant_static_lifetimes)]
mod bidi_mirroring;
mod bidi_run;
mod direction;
mod level;
mod level_stack;
mod mirror;
mod paragraph_direction_hint;
mod reordered_run;

pub use bidi_class::BidiClass;
pub use bidi_class_lookup::bidi_class_for_char;
pub use bidi_context::BidiContext;
pub use bidi_run::BidiRun;
pub use direction::Direction;
pub use level::Level;
pub use mirror::mirror_char;
pub use paragraph_direction_hint::ParagraphDirectionHint;
pub use reordered_run::ReorderedRun;

/// Represents a formatting character that has been removed by the X9 rule
pub const NO_LEVEL: i8 = -1;
