use crate::color::RgbaColor;
use crate::font::{FontStretch, FontStyle, FontWeight, FreeTypeLoadFlags, FreeTypeLoadTarget};
use onlyterm_dynamic::{FromDynamic, ToDynamic};
use ordered_float::NotNan;

#[derive(Debug, Clone, PartialEq, Eq, Hash, FromDynamic, ToDynamic)]
pub struct FontAttributes {
    /// The font family name
    pub family: String,
    /// Whether the font should be a bold variant
    #[dynamic(default)]
    pub weight: FontWeight,
    #[dynamic(default)]
    pub stretch: FontStretch,
    /// Whether the font should be an italic variant
    #[dynamic(default)]
    pub style: FontStyle,
    /// Bookkeeping for fonts that the font resolver appended itself rather
    /// than the user asking for them. Never meaningful to write in a config
    /// file -- but ktav has no constructor functions, so a user's `font` is
    /// now spelled out as a literal `FontAttributes` object and every field
    /// without a default becomes mandatory for them to supply. Defaulting
    /// these two keeps the documented `font: { font: [{ family: X }] }`
    /// spelling loadable.
    #[dynamic(default)]
    pub is_fallback: bool,
    #[dynamic(default)]
    pub is_synthetic: bool,

    #[dynamic(default)]
    pub harfbuzz_features: Option<Vec<String>>,
    /// NOTE: this option currently has no effect. Its only reader was
    /// `ftwrap.rs::compute_load_flags_from_config`, which took this
    /// per-font override ahead of the top-level `Config::freetype_load_target`
    /// when building FreeType's glyph-load flags. That file (and the
    /// vendored FreeType backend it wrapped) was removed in the
    /// freetype+harfbuzz -> rustybuzz+swash migration (phase H4); the
    /// Swash-based rasterizer that replaced it has no equivalent per-font
    /// knob. Note that the like-named top-level `Config::freetype_load_target`
    /// is unrelated and still live -- it drives subpixel vs alpha blending
    /// in the GL renderer regardless of which rasterizer produced the glyph.
    #[dynamic(
        default,
        deprecated = "this option no longer does anything: its only reader was the FreeType \
                      glyph-loading code, and the FreeType rasterizer backend it applied to was \
                      removed in the rustybuzz/swash migration"
    )]
    pub freetype_load_target: Option<FreeTypeLoadTarget>,
    /// NOTE: this option currently has no effect, for the same reason as
    /// `freetype_load_target` above: its only reader was
    /// `ftwrap.rs::compute_load_flags_from_config`, removed along with the
    /// rest of the vendored FreeType backend in the rustybuzz/swash
    /// migration (phase H4).
    #[dynamic(
        default,
        deprecated = "this option no longer does anything: its only reader was the FreeType \
                      glyph-loading code, and the FreeType rasterizer backend it applied to was \
                      removed in the rustybuzz/swash migration"
    )]
    pub freetype_render_target: Option<FreeTypeLoadTarget>,
    /// NOTE: this option currently has no effect, for the same reason as
    /// `freetype_load_target` above: its only reader was
    /// `ftwrap.rs::compute_load_flags_from_config`, removed along with the
    /// rest of the vendored FreeType backend in the rustybuzz/swash
    /// migration (phase H4).
    #[dynamic(
        default,
        deprecated = "this option no longer does anything: its only reader was the FreeType \
                      glyph-loading code, and the FreeType rasterizer backend it applied to was \
                      removed in the rustybuzz/swash migration"
    )]
    pub freetype_load_flags: Option<FreeTypeLoadFlags>,
    #[dynamic(default)]
    pub scale: Option<NotNan<f64>>,
    #[dynamic(default)]
    pub assume_emoji_presentation: Option<bool>,
}

impl std::fmt::Display for FontAttributes {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {
        // A ktav `FontAttributes` object literal, ready to paste into a
        // `font.font` array: `{ family: X, weight: Y, stretch: Z, style: W }`.
        write!(
            fmt,
            "{{ family: {}, weight: {}, stretch: {}, style: {} }}",
            self.family, self.weight, self.stretch, self.style
        )
    }
}

