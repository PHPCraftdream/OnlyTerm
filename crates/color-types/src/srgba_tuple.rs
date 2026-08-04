use crate::SrgbaPixel;
use core::hash::{Hash, Hasher};
use core::str::FromStr;
#[cfg(feature = "std")]
use csscolorparser::Color;
#[cfg(not(feature = "std"))]
#[allow(unused)]
use num_traits::float::Float;
#[cfg(feature = "use_serde")]
use serde::{Deserialize, Serialize};
#[cfg(feature = "std")]
use std::collections::HashMap;
#[cfg(feature = "std")]
use std::sync::LazyLock;
use wezterm_dynamic::{FromDynamic, FromDynamicOptions, ToDynamic, Value};

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// A pixel value encoded as SRGBA RGBA values in f32 format (range: 0.0-1.0)
#[derive(Copy, Clone, Debug, Default, PartialEq)]
#[cfg_attr(feature = "use_serde", derive(Serialize, Deserialize))]
pub struct SrgbaTuple(pub f32, pub f32, pub f32, pub f32);

impl SrgbaTuple {
    pub fn premultiply(self) -> Self {
        let SrgbaTuple(r, g, b, a) = self;
        Self(r * a, g * a, b * a, a)
    }

    pub fn demultiply(self) -> Self {
        let SrgbaTuple(r, g, b, a) = self;
        if a != 0. {
            Self(r / a, g / a, b / a, a)
        } else {
            self
        }
    }

    pub fn to_tuple_rgba(self) -> (f32, f32, f32, f32) {
        (self.0, self.1, self.2, self.3)
    }

    pub fn as_rgba_u8(self) -> (u8, u8, u8, u8) {
        let (r, g, b, a) = (self.0, self.1, self.2, self.3);
        (
            (r * 255.0) as u8,
            (g * 255.0) as u8,
            (b * 255.0) as u8,
            (a * 255.0) as u8,
        )
    }

    pub fn interpolate(self, other: Self, k: f64) -> Self {
        let k = k as f32;

        let SrgbaTuple(r0, g0, b0, a0) = self.premultiply();
        let SrgbaTuple(r1, g1, b1, a1) = other.premultiply();

        let r = SrgbaTuple(
            r0 + k * (r1 - r0),
            g0 + k * (g1 - g0),
            b0 + k * (b1 - b0),
            a0 + k * (a1 - a0),
        );

        r.demultiply()
    }
}

impl core::fmt::Display for SrgbaTuple {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if self.3 == 1.0 {
            f.write_str(&self.to_rgb_string())
        } else {
            f.write_str(&self.to_rgba_string())
        }
    }
}

impl ToDynamic for SrgbaTuple {
    fn to_dynamic(&self) -> Value {
        self.to_string().to_dynamic()
    }
}

impl FromDynamic for SrgbaTuple {
    fn from_dynamic(
        value: &Value,
        options: FromDynamicOptions,
    ) -> Result<Self, wezterm_dynamic::Error> {
        let s = String::from_dynamic(value, options)?;
        Ok(SrgbaTuple::from_str(&s).map_err(|()| format!("unknown color name: {}", s))?)
    }
}

impl From<SrgbaPixel> for SrgbaTuple {
    fn from(pixel: SrgbaPixel) -> SrgbaTuple {
        let (r, g, b, a) = pixel.as_srgba_tuple();
        SrgbaTuple(r, g, b, a)
    }
}

impl From<(f32, f32, f32, f32)> for SrgbaTuple {
    fn from((r, g, b, a): (f32, f32, f32, f32)) -> SrgbaTuple {
        SrgbaTuple(r, g, b, a)
    }
}

impl From<(u8, u8, u8, u8)> for SrgbaTuple {
    fn from((r, g, b, a): (u8, u8, u8, u8)) -> SrgbaTuple {
        SrgbaTuple(
            r as f32 / 255.,
            g as f32 / 255.,
            b as f32 / 255.,
            a as f32 / 255.,
        )
    }
}

