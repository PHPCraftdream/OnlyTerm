use bitflags::*;
use enum_display_derive::Display;
use std::convert::TryFrom;
use std::fmt::Display;
use wezterm_dynamic::{FromDynamic, ToDynamic};

pub use crate::font_weight::*;
pub use crate::text_style::*;

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Display,
    PartialOrd,
    Ord,
    FromDynamic,
    ToDynamic,
    Default,
)]
pub enum FontStyle {
    #[default]
    Normal,
    Italic,
    Oblique,
}

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Display,
    PartialOrd,
    Ord,
    FromDynamic,
    ToDynamic,
    Default,
)]
pub enum FontStretch {
    UltraCondensed,
    ExtraCondensed,
    Condensed,
    SemiCondensed,
    #[default]
    Normal,
    SemiExpanded,
    Expanded,
    ExtraExpanded,
    UltraExpanded,
}

impl FontStretch {
    pub fn from_opentype_stretch(w: u16) -> Self {
        match w {
            1 => Self::UltraCondensed,
            2 => Self::ExtraCondensed,
            3 => Self::Condensed,
            4 => Self::SemiCondensed,
            5 => Self::Normal,
            6 => Self::SemiExpanded,
            7 => Self::Expanded,
            8 => Self::ExtraExpanded,
            9 => Self::UltraExpanded,
            _ if w < 1 => Self::UltraCondensed,
            _ => Self::UltraExpanded,
        }
    }

    pub fn to_opentype_stretch(self) -> u16 {
        match self {
            Self::UltraCondensed => 1,
            Self::ExtraCondensed => 2,
            Self::Condensed => 3,
            Self::SemiCondensed => 4,
            Self::Normal => 5,
            Self::SemiExpanded => 6,
            Self::Expanded => 7,
            Self::ExtraExpanded => 8,
            Self::UltraExpanded => 9,
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, FromDynamic, ToDynamic)]
pub enum DisplayPixelGeometry {
    #[default]
    RGB,
    BGR,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, FromDynamic, ToDynamic)]
pub enum FreeTypeLoadTarget {
    /// This corresponds to the default hinting algorithm, optimized
    /// for standard gray-level rendering.
    #[default]
    Normal,
    /// A lighter hinting algorithm for non-monochrome modes. Many
    /// generated glyphs are more fuzzy but better resemble its
    /// original shape. A bit like rendering on Mac OS X.  This target
    /// implies FT_LOAD_FORCE_AUTOHINT.
    Light,
    /// Strong hinting algorithm that should only be used for
    /// monochrome output. The result is probably unpleasant if the
    /// glyph is rendered in non-monochrome modes.
    Mono,
    /// A variant of Normal optimized for horizontally decimated LCD displays.
    HorizontalLcd,
    /// A variant of Normal optimized for vertically decimated LCD displays.
    VerticalLcd,
}

bitflags! {
    // Note that these are strongly coupled with deps/freetype/src/lib.rs,
    // but we can't directly reference that from here without making config
    // depend on freetype.
    #[derive(FromDynamic, ToDynamic)]
    #[dynamic(try_from="String", into="String")]
    pub struct FreeTypeLoadFlags: u32 {
        /// FT_LOAD_DEFAULT
        const DEFAULT = 0;
        /// Disable hinting. This generally generates ‘blurrier’
        /// bitmap glyph when the glyph is rendered in any of the
        /// anti-aliased modes.
        const NO_HINTING = 2;
        const NO_BITMAP = 8;
        /// Indicates that the auto-hinter is preferred over the
        /// font’s native hinter.
        const FORCE_AUTOHINT = 32;
        const MONOCHROME = 4096;
        /// Disable auto-hinter.
        const NO_AUTOHINT = 32768;
        const NO_SVG = 16777216;
        const SVG_ONLY = 8388608;
    }
}

impl FreeTypeLoadFlags {
    pub fn default_hidpi() -> Self {
        Self::NO_HINTING
    }
}

impl Default for FreeTypeLoadFlags {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl From<FreeTypeLoadFlags> for String {
    fn from(val: FreeTypeLoadFlags) -> Self {
        val.to_string()
    }
}

impl From<&FreeTypeLoadFlags> for String {
    fn from(val: &FreeTypeLoadFlags) -> Self {
        val.to_string()
    }
}

impl std::fmt::Display for FreeTypeLoadFlags {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let mut s = vec![];
        if *self == Self::DEFAULT {
            s.push("DEFAULT");
        }
        if self.contains(Self::NO_HINTING) {
            s.push("NO_HINTING");
        }
        if self.contains(Self::NO_BITMAP) {
            s.push("NO_BITMAP");
        }
        if self.contains(Self::NO_SVG) {
            s.push("NO_SVG");
        }
        if self.contains(Self::SVG_ONLY) {
            s.push("SVG_ONLY");
        }
        if self.contains(Self::FORCE_AUTOHINT) {
            s.push("FORCE_AUTOHINT");
        }
        if self.contains(Self::MONOCHROME) {
            s.push("MONOCHROME");
        }
        if self.contains(Self::NO_AUTOHINT) {
            s.push("NO_AUTOHINT");
        }
        write!(f, "{}", s.join("|"))
    }
}

