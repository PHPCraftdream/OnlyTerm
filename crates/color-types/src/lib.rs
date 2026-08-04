#![warn(clippy::undocumented_unsafe_blocks)]
#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

mod conversion_tables;
mod linear_rgba;
mod srgba_pixel;
mod srgba_tuple;

#[cfg(feature = "std")]
pub use conversion_tables::linear_u8_to_srgb8;
pub use linear_rgba::LinearRgba;
pub use srgba_pixel::SrgbaPixel;
pub use srgba_tuple::SrgbaTuple;
