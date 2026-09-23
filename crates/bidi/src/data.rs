//! Unicode data tables for the bidi algorithm and their lookup accessors.

// Generated from Unicode data files (BidiBrackets.txt); do not edit by
// hand. Suppress the redundant `'static` lifetime at the module boundary
// so the generated source stays byte-identical to its template.
#[allow(clippy::redundant_static_lifetimes)]
pub(crate) mod bidi_brackets;
// Generated from Unicode data files (DerivedBidiClass.txt); do not edit.
#[allow(clippy::redundant_static_lifetimes)]
pub(crate) mod bidi_class;
pub(crate) mod bidi_class_lookup;
// Generated from Unicode data files (BidiMirroring.txt); do not edit.
#[allow(clippy::redundant_static_lifetimes)]
pub(crate) mod bidi_mirroring;
pub(crate) mod mirror;