impl From<(u8, u8, u8)> for SrgbaTuple {
    fn from((r, g, b): (u8, u8, u8)) -> SrgbaTuple {
        SrgbaTuple(r as f32 / 255., g as f32 / 255., b as f32 / 255., 1.0)
    }
}

impl From<SrgbaTuple> for (f32, f32, f32, f32) {
    fn from(t: SrgbaTuple) -> (f32, f32, f32, f32) {
        (t.0, t.1, t.2, t.3)
    }
}

#[cfg(feature = "std")]
impl From<Color> for SrgbaTuple {
    fn from(color: Color) -> Self {
        Self(
            color.r as f32,
            color.g as f32,
            color.b as f32,
            color.a as f32,
        )
    }
}

#[cfg(feature = "std")]
static NAMED_COLORS: LazyLock<HashMap<String, SrgbaTuple>> = LazyLock::new(build_colors);

const RGB_TXT: &str = core::include_str!("rgb.txt");

fn iter_rgb_txt(mut func: impl FnMut(&str, SrgbaTuple) -> bool) {
    let transparent = SrgbaTuple(0., 0., 0., 0.);
    for name in &["transparent", "none", "clear"] {
        if (func)(name, transparent) {
            return;
        }
    }

    for line in RGB_TXT.lines() {
        let mut fields = line.split_ascii_whitespace();
        let red = fields.next().unwrap();
        let green = fields.next().unwrap();
        let blue = fields.next().unwrap();
        let name = fields.collect::<Vec<&str>>().join(" ");

        let name = name.to_ascii_lowercase();
        let color = SrgbaTuple(
            red.parse::<f32>().unwrap() / 255.,
            green.parse::<f32>().unwrap() / 255.,
            blue.parse::<f32>().unwrap() / 255.,
            1.0,
        );

        if (func)(&name, color) {
            return;
        }
    }
}

#[cfg(feature = "std")]
fn build_colors() -> HashMap<String, SrgbaTuple> {
    let mut map = HashMap::new();

    iter_rgb_txt(|name, color| {
        map.insert(name.to_string(), color);
        false
    });
    map
}

impl SrgbaTuple {
    /// Construct a color from an X11/SVG/CSS3 color name.
    /// Returns None if the supplied name is not recognized.
    /// The list of names can be found here:
    /// <https://en.wikipedia.org/wiki/X11_color_names>
    pub fn from_named(name: &str) -> Option<Self> {
        #[cfg(feature = "std")]
        {
            NAMED_COLORS.get(&name.to_ascii_lowercase()).cloned()
        }
        #[cfg(not(feature = "std"))]
        {
            let mut result = None;
            iter_rgb_txt(|candidate, color| {
                if candidate.eq_ignore_ascii_case(name) {
                    result.replace(color);
                    true
                } else {
                    false
                }
            });
            result
        }
    }

    /// Returns self multiplied by the supplied alpha value.
    /// We don't need to linearize for this, as alpha is defined
    /// as being linear even in srgba!
    pub fn mul_alpha(self, alpha: f32) -> Self {
        Self(self.0, self.1, self.2, self.3 * alpha)
    }

    pub fn to_linear(self) -> crate::LinearRgba {
        // See https://docs.rs/palette/0.5.0/src/palette/encoding/srgb.rs.html#43
        fn to_linear(v: f32) -> f32 {
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        }
        // Note that alpha is always linear
        crate::LinearRgba(
            to_linear(self.0),
            to_linear(self.1),
            to_linear(self.2),
            self.3,
        )
    }

    pub fn to_srgb_u8(self) -> (u8, u8, u8, u8) {
        (
            (self.0 * 255.) as u8,
            (self.1 * 255.) as u8,
            (self.2 * 255.) as u8,
            (self.3 * 255.) as u8,
        )
    }

    /// Returns a string of the form `#RRGGBB`
    pub fn to_rgb_string(self) -> String {
        format!(
            "#{:02x}{:02x}{:02x}",
            (self.0 * 255.) as u8,
            (self.1 * 255.) as u8,
            (self.2 * 255.) as u8
        )
    }

