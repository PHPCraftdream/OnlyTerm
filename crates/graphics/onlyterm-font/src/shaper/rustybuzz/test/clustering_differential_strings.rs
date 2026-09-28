//! Broadens the string/script coverage feeding the differential
//! clustering check wired into every `do_shape` call in test builds (see
//! `clustering_differential.rs`): CJK, Arabic RTL, emoji ZWJ sequences,
//! and Latin combining marks, on top of the ASCII/ligature/Hebrew
//! coverage `baseline.rs`/`bidi.rs`/`shaping.rs` already exercise. None
//! of the bundled fonts have real CJK/Arabic glyph coverage, so several
//! of these deliberately drive the fallback-exhausted ("no more
//! fallbacks", merged-notdef-run) branch of the clustering logic, which
//! is exactly the riskiest part of the flat-Vec rewrite to get wrong.
use super::*;

fn noto_emoji_handle() -> ParsedFont {
    let db = FontDatabase::with_built_in().unwrap();
    db.resolve(
        &FontAttributes {
            family: "Noto Color Emoji".into(),
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

/// Shapes `text` and does a light sanity check on the result. The real
/// point of every test in this file is the `assert_clustering_matches_reference`
/// call wired into `do_shape` itself (see `clustering_differential.rs`):
/// it runs on every one of these calls and panics on any mismatch, so a
/// passing test here already proves the flat/nested clustering algorithms
/// agree for this input.
fn shape_ok(shaper: &RustybuzzShaper, text: &str, direction: Direction) {
    let mut no_glyphs = vec![];
    let info = shaper
        .shape(text, 14., 72, &mut no_glyphs, None, direction, None, None)
        .unwrap_or_else(|e| panic!("shape failed for {:?}: {:?}", text, e));
    assert!(
        !info.is_empty(),
        "shape produced no glyphs at all for non-empty text {:?}",
        text
    );
}

#[test]
fn cjk_text_through_fallback_exhaustion() {
    let _ = env_logger::Builder::new()
        .is_test(true)
        .filter_level(log::LevelFilter::Debug)
        .try_init();
    let config = onlyterm_config::configuration();
    // JetBrains Mono has no CJK coverage at all, so this exercises the
    // "ran out of fallback options" / merged-notdef-run path.
    let shaper = RustybuzzShaper::new(&config, &[jetbrains_handle()]).unwrap();
    for text in [
        "漢字テスト",
        "你好，世界",
        "안녕하세요",
        "日本語とEnglishの混在",
    ] {
        shape_ok(&shaper, text, Direction::LeftToRight);
    }
}

#[test]
fn arabic_rtl_text_through_fallback_exhaustion() {
    let _ = env_logger::Builder::new()
        .is_test(true)
        .filter_level(log::LevelFilter::Debug)
        .try_init();
    let config = onlyterm_config::configuration();
    let shaper = RustybuzzShaper::new(&config, &[jetbrains_handle()]).unwrap();
    for text in ["مرحبا بالعالم", "السلام عليكم", "١٢٣ عربي"] {
        shape_ok(&shaper, text, Direction::RightToLeft);
    }
}

#[test]
fn emoji_zwj_sequences() {
    let _ = env_logger::Builder::new()
        .is_test(true)
        .filter_level(log::LevelFilter::Debug)
        .try_init();
    let config = onlyterm_config::configuration();
    let shaper = RustybuzzShaper::new(&config, &[jetbrains_handle(), noto_emoji_handle()]).unwrap();
    for text in [
        "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}\u{200D}\u{1F466}", // family ZWJ sequence
        "\u{1F469}\u{200D}\u{1F4BB}",                                   // woman technologist
        "\u{1F3F3}\u{FE0F}\u{200D}\u{1F308}",                           // rainbow flag
        "\u{1F44D}\u{1F3FD}", // thumbs up + medium skin tone modifier
    ] {
        shape_ok(&shaper, text, Direction::LeftToRight);
    }
}

#[test]
fn latin_combining_marks_stack() {
    let _ = env_logger::Builder::new()
        .is_test(true)
        .filter_level(log::LevelFilter::Debug)
        .try_init();
    let config = onlyterm_config::configuration();
    let shaper = RustybuzzShaper::new(&config, &[jetbrains_handle()]).unwrap();
    for text in [
        "e\u{0301}",                 // e + combining acute accent
        "a\u{0300}\u{0301}\u{0302}", // stacked combining marks on one base
        "n\u{0303}",                 // n + combining tilde
        "cafe\u{0301}",              // combining mark at the end of a word
    ] {
        shape_ok(&shaper, text, Direction::LeftToRight);
    }
}
