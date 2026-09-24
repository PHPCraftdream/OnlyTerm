use crate::{LinearRgba, SrgbaTuple};

/// A pixel holding SRGBA32 data in big endian format
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct SrgbaPixel(u32);

impl SrgbaPixel {
    /// Create a pixel with the provided sRGBA values in u8 format
    pub fn rgba(red: u8, green: u8, blue: u8, alpha: u8) -> Self {
        #[allow(clippy::cast_lossless)]
        let word = (blue as u32) << 24 | (green as u32) << 16 | (red as u32) << 8 | alpha as u32;
        Self(word.to_be())
    }

    /// Returns the unpacked sRGBA components as u8
    #[inline]
    pub fn as_rgba(self) -> (u8, u8, u8, u8) {
        let host = u32::from_be(self.0);
        (
            (host >> 8) as u8,
            (host >> 16) as u8,
            (host >> 24) as u8,
            (host & 0xff) as u8,
        )
    }

    /// Returns RGBA channels in linear f32 format
    pub fn to_linear(self) -> LinearRgba {
        let (r, g, b, a) = self.as_rgba();
        LinearRgba::with_srgba(r, g, b, a)
    }

    /// Create a pixel with the provided big-endian u32 SRGBA data
    pub fn with_srgba_u32(word: u32) -> Self {
        Self(word)
    }

    /// Returns the underlying big-endian u32 SRGBA data
    pub fn as_srgba32(self) -> u32 {
        self.0
    }

    pub fn as_srgba_tuple(self) -> (f32, f32, f32, f32) {
        let u8tuple = self.as_rgba();
        let SrgbaTuple(r, g, b, a) = u8tuple.into();
        (r, g, b, a)
    }
}
