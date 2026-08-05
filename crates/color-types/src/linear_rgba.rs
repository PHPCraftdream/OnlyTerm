use crate::conversion_tables::{
    linear_f32_to_srgb8, linear_f32_to_srgbf32, rgb_to_linear_f32, srgb8_to_linear_f32,
};
use crate::{SrgbaPixel, SrgbaTuple};
use core::hash::{Hash, Hasher};

/// A pixel value encoded as linear RGBA values in f32 format (range: 0.0-1.0)
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct LinearRgba(pub f32, pub f32, pub f32, pub f32);

impl Eq for LinearRgba {}

impl Hash for LinearRgba {
    fn hash<H>(&self, state: &mut H)
    where
        H: Hasher,
    {
        self.0.to_ne_bytes().hash(state);
        self.1.to_ne_bytes().hash(state);
        self.2.to_ne_bytes().hash(state);
        self.3.to_ne_bytes().hash(state);
    }
}

impl From<(f32, f32, f32, f32)> for LinearRgba {
    fn from((r, g, b, a): (f32, f32, f32, f32)) -> Self {
        Self(r, g, b, a)
    }
}

impl From<[f32; 4]> for LinearRgba {
    fn from([r, g, b, a]: [f32; 4]) -> Self {
        Self(r, g, b, a)
    }
}

impl From<LinearRgba> for [f32; 4] {
    fn from(val: LinearRgba) -> Self {
        [val.0, val.1, val.2, val.3]
    }
}

impl LinearRgba {
    /// Convert SRGBA u8 components to LinearRgba.
    /// Note that alpha in SRGBA colorspace is already linear,
    /// so this only applies gamma correction to RGB.
    pub fn with_srgba(red: u8, green: u8, blue: u8, alpha: u8) -> Self {
        Self(
            srgb8_to_linear_f32(red),
            srgb8_to_linear_f32(green),
            srgb8_to_linear_f32(blue),
            rgb_to_linear_f32(alpha),
        )
    }

    /// Convert linear RGBA u8 components to LinearRgba (f32)
    pub fn with_rgba(red: u8, green: u8, blue: u8, alpha: u8) -> Self {
        Self(
            rgb_to_linear_f32(red),
            rgb_to_linear_f32(green),
            rgb_to_linear_f32(blue),
            rgb_to_linear_f32(alpha),
        )
    }

    /// Create using the provided f32 components in the range 0.0-1.0
    pub const fn with_components(red: f32, green: f32, blue: f32, alpha: f32) -> Self {
        Self(red, green, blue, alpha)
    }

    pub const TRANSPARENT: Self = Self::with_components(0., 0., 0., 0.);

    /// Returns true if this color is fully transparent
    pub fn is_fully_transparent(self) -> bool {
        self.3 == 0.0
    }

    /// Returns self, except when self is transparent, in which case returns other
    pub fn when_fully_transparent(self, other: Self) -> Self {
        if self.is_fully_transparent() {
            other
        } else {
            self
        }
    }

    /// Returns self multiplied by the supplied alpha value
    pub fn mul_alpha(self, alpha: f32) -> Self {
        Self(self.0, self.1, self.2, self.3 * alpha)
    }

    /// Convert to an SRGB u32 pixel
    pub fn srgba_pixel(self) -> SrgbaPixel {
        SrgbaPixel::rgba(
            linear_f32_to_srgb8(self.0),
            linear_f32_to_srgb8(self.1),
            linear_f32_to_srgb8(self.2),
            (self.3 * 255.) as u8,
        )
    }

    /// Returns the individual RGBA channels as f32 components 0.0-1.0
    pub fn tuple(self) -> (f32, f32, f32, f32) {
        (self.0, self.1, self.2, self.3)
    }

    pub fn to_srgb(self) -> SrgbaTuple {
        // Note that alpha is always linear
        SrgbaTuple(
            linear_f32_to_srgbf32(self.0),
            linear_f32_to_srgbf32(self.1),
            linear_f32_to_srgbf32(self.2),
            self.3,
        )
    }

    #[cfg(feature = "std")]
    pub fn relative_luminance(&self) -> f32 {
        0.2126 * self.0 + 0.7152 * self.1 + 0.0722 * self.2
    }

    #[cfg(feature = "std")]
    pub fn contrast_ratio(&self, other: &Self) -> f32 {
        let lum_a = self.relative_luminance();
        let lum_b = other.relative_luminance();
        Self::lum_contrast_ratio(lum_a, lum_b)
    }

    #[cfg(feature = "std")]
    fn lum_contrast_ratio(lum_a: f32, lum_b: f32) -> f32 {
        let a = lum_a + 0.05;
        let b = lum_b + 0.05;
        if a > b {
            a / b
        } else {
            b / a
        }
    }