    pub fn to_rgba_string(self) -> String {
        format!(
            "rgba({}% {}% {}% {}%)",
            (self.0 * 100.),
            (self.1 * 100.),
            (self.2 * 100.),
            (self.3 * 100.)
        )
    }

    /// Returns a string of the form `rgb:RRRR/GGGG/BBBB`
    pub fn to_x11_16bit_rgb_string(self) -> String {
        format!(
            "rgb:{:04x}/{:04x}/{:04x}",
            (self.0 * 65535.) as u16,
            (self.1 * 65535.) as u16,
            (self.2 * 65535.) as u16
        )
    }

    #[cfg(feature = "std")]
    pub fn to_laba(self) -> (f64, f64, f64, f64) {
        Color::new(self.0.into(), self.1.into(), self.2.into(), self.3.into()).to_lab()
    }

    #[cfg(feature = "std")]
    pub fn to_hsla(self) -> (f64, f64, f64, f64) {
        Color::new(self.0.into(), self.1.into(), self.2.into(), self.3.into()).to_hsla()
    }

    #[cfg(feature = "std")]
    pub fn from_hsla(h: f64, s: f64, l: f64, a: f64) -> Self {
        let Color { r, g, b, a } = Color::from_hsla(h, s, l, a);
        Self(r as f32, g as f32, b as f32, a as f32)
    }

    /// Scale the color towards the maximum saturation by factor, a value ranging from 0.0 to 1.0.
    #[cfg(feature = "std")]
    pub fn saturate(&self, factor: f64) -> Self {
        let (h, s, l, a) = self.to_hsla();
        let s = apply_scale(s, factor);
        Self::from_hsla(h, s, l, a)
    }

    /// Increase the saturation by amount, a value ranging from 0.0 to 1.0.
    #[cfg(feature = "std")]
    pub fn saturate_fixed(&self, amount: f64) -> Self {
        let (h, s, l, a) = self.to_hsla();
        let s = apply_fixed(s, amount);
        Self::from_hsla(h, s, l, a)
    }

    /// Scale the color towards the maximum lightness by factor, a value ranging from 0.0 to 1.0
    #[cfg(feature = "std")]
    pub fn lighten(&self, factor: f64) -> Self {
        let (h, s, l, a) = self.to_hsla();
        let l = apply_scale(l, factor);
        Self::from_hsla(h, s, l, a)
    }

    /// Lighten the color by amount, a value ranging from 0.0 to 1.0
    #[cfg(feature = "std")]
    pub fn lighten_fixed(&self, amount: f64) -> Self {
        let (h, s, l, a) = self.to_hsla();
        let l = apply_fixed(l, amount);
        Self::from_hsla(h, s, l, a)
    }

    /// Rotate the hue angle by the specified number of degrees
    #[cfg(feature = "std")]
    pub fn adjust_hue_fixed(&self, amount: f64) -> Self {
        let (h, s, l, a) = self.to_hsla();
        let h = normalize_angle(h + amount);
        Self::from_hsla(h, s, l, a)
    }

    #[cfg(feature = "std")]
    pub fn complement(&self) -> Self {
        self.adjust_hue_fixed(180.)
    }

    #[cfg(feature = "std")]
    pub fn complement_ryb(&self) -> Self {
        self.adjust_hue_fixed_ryb(180.)
    }

    #[cfg(feature = "std")]
    pub fn triad(&self) -> (Self, Self) {
        (self.adjust_hue_fixed(120.), self.adjust_hue_fixed(-120.))
    }

    #[cfg(feature = "std")]
    pub fn square(&self) -> (Self, Self, Self) {
        (
            self.adjust_hue_fixed(90.),
            self.adjust_hue_fixed(270.),
            self.adjust_hue_fixed(180.),
        )
    }

    /// Rotate the hue angle by the specified number of degrees, using
    /// the RYB color wheel
    #[cfg(feature = "std")]
    pub fn adjust_hue_fixed_ryb(&self, amount: f64) -> Self {
        let (h, s, l, a) = self.to_hsla();
        let h = rgb_hue_to_ryb_hue(h);
        let h = normalize_angle(h + amount);
        let h = ryb_huge_to_rgb_hue(h);
        Self::from_hsla(h, s, l, a)
    }

