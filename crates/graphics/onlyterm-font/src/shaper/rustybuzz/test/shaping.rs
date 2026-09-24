use super::*;

#[test]
fn shape_basic() {
    let _ = env_logger::Builder::new()
        .is_test(true)
        .filter_level(log::LevelFilter::Trace)
        .try_init();

    let config = onlyterm_config::configuration();
    let shaper = RustybuzzShaper::new(&config, &[jetbrains_handle()]).unwrap();
    let mut no_glyphs = vec![];
    let info = shaper
        .shape(
            "abc",
            10.,
            72,
            &mut no_glyphs,
            None,
            Direction::LeftToRight,
            None,
            None,
        )
        .unwrap();
    assert!(no_glyphs.is_empty(), "{:?}", no_glyphs);
    assert_eq!(info.len(), 3);
    assert_eq!(info[0].only_char, Some('a'));
    assert_eq!(info[1].only_char, Some('b'));
    assert_eq!(info[2].only_char, Some('c'));
    assert_eq!(info[0].cluster, 0);
    assert_eq!(info[1].cluster, 1);
    assert_eq!(info[2].cluster, 2);
}

/// Regression coverage for the `FontPair::shape_plans` cache added
/// alongside this test: a repeat `shape()` call with the same
/// (direction, script) must reuse the cached `rustybuzz::ShapePlan`
/// (asserted by checking the cache doesn't grow), while a call with a
/// *different* script must build and cache a distinct plan rather than
/// silently reusing the wrong one (which `rustybuzz::shape_with_plan`'s
/// own docs warn produces incorrect shaping, not a crash -- so this is
/// the test that would actually catch a wrong cache key, not just an
/// absent one).
#[test]
fn shape_plan_cache_is_reused_per_script_not_shared_across_scripts() {
    let _ = env_logger::Builder::new()
        .is_test(true)
        .filter_level(log::LevelFilter::Trace)
        .try_init();

    let config = onlyterm_config::configuration();
    let shaper = RustybuzzShaper::new(&config, &[jetbrains_handle()]).unwrap();
    let mut no_glyphs = vec![];

    // `self.fonts[0]` (the `FontPair`) isn't loaded until the first
    // `shape()` call reaches `load_fallback`, so this can only be called
    // after that first call.
    let plan_count = || {
        shaper.fonts[0]
            .borrow()
            .as_ref()
            .unwrap()
            .shape_plans
            .borrow()
            .len()
    };

    // First call, Latin script: builds and caches one plan.
    let latin1 = shaper
        .shape(
            "abc",
            10.,
            72,
            &mut no_glyphs,
            None,
            Direction::LeftToRight,
            None,
            None,
        )
        .unwrap();
    assert!(no_glyphs.is_empty(), "{:?}", no_glyphs);
    assert_eq!(
        plan_count(),
        1,
        "one plan cached after the first (Latin) call"
    );

    // Second call, same script: must reuse the cached plan, not grow the
    // cache, and must still shape correctly.
    let latin2 = shaper
        .shape(
            "def",
            10.,
            72,
            &mut no_glyphs,
            None,
            Direction::LeftToRight,
            None,
            None,
        )
        .unwrap();
    assert!(no_glyphs.is_empty(), "{:?}", no_glyphs);
    assert_eq!(
        plan_count(),
        1,
        "a second call with the same (direction, script) must reuse the cached plan"
    );
    assert_eq!(latin1.len(), 3);
    assert_eq!(latin2.len(), 3);
    assert_eq!(latin2[0].only_char, Some('d'));

    // Third call, different script (Cyrillic): must build and cache a
    // *second*, distinct plan rather than reusing the Latin one.
    let cyrillic = shaper
        .shape(
            "\u{431}\u{432}\u{433}", // "бвг"
            10.,
            72,
            &mut no_glyphs,
            None,
            Direction::LeftToRight,
            None,
            None,
        )
        .unwrap();
    assert!(no_glyphs.is_empty(), "{:?}", no_glyphs);
    assert_eq!(
        plan_count(),
        2,
        "a call with a different script must build and cache a distinct plan"
    );
    assert_eq!(cyrillic.len(), 3);
    assert_eq!(cyrillic[0].only_char, Some('\u{431}'));
    assert_eq!(cyrillic[1].only_char, Some('\u{432}'));
    assert_eq!(cyrillic[2].only_char, Some('\u{433}'));

    // Repeating the Cyrillic shape must reuse that second plan, not
    // build a third.
    let _ = shaper
        .shape(
            "\u{434}\u{435}\u{436}", // "дез" (different letters, same script)
            10.,
            72,
            &mut no_glyphs,
            None,
            Direction::LeftToRight,
            None,
            None,
        )
        .unwrap();
    assert!(no_glyphs.is_empty(), "{:?}", no_glyphs);
    assert_eq!(
        plan_count(),
        2,
        "a repeat call with an already-seen script must not grow the cache further"
    );
}