    #[cfg(feature = "std")]
    fn to_oklaba(self) -> [f32; 4] {
        let (r, g, b, alpha) = (self.0, self.1, self.2, self.3);
        let l_ = (0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
        let m_ = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
        let s_ = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
        let l = 0.210_454_26 * l_ + 0.793_617_8 * m_ - 0.004_072_047 * s_;
        let a = 1.977_998_5 * l_ - 2.428_592_2 * m_ + 0.450_593_7 * s_;
        let b = 0.025_904_037 * l_ + 0.782_771_77 * m_ - 0.808_675_77 * s_;
        [l, a, b, alpha]
    }

    #[cfg(feature = "std")]
    fn from_oklaba(l: f32, a: f32, b: f32, alpha: f32) -> Self {
        let l_ = (l + 0.396_337_78 * a + 0.215_803_76 * b).powi(3);
        let m_ = (l - 0.105_561_346 * a - 0.063_854_17 * b).powi(3);
        let s_ = (l - 0.089_484_18 * a - 1.291_485_5 * b).powi(3);

        let r = 4.076_741_7 * l_ - 3.307_711_6 * m_ + 0.230_969_94 * s_;
        let g = -1.268_438 * l_ + 2.609_757_4 * m_ - 0.341_319_38 * s_;
        let b = -0.0041960863 * l_ - 0.703_418_6 * m_ + 1.707_614_7 * s_;

        Self(r, g, b, alpha)
    }

    /// Assuming that `self` represents the foreground color
    /// and `other` represents the background color, if the
    /// contrast ratio is below min_ratio, returns Some color
    /// that equals or exceeds the min_ratio to use as an alternative
    /// foreground color.
    /// If the ratio is already suitable, returns None; the caller should
    /// continue to use `self` as the foreground color.
    #[cfg(feature = "std")]
    pub fn ensure_contrast_ratio(&self, other: &Self, min_ratio: f32) -> Option<Self> {
        if self == other {
            // Intentionally the same color, don't try to fixup
            return None;
        }

        let fg_lum = self.relative_luminance();
        let bg_lum = other.relative_luminance();
        let ratio = Self::lum_contrast_ratio(fg_lum, bg_lum);
        if ratio >= min_ratio {
            // Already has desired ratio or better
            return None;
        }

        let [_fg_l, fg_a, fg_b, fg_alpha] = self.to_oklaba();

        let reduced_lum = ((bg_lum + 0.05) / min_ratio - 0.05).clamp(0.05, 1.0);
        let reduced_col = Self::from_oklaba(reduced_lum, fg_a, fg_b, fg_alpha);
        let reduced_ratio = reduced_col.contrast_ratio(other);

        let increased_lum = ((bg_lum + 0.05) * min_ratio - 0.05).clamp(0.05, 1.0);
        let increased_col = Self::from_oklaba(increased_lum, fg_a, fg_b, fg_alpha);
        let increased_ratio = reduced_col.contrast_ratio(other);

        // Prefer the reduced luminance version if the fg is dimmer than bg
        if fg_lum < bg_lum && reduced_ratio >= min_ratio {
            return Some(reduced_col);
        }
        // Otherwise, let's find a satisfactory alternative
        if increased_ratio >= min_ratio {
            return Some(increased_col);
        }
        if reduced_ratio >= min_ratio {
            return Some(reduced_col);
        }

        // Didn't find one that satifies the min_ratio, but did we find
        // one that is better than the existing ratio?
        if reduced_ratio > ratio {
            return Some(reduced_col);
        }
        if increased_ratio > ratio {
            return Some(increased_col);
        }

        // What they had was as good as it gets
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nan_component_does_not_panic_converting_to_srgb() {
        // Regression test: a NaN linear-rgb component used to bypass the
        // clamp in `linear_f32_to_srgb8_using_table` (NaN compares false
        // against both bounds) and index the lookup table out of bounds.
        let pixel = LinearRgba(f32::NAN, 0.5, 1.0, 1.0).srgba_pixel();
        // No particular color is "correct" for NaN input; just assert the
        // conversion completed without panicking and returned some pixel.
        let _ = pixel;
    }

    #[cfg(feature = "std")]
    #[test]
    fn linear_rgb_contrast_ratio() {
        let a = LinearRgba::with_srgba(255, 0, 0, 1);
        let b = LinearRgba::with_srgba(0, 255, 0, 1);
        let contrast_ratio = a.contrast_ratio(&b);
        assert!(
            (2.91 - contrast_ratio).abs() < 0.01,
            "contrast({}) == 2.91",
            contrast_ratio
        );
    }
}
