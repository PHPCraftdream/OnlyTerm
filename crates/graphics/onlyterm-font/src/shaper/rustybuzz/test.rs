use super::*;
mod baseline;
mod bidi;
mod fallback_cache;
mod shaping;
use crate::locator::{FontDataHandle, FontDataSource};
use crate::FontDatabase;
use onlyterm_config::FontAttributes;

fn hebrew_fallback_handle() -> ParsedFont {
    let db = FontDatabase::with_built_in().unwrap();
    db.resolve(
        &FontAttributes {
            family: "Cascadia Mono".into(),
            stretch: Default::default(),
            weight: Default::default(),
            is_fallback: false,
            is_synthetic: false,
            style: Default::default(),
            freetype_load_flags: None,
            freetype_load_target: None,
            freetype_render_target: None,
            harfbuzz_features: None,
            scale: None,
            assume_emoji_presentation: None,
        },
        14,
    )
    .unwrap()
    .clone()
}

/// Mirrors the real default font stack's shape: a primary font with no
/// Hebrew coverage (Lucida Console isn't available in this Linux/CI
/// build environment, so JetBrains Mono stands in for "primary font
/// without Hebrew glyphs") followed by the bundled Hebrew fallback, so
/// Hebrew codepoints only resolve after at least one no-glyphs/
/// "incomplete" pass through a font that can't shape them.
fn primary_then_hebrew_fallback_handles() -> Vec<ParsedFont> {
    vec![jetbrains_handle(), hebrew_fallback_handle()]
}

/// Same idea, but using the actual default primary font
/// (`default_font_style` on Windows), which -- unlike JetBrains Mono --
/// may have partial native Hebrew coverage (eg: base consonants but not
/// niqqud combining marks), producing a different pattern of
/// direct-vs-"incomplete" glyphs within the same rustybuzz cluster than
/// a font with zero Hebrew coverage at all.
#[cfg(windows)]
fn lucida_then_hebrew_fallback_handles() -> Vec<ParsedFont> {
    let lucida = ParsedFont::from_locator(&FontDataHandle {
        source: FontDataSource::OnDisk(std::path::PathBuf::from("C:\\Windows\\Fonts\\lucon.ttf")),
        index: 0,
        variation: 0,
        origin: crate::locator::FontOrigin::FontDirs,
        coverage: None,
    })
    .expect("C:\\Windows\\Fonts\\lucon.ttf (Lucida Console) must be present on Windows CI");

    fn built_in(family: &str) -> ParsedFont {
        let db = FontDatabase::with_built_in().unwrap();
        db.resolve(
            &FontAttributes {
                family: family.into(),
                stretch: Default::default(),
                weight: Default::default(),
                is_fallback: true,
                is_synthetic: false,
                style: Default::default(),
                freetype_load_flags: None,
                freetype_load_target: None,
                freetype_render_target: None,
                harfbuzz_features: None,
                scale: None,
                assume_emoji_presentation: None,
            },
            14,
        )
        .unwrap()
        .clone()
    }

    // Exact real default order: primary, JetBrains fallback, Noto Color
    // Emoji, Cascadia Mono (Hebrew), Symbols Nerd Font Mono (see
    // `TextStyle::font_with_fallback`).
    vec![
        lucida,
        jetbrains_handle(),
        built_in("Noto Color Emoji"),
        hebrew_fallback_handle(),
        built_in("Symbols Nerd Font Mono"),
    ]
}

fn jetbrains_handle() -> ParsedFont {
    let db = FontDatabase::with_built_in().unwrap();
    db.resolve(
        &FontAttributes {
            family: "JetBrains Mono".into(),
            stretch: Default::default(),
            weight: Default::default(),
            is_fallback: false,
            is_synthetic: false,
            style: Default::default(),
            freetype_load_flags: None,
            freetype_load_target: None,
            freetype_render_target: None,
            harfbuzz_features: None,
            scale: None,
            assume_emoji_presentation: None,
        },
        14,
    )
    .unwrap()
    .clone()
}