/// Regression coverage for
/// <https://github.com/wezterm/wezterm/issues/7963>: a fallback font
/// candidate whose backing file cannot be opened (originally reported
/// as a Windows Store / MSIX font living under an ACL-protected
/// `C:\Program Files\WindowsApps\...` path, denying access with "Access
/// is denied. (os error 5)") must not abort shaping for the whole text
/// run. The old `HarfbuzzShaper::load_fallback` (removed along with the
/// rest of the harfbuzz shaper in the freetype/harfbuzz -> rustybuzz/
/// swash migration) panicked in this situation; that panic could
/// escalate to a fatal crash (STATUS_FATAL_APP_EXIT) if a caught panic
/// unwind triggered a second panic, e.g. from a CLI spinner animation
/// re-triggering fallback resolution on every tick.
///
/// We don't attempt to reproduce real Windows ACL denial here (fragile
/// and platform-specific); instead we point a fallback candidate's
/// `FontDataSource::OnDisk` at a path that does not exist at all. From
/// `RustybuzzShaper::load_fallback`'s point of view this produces the
/// same shape of failure as an ACL-Denied open: `std::fs::read` (inside
/// `FontDataSource::load_data`, called by
/// `SwashFontInfo::from_locator`) returns an `Err`, and any IO error
/// there must be handled identically regardless of its underlying
/// `io::ErrorKind` (`NotFound`, `PermissionDenied`, etc.) -- the
/// resolver has no business special-casing one IO error kind over
/// another; all of them mean "this candidate is unusable, move on".
///
/// The fallback list here has the broken candidate at index 0 and a
/// real, working font (JetBrains Mono) at index 1. If the resolver
/// still worked the old (buggy) way -- propagating the open/parse
/// error out of `do_shape` via `?` -- this test would fail with
/// `shape(..).unwrap()` panicking on the propagated `Err`. With the
/// fix, `shape` logs a warning for the broken candidate and moves on
/// to shape successfully against font_idx=1.
#[test]
fn fallback_skips_unreadable_candidate() {
    let _ = env_logger::Builder::new()
        .is_test(true)
        .filter_level(log::LevelFilter::Trace)
        .try_init();

    let config = onlyterm_config::configuration();

    let unreadable_handle = ParsedFont::from_locator(&FontDataHandle {
        source: FontDataSource::OnDisk(std::path::PathBuf::from(
            "/this/path/does/not/exist/onlyterm-issue-7963-fallback-test.ttf",
        )),
        index: 0,
        variation: 0,
        origin: crate::locator::FontOrigin::FontDirs,
        coverage: None,
    });
    // `ParsedFont::from_locator` itself may already fail to build a
    // `ParsedFont` for a nonexistent path (it needs to peek at the file
    // to extract names/metrics) -- either way we want a `ParsedFont`
    // value to put in the handles list, because the real-world bug is
    // about a *resolved* fallback candidate (one that made it into the
    // handles list, e.g. because font enumeration read it from a
    // directory listing without opening it) whose file later can't be
    // opened when the shaper actually tries to load it. So if
    // constructing it from a bogus path fails up front, fall back to
    // building one from the real JetBrains Mono font and then
    // rewriting its `handle.source` to the bogus path -- this forges
    // exactly the "resolved candidate, unreadable file" scenario
    // `load_fallback` must tolerate.
    let mut broken = unreadable_handle.unwrap_or_else(|_| jetbrains_handle());
    broken.handle.source = FontDataSource::OnDisk(std::path::PathBuf::from(
        "/this/path/does/not/exist/onlyterm-issue-7963-fallback-test.ttf",
    ));

    let working = jetbrains_handle();

    let shaper = RustybuzzShaper::new(&config, &[broken, working]).unwrap();

    let mut no_glyphs = vec![];
    let info = shaper
        .shape(
            "abc",
            10.,
            72,
            &mut no_glyphs,
            None,
            Direction::LeftToRight,
            None,
            None,
        )
        .expect(
            "shape() must gracefully skip an unreadable fallback candidate \
             instead of propagating its IO/parse error (see #7963)",
        );

    assert!(no_glyphs.is_empty(), "{:?}", no_glyphs);
    assert_eq!(info.len(), 3);
    assert_eq!(info[0].only_char, Some('a'));
    assert_eq!(info[1].only_char, Some('b'));
    assert_eq!(info[2].only_char, Some('c'));
    for glyph in &info {
        assert_eq!(
            glyph.font_idx, 1,
            "expected glyphs to be shaped from the working fallback \
             candidate (font_idx=1), not the unreadable one: {:?}",
            info
        );
    }
}