    #[cfg(feature = "std")]
    fn lab_value(&self) -> deltae::LabValue {
        let (l, a, b, _alpha) = self.to_laba();
        deltae::LabValue {
            l: l as f32,
            a: a as f32,
            b: b as f32,
        }
    }

    #[cfg(feature = "std")]
    pub fn delta_e(&self, other: &Self) -> f32 {
        let a = self.lab_value();
        let b = other.lab_value();
        *deltae::DeltaE::new(a, b, deltae::DEMethod::DE2000).value()
    }

    #[cfg(feature = "std")]
    pub fn contrast_ratio(&self, other: &Self) -> f32 {
        self.to_linear().contrast_ratio(&other.to_linear())
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
        self.to_linear()
            .ensure_contrast_ratio(&other.to_linear(), min_ratio)
            .map(|linear| linear.to_srgb())
    }
}

/// Convert an RGB color space hue angle to an RYB colorspace hue angle
/// <https://github.com/TNMEM/Material-Design-Color-Picker/blob/1afe330c67d9db4deef7031d601324b538b43b09/rybcolor.js#L33>
#[cfg(feature = "std")]
fn rgb_hue_to_ryb_hue(hue: f64) -> f64 {
    if hue < 35. {
        map_range(hue, 0., 35., 0., 60.)
    } else if hue < 60. {
        map_range(hue, 35., 60., 60., 122.)
    } else if hue < 120. {
        map_range(hue, 60., 120., 122., 165.)
    } else if hue < 180. {
        map_range(hue, 120., 180., 165., 218.)
    } else if hue < 240. {
        map_range(hue, 180., 240., 218., 275.)
    } else if hue < 300. {
        map_range(hue, 240., 300., 275., 330.)
    } else {
        map_range(hue, 300., 360., 330., 360.)
    }
}

/// Convert an RYB color space hue angle to an RGB colorspace hue angle
#[cfg(feature = "std")]
fn ryb_huge_to_rgb_hue(hue: f64) -> f64 {
    if hue < 60. {
        map_range(hue, 0., 60., 0., 35.)
    } else if hue < 122. {
        map_range(hue, 60., 122., 35., 60.)
    } else if hue < 165. {
        map_range(hue, 122., 165., 60., 120.)
    } else if hue < 218. {
        map_range(hue, 165., 218., 120., 180.)
    } else if hue < 275. {
        map_range(hue, 218., 275., 180., 240.)
    } else if hue < 330. {
        map_range(hue, 275., 330., 240., 300.)
    } else {
        map_range(hue, 330., 360., 300., 360.)
    }
}

#[cfg(feature = "std")]
fn map_range(x: f64, x1: f64, x2: f64, y1: f64, y2: f64) -> f64 {
    let a_slope = (y2 - y1) / (x2 - x1);
    let a_slope_intercept = y1 - (a_slope * x1);
    x * a_slope + a_slope_intercept
}

#[cfg(feature = "std")]
fn normalize_angle(t: f64) -> f64 {
    let mut t = t % 360.0;
    if t < 0.0 {
        t += 360.0;
    }
    t
}

#[cfg(feature = "std")]
fn apply_scale(current: f64, factor: f64) -> f64 {
    let difference = if factor >= 0. { 1.0 - current } else { current };
    let delta = difference.max(0.) * factor;
    (current + delta).max(0.)
}

#[cfg(feature = "std")]
fn apply_fixed(current: f64, amount: f64) -> f64 {
    (current + amount).max(0.)
}

impl Hash for SrgbaTuple {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.to_ne_bytes().hash(state);
        self.1.to_ne_bytes().hash(state);
        self.2.to_ne_bytes().hash(state);
        self.3.to_ne_bytes().hash(state);
    }
}

impl Eq for SrgbaTuple {}

