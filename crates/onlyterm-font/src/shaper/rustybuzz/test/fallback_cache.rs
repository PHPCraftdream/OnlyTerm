use super::*;

#[test]
fn fallback_only_runs_keep_parsed_primary_font() {
    let handles = primary_then_hebrew_fallback_handles();
    let shaper = RustybuzzShaper::new(&config::configuration(), &handles).unwrap();
    let mut missing = Vec::new();
    let text = "שלום";
    let first = shaper
        .shape(
            text,
            14.,
            96,
            &mut missing,
            None,
            Direction::RightToLeft,
            None,
            None,
        )
        .unwrap();
    assert!(missing.is_empty());
    assert!(first.iter().all(|glyph| glyph.font_idx == 1));
    assert!(
        shaper.fonts[0].borrow().is_some(),
        "a coverage miss must not discard a parsed font and its shaping plans"
    );
    let face_ptr = shaper.fonts[0]
        .borrow()
        .as_ref()
        .unwrap()
        .rb_face
        .borrow()
        .as_ref()
        .unwrap()
        ._data
        .as_ptr();
    let second = shaper
        .shape(
            text,
            14.,
            96,
            &mut missing,
            None,
            Direction::RightToLeft,
            None,
            None,
        )
        .unwrap();
    assert_eq!(first.len(), second.len());
    for (before, after) in first.iter().zip(&second) {
        assert_eq!(
            (
                before.font_idx,
                before.glyph_pos,
                before.cluster,
                before.num_cells
            ),
            (
                after.font_idx,
                after.glyph_pos,
                after.cluster,
                after.num_cells
            )
        );
        assert_eq!(before.x_advance, after.x_advance);
        assert_eq!(before.x_offset, after.x_offset);
    }
    assert_eq!(
        face_ptr,
        shaper.fonts[0]
            .borrow()
            .as_ref()
            .unwrap()
            .rb_face
            .borrow()
            .as_ref()
            .unwrap()
            ._data
            .as_ptr()
    );
}

#[test]
#[ignore = "manual CJK measurement; requires ONLYTERM_CJK_FONT pointing at a font file"]
fn cjk_repeated_shape_probe() {
    let path = std::env::var_os("ONLYTERM_CJK_FONT").expect("set ONLYTERM_CJK_FONT");
    let cjk = ParsedFont::from_locator(&FontDataHandle {
        source: FontDataSource::OnDisk(path.into()),
        index: 0,
        variation: 0,
        origin: crate::locator::FontOrigin::FontDirs,
        coverage: None,
    })
    .unwrap();
    let handles = vec![jetbrains_handle(), hebrew_fallback_handle(), cjk];
    let shaper = RustybuzzShaper::new(&config::configuration(), &handles).unwrap();
    let text = "春天的清晨阳光照进安静的书房窗外青山绿水远处传来鸟鸣人们沿着河岸慢慢散步";
    let started = std::time::Instant::now();
    let mut unloaded = 0;
    for _ in 0..100 {
        let mut missing = Vec::new();
        let glyphs = shaper
            .shape(
                text,
                14.,
                96,
                &mut missing,
                None,
                Direction::LeftToRight,
                None,
                None,
            )
            .unwrap();
        assert!(missing.is_empty(), "missing: {missing:?}");
        assert!(!glyphs.is_empty());
        unloaded += shaper
            .fonts
            .iter()
            .filter(|font| font.borrow().is_none())
            .count();
    }
    eprintln!(
        "CJK 100 repeated rows: {:?}; unloaded font slots after rows: {}",
        started.elapsed(),
        unloaded
    );
}