impl FontAttributes {
    pub fn new(family: &str) -> Self {
        Self {
            family: family.into(),
            weight: FontWeight::default(),
            stretch: FontStretch::default(),
            style: FontStyle::Normal,
            is_fallback: false,
            is_synthetic: false,
            harfbuzz_features: None,
            freetype_load_target: None,
            freetype_render_target: None,
            freetype_load_flags: None,
            scale: None,
            assume_emoji_presentation: None,
        }
    }

    pub fn new_fallback(family: &str) -> Self {
        Self {
            family: family.into(),
            weight: FontWeight::default(),
            stretch: FontStretch::default(),
            style: FontStyle::Normal,
            is_fallback: true,
            is_synthetic: false,
            harfbuzz_features: None,
            freetype_load_target: None,
            freetype_render_target: None,
            freetype_load_flags: None,
            scale: None,
            assume_emoji_presentation: None,
        }
    }
}

impl Default for FontAttributes {
    fn default() -> Self {
        Self {
            family: "JetBrains Mono".into(),
            weight: FontWeight::default(),
            stretch: FontStretch::default(),
            style: FontStyle::Normal,
            is_fallback: false,
            is_synthetic: false,
            harfbuzz_features: None,
            freetype_load_target: None,
            freetype_render_target: None,
            freetype_load_flags: None,
            scale: None,
            assume_emoji_presentation: None,
        }
    }
}

/// Represents textual styling.
#[derive(Debug, Clone, PartialEq, Eq, Hash, FromDynamic, ToDynamic)]
pub struct TextStyle {
    #[dynamic(default)]
    pub font: Vec<FontAttributes>,

    /// If set, when rendering text that is set to the default
    /// foreground color, use this color instead.  This is most
    /// useful in a `[[font_rules]]` section to implement changing
    /// the text color for eg: bold text.
    pub foreground: Option<RgbaColor>,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            foreground: None,
            font: vec![FontAttributes::default()],
        }
    }
}

impl TextStyle {
    /// Make a version of this style where the first entry
    /// has any explicitly named bold/italic components
    /// removed.  The intent is to set it up for make_bold
    /// and make_italic below.
    ///
    /// This is done heuristically based on the family name
    /// string as we cannot depend on the font parser from
    /// this crate, and even if we did have a parser, that
    /// doesn't help us know anything about the name until
    /// we have a parsed font to compare with.
    ///
    /// <https://github.com/wezterm/wezterm/issues/456>
    pub fn reduce_first_font_to_family(&self) -> Self {
        fn reduce(mut family: &str) -> String {
            loop {
                let start = family;

                for s in &[
                    "Black",
                    "Bold",
                    "Book",
                    "Condensed",
                    "Demi",
                    "Expanded",
                    "Extra",
                    "Italic",
                    "Light",
                    "Medium",
                    "Regular",
                    "Semi",
                    "Thin",
                    "Ultra",
                ] {
                    family = family.trim().trim_end_matches(s);
                }

                if family == start {
                    break;
                }
            }

            family.trim().to_string()
        }
        Self {
            foreground: self.foreground,
            font: self
                .font
                .iter()
                .enumerate()
                .map(|(idx, orig_attr)| {
                    let mut attr = orig_attr.clone();
                    if idx == 0 {
                        attr.family = reduce(&attr.family);
                    }
                    attr
                })
                .collect(),
        }
    }

    /// Make a version of this style with bold enabled.
    pub fn make_bold(&self) -> Self {
        Self {
            foreground: self.foreground,
            font: self
                .font
                .iter()
                .map(|attr| {
                    let mut attr = attr.clone();
                    attr.weight = attr.weight.bolder();
                    attr.is_synthetic = true;
                    attr
                })
                .collect(),
        }
    }

