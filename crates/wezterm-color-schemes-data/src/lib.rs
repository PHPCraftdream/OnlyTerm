//! Generated built-in color scheme data.
//!
//! Kept in its own crate so updates to configuration logic do not recompile
//! the large generated table, and vice versa.

mod scheme_data;

pub use scheme_data::SCHEMES;