impl TryFrom<String> for FreeTypeLoadFlags {
    type Error = String;
    fn try_from(s: String) -> Result<Self, String> {
        let mut flags = FreeTypeLoadFlags::empty();

        for ele in s.split('|') {
            let ele = ele.trim();
            match ele {
                "DEFAULT" => flags |= Self::DEFAULT,
                "NO_HINTING" => flags |= Self::NO_HINTING,
                "NO_BITMAP" => flags |= Self::NO_BITMAP,
                "NO_SVG" => flags |= Self::NO_SVG,
                "SVG_ONLY" => flags |= Self::SVG_ONLY,
                "FORCE_AUTOHINT" => flags |= Self::FORCE_AUTOHINT,
                "MONOCHROME" => flags |= Self::MONOCHROME,
                "NO_AUTOHINT" => flags |= Self::NO_AUTOHINT,
                _ => {
                    return Err(format!("invalid FreeTypeLoadFlags `{}` in `{}`", ele, s));
                }
            }
        }

        Ok(flags)
    }
}

/// Defines a rule that can be used to select a `TextStyle` given
/// an input `CellAttributes` value.  The logic that applies the
/// matching can be found in src/font/mod.rs.  The concept is that
/// the user can specify something like this:
///
/// ```toml
/// [[font_rules]]
/// italic = true
/// font = { font = [{family = "Operator Mono SSm Lig", italic=true}]}
/// ```
///
/// The above is translated as: "if the `CellAttributes` have the italic bit
/// set, then use the italic style of font rather than the default", and
/// stop processing further font rules.
#[derive(Debug, Default, Clone, FromDynamic, ToDynamic)]
pub struct StyleRule {
    /// If present, this rule matches when CellAttributes::intensity holds
    /// a value that matches this rule.  Valid values are "Bold", "Normal",
    /// "Half".
    pub intensity: Option<wezterm_term::Intensity>,
    /// If present, this rule matches when CellAttributes::underline holds
    /// a value that matches this rule.  Valid values are "None", "Single",
    /// "Double".
    pub underline: Option<wezterm_term::Underline>,
    /// If present, this rule matches when CellAttributes::italic holds
    /// a value that matches this rule.
    pub italic: Option<bool>,
    /// If present, this rule matches when CellAttributes::blink holds
    /// a value that matches this rule.
    pub blink: Option<wezterm_term::Blink>,
    /// If present, this rule matches when CellAttributes::reverse holds
    /// a value that matches this rule.
    pub reverse: Option<bool>,
    /// If present, this rule matches when CellAttributes::strikethrough holds
    /// a value that matches this rule.
    pub strikethrough: Option<bool>,
    /// If present, this rule matches when CellAttributes::invisible holds
    /// a value that matches this rule.
    pub invisible: Option<bool>,

    /// When this rule matches, `font` specifies the styling to be used.
    pub font: TextStyle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, FromDynamic, ToDynamic, Default)]
pub enum AllowSquareGlyphOverflow {
    Never,
    Always,
    #[default]
    WhenFollowedBySpace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, FromDynamic, ToDynamic, Default)]
pub enum FontLocatorSelection {
    /// Use fontconfig APIs to resolve fonts (!macos, posix systems)
    FontConfig,
    /// Use GDI on win32 systems
    #[default]
    Gdi,
    /// Use CoreText on macOS
    CoreText,
    /// Use only the font_dirs configuration to locate fonts
    ConfigDirsOnly,
}

#[derive(Debug, Clone, Copy, FromDynamic, ToDynamic, Default)]
pub enum FontRasterizerSelection {
    /// No longer implemented: the vendored FreeType C library and the
    /// `FreeTypeRasterizer` built on it were removed from the workspace
    /// in phase H4 of the freetype+harfbuzz -> rustybuzz+swash migration.
    /// This variant is kept only so that existing config files selecting
    /// it fail with an explanatory error at startup, rather than a
    /// config-parse error; select it and `new_rasterizer` will report why.
    FreeType,
    /// No longer implemented: the vendored HarfBuzz C++ library and the
    /// paint-API-based `HarfbuzzRasterizer` built on it were removed from
    /// the workspace in phase H4; `Swash`'s COLR/COLRv1 fallback is now
    /// `colr_paint::ColrRasterizer`, a pure-Rust replacement built on
    /// `ttf_parser::colr`. Kept for the same reason as `FreeType` above.
    Harfbuzz,
    /// Pure-Rust rasterizer built on the `swash` crate. Handles ordinary
    /// (non-COLR) glyph outlines itself and internally delegates
    /// COLR/COLRv1/CBDT/sbix color glyphs to
    /// `colr_paint::ColrRasterizer` (a pure-Rust COLR/COLRv1 paint-graph
    /// rasterizer built on `ttf_parser::colr`) -- see
    /// `wezterm-font/src/rasterizer/swash.rs` module docs. Default as of
    /// phase H3.5 of the freetype+harfbuzz -> rustybuzz+swash migration.
    #[default]
    Swash,
}

#[derive(Debug, Clone, Copy, FromDynamic, ToDynamic, Default)]
pub enum FontShaperSelection {
    /// No longer implemented; kept only so that existing config files
    /// selecting it fail with an explanatory error rather than a
    /// config-parse error.
    Allsorts,
    /// No longer implemented: the vendored HarfBuzz C++ library and the
    /// `HarfbuzzShaper` built on it were removed from the workspace in
    /// phase H4 of the freetype+harfbuzz -> rustybuzz+swash migration.
    /// Kept for the same reason as `Allsorts` above.
    Harfbuzz,
    /// Pure-Rust shaper built on the `rustybuzz` crate (a Rust port of the
    /// HarfBuzz shaping algorithm). Default as of phase H3.5 of the
    /// freetype+harfbuzz -> rustybuzz+swash migration.
    #[default]
    RustyBuzz,
}
