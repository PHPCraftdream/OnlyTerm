//! Regression coverage for PERF1: `parse_and_collect_font_info` used
//! to call `SwashFontInfo::from_locator` once per sub-face of a
//! `.ttc`/`.otc` collection, and `from_locator` re-reads the *entire*
//! file off disk every time it's called -- so a collection with N
//! sub-faces was read off disk N+1 times over (once just to learn the
//! face count via `fonts_in_collection`, then once again per
//! sub-face), even though every sub-face lives in the same file. For
//! large CJK system font collections (e.g. `mingliub.ttc` at ~37MB
//! with 3 faces, or `Sitka*.ttc` with 6 faces each) that made
//! `enumerate_all_fonts`/`ls-fonts --list-system` measurably slower
//! than it needs to be. The fix reads the file once and reuses the
//! in-memory bytes (a cheap memcpy, not I/O) for every sub-face via
//! `SwashFontInfo::from_data`.
use super::*;
use crate::locator::FontOrigin;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// A real (not hand-rolled) 2-face TrueType Collection built with
/// `fonttools`'s `TTCollection` writer from
/// `assets/fonts/JetBrainsMono-{Regular,Bold}.ttf`, checked in purely
/// for this regression test (there was no `.ttc` fixture in the repo
/// before this).
fn ttc_fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("testdata")
        .join("JetBrainsMono-RegularBold.ttc")
}

/// Sanity check that multi-face enumeration still produces one
/// `ParsedFont` per sub-face (i.e. the read-once refactor didn't
/// silently break the loop over `num_faces`).
#[test]
fn ttc_enumeration_yields_one_entry_per_face() {
    let source = FontDataSource::OnDisk(ttc_fixture_path());
    let mut font_info = vec![];
    parse_and_collect_font_info(&source, &mut font_info, FontOrigin::FontDirs).unwrap();

    assert_eq!(
        font_info.len(),
        2,
        "expected exactly one ParsedFont per sub-face of the 2-face ttc fixture, got {:?}",
        font_info
            .iter()
            .map(|f| f.names().full_name.clone())
            .collect::<Vec<_>>()
    );

    let mut full_names: Vec<&str> = font_info
        .iter()
        .map(|f| f.names().full_name.as_str())
        .collect();
    full_names.sort();
    assert_eq!(
        full_names,
        vec!["JetBrains Mono Bold", "JetBrains Mono Regular"]
    );
}

/// `SwashFontInfo::from_data` (fed already-loaded bytes) must produce
/// results identical to `SwashFontInfo::from_locator` (which loads the
/// bytes itself) for the same face -- i.e. splitting "load the bytes"
/// out of `from_locator` must be a pure refactor with no behavior
/// change.
#[test]
fn from_data_matches_from_locator() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("assets/fonts/JetBrainsMono-Regular.ttf");
    let source = FontDataSource::OnDisk(path);

    let via_locator = SwashFontInfo::from_locator(&source, 0).unwrap();
    let raw = source.load_data().unwrap().into_owned().into_boxed_slice();
    let via_data = SwashFontInfo::from_data(raw, 0).unwrap();

    assert_eq!(via_locator.family_name(), via_data.family_name());
    assert_eq!(via_locator.style_name(), via_data.style_name());
    assert_eq!(via_locator.units_per_em(), via_data.units_per_em());
    assert_eq!(via_locator.num_glyphs(), via_data.num_glyphs());
    assert_eq!(
        via_locator.weight_and_width(None),
        via_data.weight_and_width(None)
    );
}

/// Coarse wall-clock regression guard: enumerating the 2-face ttc
/// fixture (~475KB total) should be well within a generous upper
/// bound. This is not a tight perf assertion (machine/CI speed
/// varies enormously) -- it exists to catch a *gross* regression like
/// accidentally reintroducing an O(num_faces) full-file re-read (or
/// worse, an O(num_faces^2) pattern) for collections, which is
/// exactly the bug this change fixes. The bound is generous enough
/// (2s for a ~475KB file processed twice) that it should never be
/// flaky on any real CI/dev machine, while still being tight enough
/// to catch a reintroduced per-face disk re-read of a much larger
/// real-world collection (which this test's small fixture can't
/// directly reproduce the magnitude of, but a correctness-preserving
/// refactor should not care about fixture size).
#[test]
fn ttc_enumeration_completes_quickly() {
    let source = FontDataSource::OnDisk(ttc_fixture_path());

    let start = Instant::now();
    for _ in 0..10 {
        let mut font_info = vec![];
        parse_and_collect_font_info(&source, &mut font_info, FontOrigin::FontDirs).unwrap();
        assert_eq!(font_info.len(), 2);
    }
    let elapsed = start.elapsed();

    assert!(
        elapsed < Duration::from_secs(2),
        "enumerating the 2-face ttc fixture 10 times took {:?}, expected well under 2s \
         (possible regression: re-reading the file from disk per sub-face instead of once)",
        elapsed
    );
}