    pub fn make_half_bright(&self) -> Self {
        Self {
            foreground: self.foreground,
            font: self
                .font
                .iter()
                .map(|attr| {
                    let mut attr = attr.clone();
                    attr.weight = attr.weight.lighter();
                    attr.is_synthetic = true;
                    attr
                })
                .collect(),
        }
    }

    /// Make a version of this style with italic enabled.
    pub fn make_italic(&self) -> Self {
        Self {
            foreground: self.foreground,
            font: self
                .font
                .iter()
                .map(|attr| {
                    let mut attr = attr.clone();
                    attr.style = FontStyle::Italic;
                    attr.is_synthetic = true;
                    attr
                })
                .collect(),
        }
    }

    #[allow(clippy::let_and_return)]
    pub fn font_with_fallback(&self) -> Vec<FontAttributes> {
        let mut font = self.font.clone();

        let mut default_font = FontAttributes::default();

        // Insert our bundled default JetBrainsMono as a fallback
        // in case their preference doesn't match anything.
        // But don't add it if it is already their preference.
        if !font.contains(&default_font) {
            default_font.is_fallback = true;
            font.push(default_font);
        }

        // We bundle this emoji font as an in-memory fallback
        font.push(FontAttributes::new_fallback("Noto Color Emoji"));

        // We bundle this Hebrew font (genuinely monospaced consonants,
        // OFL-licensed, no proportional-width fallback chain) as an
        // in-memory fallback so Hebrew text renders correctly even
        // without a Hebrew font installed on the system. It doesn't cover
        // cantillation marks or some niqqud; those render as .notdef.
        font.push(FontAttributes::new_fallback("Cascadia Mono"));

        // Add symbols that many people end up using via patched fonts
        font.push(FontAttributes::new_fallback("Symbols Nerd Font Mono"));

        font
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_reduce() {
        for family in &[
            "Inconsolata SemiCondensed ExtraBold",
            "Inconsolata SemiCondensed Regular",
            "Inconsolata SemiCondensed Medium",
            "Inconsolata SemiCondensed SemiBold",
        ] {
            let style = TextStyle {
                font: vec![FontAttributes::new(family)],
                foreground: None,
            };
            let style = style.reduce_first_font_to_family();
            assert_eq!(style.font[0].family, "Inconsolata");
        }
    }
}

#[cfg(test)]
mod font_attribute_tests {
    use super::*;

    /// ktav has no constructor functions, so the only way to configure a
    /// font is to spell out a `FontAttributes` object literal -- which
    /// means every field without a `#[dynamic(default)]` becomes something
    /// the user is forced to write. `is_fallback`/`is_synthetic` are
    /// internal bookkeeping that no user should ever have to think about,
    /// and requiring them made the spelling shown throughout the docs
    /// (`font: { font: [{ family: X }] }`) fail to load, taking the whole
    /// config down with it.
    #[test]
    fn family_alone_is_enough_to_configure_a_font() {
        let parsed = ktav::parse("font: [{ family: Lucida Console }]").unwrap();
        let dyn_value = crate::ktav_value::ktav_value_to_dynamic(&parsed);
        let obj = match dyn_value {
            onlyterm_dynamic::Value::Object(obj) => obj,
            // Explicit argument rather than an inline `{other:?}` capture:
            // this crate is edition 2018, where implicit format captures
            // don't exist, so the placeholder would be printed literally
            // and the offending value never shown.
            other => panic!("expected an object, got {:?}", other),
        };
        let font = obj
            .get(&onlyterm_dynamic::Value::String("font".to_string()))
            .expect("font key")
            .clone();

        let attrs: Vec<FontAttributes> =
            onlyterm_dynamic::FromDynamic::from_dynamic(&font, Default::default())
                .expect("`{ family: ... }` alone must be a loadable font entry");

        assert_eq!(attrs.len(), 1);
        assert_eq!(attrs[0].family, "Lucida Console");
        assert!(!attrs[0].is_fallback);
        assert!(!attrs[0].is_synthetic);
    }
}
