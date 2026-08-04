use super::*;
use num_derive::FromPrimitive;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sgr {
    /// Resets rendition to defaults.  Typically switches off
    /// all other Sgr options, but may have greater or lesser impact.
    Reset,
    /// Set the intensity/bold level
    Intensity(Intensity),
    Underline(Underline),
    UnderlineColor(ColorSpec),
    Blink(Blink),
    Italic(bool),
    Inverse(bool),
    Invisible(bool),
    StrikeThrough(bool),
    Font(Font),
    Foreground(ColorSpec),
    Background(ColorSpec),
    Overline(bool),
    VerticalAlign(VerticalAlign),
}

#[cfg(all(test, target_pointer_width = "64"))]
#[test]
fn sgr_size() {
    assert_eq!(core::mem::size_of::<Intensity>(), 1);
    assert_eq!(core::mem::size_of::<Underline>(), 1);
    assert_eq!(core::mem::size_of::<ColorSpec>(), 20);
    assert_eq!(core::mem::size_of::<Blink>(), 1);
    assert_eq!(core::mem::size_of::<Font>(), 2);
}

impl Display for Sgr {
    fn fmt(&self, f: &mut Formatter) -> Result<(), FmtError> {
        macro_rules! code {
            ($t:ident) => {
                write!(f, "{}m", SgrCode::$t as i64)?
            };
        }

        macro_rules! ansi_color {
            ($idx:expr, $eightbit:ident, $( ($Ansi:ident, $code:ident) ),*) => {
                if let Some(ansi) = FromPrimitive::from_u8($idx) {
                    match ansi {
                        $(AnsiColor::$Ansi => code!($code) ,)*
                    }
                } else {
                    write!(f, "{};5;{}m", SgrCode::$eightbit as i64, $idx)?
                }
            }
        }

        match self {
            Sgr::Reset => code!(Reset),
            Sgr::Intensity(Intensity::Bold) => code!(IntensityBold),
            Sgr::Intensity(Intensity::Half) => code!(IntensityDim),
            Sgr::Intensity(Intensity::Normal) => code!(NormalIntensity),
            Sgr::Underline(Underline::Single) => code!(UnderlineOn),
            Sgr::Underline(Underline::Double) => code!(UnderlineDouble),
            Sgr::Underline(Underline::Curly) => write!(f, "4:3m")?,
            Sgr::Underline(Underline::Dotted) => write!(f, "4:4m")?,
            Sgr::Underline(Underline::Dashed) => write!(f, "4:5m")?,
            Sgr::Underline(Underline::None) => code!(UnderlineOff),
            Sgr::Blink(Blink::Slow) => code!(BlinkOn),
            Sgr::Blink(Blink::Rapid) => code!(RapidBlinkOn),
            Sgr::Blink(Blink::None) => code!(BlinkOff),
            Sgr::Italic(true) => code!(ItalicOn),
            Sgr::Italic(false) => code!(ItalicOff),
            Sgr::Inverse(true) => code!(InverseOn),
            Sgr::Inverse(false) => code!(InverseOff),
            Sgr::Invisible(true) => code!(InvisibleOn),
            Sgr::Invisible(false) => code!(InvisibleOff),
            Sgr::StrikeThrough(true) => code!(StrikeThroughOn),
            Sgr::StrikeThrough(false) => code!(StrikeThroughOff),
            Sgr::Overline(true) => code!(OverlineOn),
            Sgr::Overline(false) => code!(OverlineOff),
            Sgr::VerticalAlign(VerticalAlign::BaseLine) => code!(VerticalAlignBaseLine),
            Sgr::VerticalAlign(VerticalAlign::SuperScript) => code!(VerticalAlignSuperScript),
            Sgr::VerticalAlign(VerticalAlign::SubScript) => code!(VerticalAlignSubScript),
            Sgr::Font(Font::Default) => code!(DefaultFont),
            Sgr::Font(Font::Alternate(1)) => code!(AltFont1),
            Sgr::Font(Font::Alternate(2)) => code!(AltFont2),
            Sgr::Font(Font::Alternate(3)) => code!(AltFont3),
            Sgr::Font(Font::Alternate(4)) => code!(AltFont4),
            Sgr::Font(Font::Alternate(5)) => code!(AltFont5),
            Sgr::Font(Font::Alternate(6)) => code!(AltFont6),
            Sgr::Font(Font::Alternate(7)) => code!(AltFont7),
            Sgr::Font(Font::Alternate(8)) => code!(AltFont8),
            Sgr::Font(Font::Alternate(9)) => code!(AltFont9),
            Sgr::Font(_) => { /* there are no other possible font values */ }
            Sgr::Foreground(ColorSpec::Default) => code!(ForegroundDefault),
            Sgr::Background(ColorSpec::Default) => code!(BackgroundDefault),
            Sgr::Foreground(ColorSpec::PaletteIndex(idx)) => ansi_color!(
                *idx,
                ForegroundColor,
                (Black, ForegroundBlack),
                (Maroon, ForegroundRed),
                (Green, ForegroundGreen),
                (Olive, ForegroundYellow),
                (Navy, ForegroundBlue),
                (Purple, ForegroundMagenta),
                (Teal, ForegroundCyan),
                (Silver, ForegroundWhite),
                // Note: these brights are emitted using codes in the 100 range.
                // I don't know how portable this is vs. the 256 color sequences,
                // so we may need to make an adjustment here later.
                (Grey, ForegroundBrightBlack),
                (Red, ForegroundBrightRed),
                (Lime, ForegroundBrightGreen),
                (Yellow, ForegroundBrightYellow),
                (Blue, ForegroundBrightBlue),
                (Fuchsia, ForegroundBrightMagenta),
                (Aqua, ForegroundBrightCyan),
                (White, ForegroundBrightWhite)
            ),
            Sgr::Foreground(ColorSpec::TrueColor(c)) => {
                let (red, green, blue, alpha) = c.to_srgb_u8();
                if alpha == 255 {
                    write!(
                        f,
                        "{}:2::{}:{}:{}m",
                        SgrCode::ForegroundColor as i64,
                        red,
                        green,
                        blue
                    )?
                } else {
                    write!(
                        f,
                        "{}:6::{}:{}:{}:{}m",
                        SgrCode::ForegroundColor as i64,
                        red,
                        green,
                        blue,
                        alpha
                    )?
                }
            }
            Sgr::Background(ColorSpec::PaletteIndex(idx)) => ansi_color!(
                *idx,
                BackgroundColor,
                (Black, BackgroundBlack),
                (Maroon, BackgroundRed),
                (Green, BackgroundGreen),
                (Olive, BackgroundYellow),
                (Navy, BackgroundBlue),
                (Purple, BackgroundMagenta),
                (Teal, BackgroundCyan),
                (Silver, BackgroundWhite),
                // Note: these brights are emitted using codes in the 100 range.
                // I don't know how portable this is vs. the 256 color sequences,
                // so we may need to make an adjustment here later.
                (Grey, BackgroundBrightBlack),
                (Red, BackgroundBrightRed),
                (Lime, BackgroundBrightGreen),
                (Yellow, BackgroundBrightYellow),
                (Blue, BackgroundBrightBlue),
                (Fuchsia, BackgroundBrightMagenta),
                (Aqua, BackgroundBrightCyan),
                (White, BackgroundBrightWhite)
            ),
            Sgr::Background(ColorSpec::TrueColor(c)) => {
                let (red, green, blue, alpha) = c.to_srgb_u8();
                if alpha == 255 {
                    write!(
                        f,
                        "{}:2::{}:{}:{}m",
                        SgrCode::BackgroundColor as i64,
                        red,
                        green,
                        blue
                    )?
                } else {
                    write!(
                        f,
                        "{}:6::{}:{}:{}:{}m",
                        SgrCode::BackgroundColor as i64,
                        red,
                        green,
                        blue,
                        alpha
                    )?
                }
            }
            Sgr::UnderlineColor(ColorSpec::Default) => code!(ResetUnderlineColor),
            Sgr::UnderlineColor(ColorSpec::TrueColor(c)) => {
                let (red, green, blue, alpha) = c.to_srgb_u8();
                if alpha == 255 {
                    write!(
                        f,
                        "{}:2::{}:{}:{}m",
                        SgrCode::UnderlineColor as i64,
                        red,
                        green,
                        blue
                    )?
                } else {
                    write!(
                        f,
                        "{}:6::{}:{}:{}:{}m",
                        SgrCode::UnderlineColor as i64,
                        red,
                        green,
                        blue,
                        alpha
                    )?
                }
            }
            Sgr::UnderlineColor(ColorSpec::PaletteIndex(idx)) => {
                write!(f, "{};5;{}m", SgrCode::UnderlineColor as i64, *idx)?
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Font {
    Default,
    Alternate(u8),
}

#[derive(Debug, Clone, PartialEq, Eq, FromPrimitive)]
pub enum SgrCode {
    Reset = 0,
    IntensityBold = 1,
    IntensityDim = 2,
    ItalicOn = 3,
    UnderlineOn = 4,
    /// Blinks < 150 times per minute
    BlinkOn = 5,
    /// Blinks > 150 times per minute
    RapidBlinkOn = 6,
    InverseOn = 7,
    InvisibleOn = 8,
    StrikeThroughOn = 9,
    DefaultFont = 10,
    AltFont1 = 11,
    AltFont2 = 12,
    AltFont3 = 13,
    AltFont4 = 14,
    AltFont5 = 15,
    AltFont6 = 16,
    AltFont7 = 17,
    AltFont8 = 18,
    AltFont9 = 19,
    // Fraktur = 20,
    UnderlineDouble = 21,
    NormalIntensity = 22,
    ItalicOff = 23,
    UnderlineOff = 24,
    BlinkOff = 25,
    InverseOff = 27,
    InvisibleOff = 28,
    StrikeThroughOff = 29,
    ForegroundBlack = 30,
    ForegroundRed = 31,
    ForegroundGreen = 32,
    ForegroundYellow = 33,
    ForegroundBlue = 34,
    ForegroundMagenta = 35,
    ForegroundCyan = 36,
    ForegroundWhite = 37,
    ForegroundDefault = 39,
    BackgroundBlack = 40,
    BackgroundRed = 41,
    BackgroundGreen = 42,
    BackgroundYellow = 43,
    BackgroundBlue = 44,
    BackgroundMagenta = 45,
    BackgroundCyan = 46,
    BackgroundWhite = 47,
    BackgroundDefault = 49,
    OverlineOn = 53,
    OverlineOff = 55,

    UnderlineColor = 58,
    ResetUnderlineColor = 59,

    VerticalAlignSuperScript = 73,
    VerticalAlignSubScript = 74,
    VerticalAlignBaseLine = 75,

    ForegroundBrightBlack = 90,
    ForegroundBrightRed = 91,
    ForegroundBrightGreen = 92,
    ForegroundBrightYellow = 93,
    ForegroundBrightBlue = 94,
    ForegroundBrightMagenta = 95,
    ForegroundBrightCyan = 96,
    ForegroundBrightWhite = 97,

    BackgroundBrightBlack = 100,
    BackgroundBrightRed = 101,
    BackgroundBrightGreen = 102,
    BackgroundBrightYellow = 103,
    BackgroundBrightBlue = 104,
    BackgroundBrightMagenta = 105,
    BackgroundBrightCyan = 106,
    BackgroundBrightWhite = 107,

    /// Maybe followed either either a 256 color palette index or
    /// a sequence describing a true color rgb value
    ForegroundColor = 38,
    BackgroundColor = 48,
}