fn x_parse_color_component(value: &str) -> Result<f32, ()> {
    let mut component = 0u16;
    let mut num_digits = 0;

    for c in value.chars() {
        num_digits += 1;
        component <<= 4;

        let nybble = match c.to_digit(16) {
            Some(v) => v as u16,
            None => return Err(()),
        };
        component |= nybble;
    }

    // From XParseColor, the `rgb:` prefixed syntax scales the
    // value into 16 bits from the number of bits specified
    Ok((match num_digits {
        1 => (component | component << 4) as f32,
        2 => component as f32,
        3 => (component >> 4) as f32,
        4 => (component >> 8) as f32,
        _ => return Err(()),
    }) / 255.0)
}

impl FromStr for SrgbaTuple {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Workaround <https://github.com/mazznoer/csscolorparser-rs/pull/7/files>
        if !s.is_ascii() {
            return Err(());
        }
        if !s.is_empty() && s.as_bytes()[0] == b'#' {
            // Probably `#RGB`

            let digits = (s.len() - 1) / 3;
            if 1 + (digits * 3) != s.len() {
                return Err(());
            }

            if digits == 0 || digits > 4 {
                // Max of 16 bits supported
                return Err(());
            }

            let mut chars = s.chars().skip(1);

            macro_rules! digit {
                () => {{
                    let mut component = 0u16;

                    for _ in 0..digits {
                        component <<= 4;

                        let nybble = match chars.next().unwrap().to_digit(16) {
                            Some(v) => v as u16,
                            None => return Err(()),
                        };
                        component |= nybble;
                    }

                    // From XParseColor, the `#` syntax takes the most significant
                    // bits and uses those for the color value.  That function produces
                    // 16-bit color components but we want 8-bit components so we shift
                    // or truncate the bits here depending on the number of digits
                    (match digits {
                        1 => (component << 4) as f32,
                        2 => component as f32,
                        3 => (component >> 4) as f32,
                        4 => (component >> 8) as f32,
                        _ => return Err(()),
                    }) / 255.0
                }};
            }
            Ok(Self(digit!(), digit!(), digit!(), 1.0))
        } else if let Some(value) = s.strip_prefix("rgb:") {
            let fields: Vec<&str> = value.split('/').collect();
            if fields.len() != 3 {
                return Err(());
            }

            let red = x_parse_color_component(fields[0])?;
            let green = x_parse_color_component(fields[1])?;
            let blue = x_parse_color_component(fields[2])?;
            Ok(Self(red, green, blue, 1.0))
        } else if let Some(value) = s.strip_prefix("rgba:") {
            let fields: Vec<&str> = value.split('/').collect();
            if fields.len() == 4 {
                let red = x_parse_color_component(fields[0])?;
                let green = x_parse_color_component(fields[1])?;
                let blue = x_parse_color_component(fields[2])?;
                let alpha = x_parse_color_component(fields[3])?;
                return Ok(Self(red, green, blue, alpha));
            }

            let fields: Vec<_> = s[5..].split_ascii_whitespace().collect();
            if fields.len() == 4 {
                fn field(s: &str) -> Result<f32, ()> {
                    if s.ends_with('%') {
                        let v: f32 = s[0..s.len() - 1].parse().map_err(|_| ())?;
                        Ok(v / 100.)
                    } else {
                        let v: f32 = s.parse().map_err(|_| ())?;
                        if !(0. ..=255.0).contains(&v) {
                            Err(())
                        } else {
                            Ok(v / 255.)
                        }
                    }
                }
                let r: f32 = field(fields[0])?;
                let g: f32 = field(fields[1])?;
                let b: f32 = field(fields[2])?;
                let a: f32 = field(fields[3])?;

                Ok(Self(r, g, b, a))
            } else {
                Err(())
            }
        } else if let Some(rest) = s.strip_prefix("hsl:") {
            let fields: Vec<_> = rest.split_ascii_whitespace().collect();
            if fields.len() == 3 {
                // Expected to be degrees in range 0-360, but we allow for negative and wrapping
                let h: i32 = fields[0].parse().map_err(|_| ())?;
                // Expected to be percentage in range 0-100
                let s: i32 = fields[1].parse().map_err(|_| ())?;
                // Expected to be percentage in range 0-100
                let l: i32 = fields[2].parse().map_err(|_| ())?;

                fn hsl_to_rgb(hue: i32, sat: i32, light: i32) -> (f32, f32, f32) {
                    let hue = hue % 360;
                    let hue = if hue < 0 { hue + 360 } else { hue } as f32;
                    let sat = sat as f32 / 100.;
                    let light = light as f32 / 100.;
                    let a = sat * light.min(1. - light);
                    let f = |n: f32| -> f32 {
                        let k = (n + hue / 30.) % 12.;
                        light - a * (k - 3.).min(9. - k).clamp(-1., 1.)
                    };
                    (f(0.), f(8.), f(4.))
                }

                let (r, g, b) = hsl_to_rgb(h, s, l);
                Ok(Self(r, g, b, 1.0))
            } else {
                Err(())
            }
        } else {
            #[cfg(feature = "std")]
            {
                if let Ok(c) = csscolorparser::parse(s) {
                    return Ok(Self(c.r as f32, c.g as f32, c.b as f32, c.a as f32));
                }
            }
            Self::from_named(s).ok_or(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_rgb() {
        let dark_green = SrgbaTuple::from_named("DarkGreen").unwrap();
        assert_eq!(dark_green.to_rgb_string(), "#006400");
    }

    #[test]
    fn from_hsl() {
        let foo = SrgbaTuple::from_str("hsl:235 100  50").unwrap();
        assert_eq!(foo.to_rgb_string(), "#0015ff");
    }

    #[test]
    fn from_rgba() {
        assert_eq!(
            SrgbaTuple::from_str("clear").unwrap().to_rgba_string(),
            "rgba(0% 0% 0% 0%)"
        );
        assert_eq!(
            SrgbaTuple::from_str("rgba:100% 0 0 50%")
                .unwrap()
                .to_rgba_string(),
            "rgba(100% 0% 0% 50%)"
        );
    }

    #[cfg(feature = "std")]
    #[test]
    fn from_css() {
        assert_eq!(
            SrgbaTuple::from_str("rgb(255,0,0)")
                .unwrap()
                .to_rgb_string(),
            "#ff0000"
        );

        let rgba = SrgbaTuple::from_str("rgba(255,0,0,1)").unwrap();
        let round_trip = SrgbaTuple::from_str(&rgba.to_rgba_string()).unwrap();
        assert_eq!(rgba, round_trip);
        assert_eq!(rgba.to_rgba_string(), "rgba(100% 0% 0% 100%)");
    }

    #[test]
    fn from_rgb() {
        assert!(SrgbaTuple::from_str("").is_err());
        assert!(SrgbaTuple::from_str("#xyxyxy").is_err());

        let foo = SrgbaTuple::from_str("#f00f00f00").unwrap();
        assert_eq!(foo.to_rgb_string(), "#f0f0f0");

        let black = SrgbaTuple::from_str("#000").unwrap();
        assert_eq!(black.to_rgb_string(), "#000000");

        let black = SrgbaTuple::from_str("#FFF").unwrap();
        assert_eq!(black.to_rgb_string(), "#f0f0f0");

        let black = SrgbaTuple::from_str("#000000").unwrap();
        assert_eq!(black.to_rgb_string(), "#000000");

        let grey = SrgbaTuple::from_str("rgb:D6/D6/D6").unwrap();
        assert_eq!(grey.to_rgb_string(), "#d6d6d6");

        let grey = SrgbaTuple::from_str("rgb:f0f0/f0f0/f0f0").unwrap();
        assert_eq!(grey.to_rgb_string(), "#f0f0f0");
    }

    #[cfg(feature = "std")]
    #[test]
    fn srgba_contrast_ratio() {
        let a = SrgbaTuple::from_str("hsl:0   100  50").unwrap();
        let b = SrgbaTuple::from_str("hsl:120 100  50").unwrap();
        let contrast_ratio = a.contrast_ratio(&b);
        assert!(
            (2.91 - contrast_ratio).abs() < 0.01,
            "contrast({}) == 2.91",
            contrast_ratio
        );
    }
}
