// SAFETY: `generated` below is entirely `gl_generator` build-script output;
// its `unsafe impl Send` markers are generated code, not hand-written here.
mod connection;
#[allow(
    non_camel_case_types,
    clippy::unreadable_literal,
    clippy::undocumented_unsafe_blocks,
    clippy::missing_transmute_annotations
)]
pub mod ffi;
mod state;
mod wrapper;

pub use connection::GlConnection;
pub use state::GlState;
