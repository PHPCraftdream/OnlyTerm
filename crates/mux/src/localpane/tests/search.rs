use super::*;

fn search_pane_output(pane: &Arc<LocalPane>, output: &str) {
    pane.terminal.lock().advance_bytes(output);
}

#[test]
fn search_limit_zero_returns_no_results() {
    let (pane, _) = make_pane();
    search_pane_output(&pane, "foo foo");
    let results = smol::block_on(pane.search(
        Pattern::CaseSensitiveString("foo".to_string()),
        0..ROWS as isize,
        Some(0),
    ))
    .expect("search should succeed");
    assert!(results.is_empty());
}

#[test]
fn search_collects_multiple_matches_across_rows() {
    let (pane, _) = make_pane();
    search_pane_output(&pane, "foo foo\r\nfoo");
    let results = smol::block_on(pane.search(
        Pattern::CaseSensitiveString("foo".to_string()),
        0..ROWS as isize,
        None,
    ))
    .expect("search should succeed");
    assert_eq!(results.len(), 3);
}

#[test]
fn search_limit_one_stops_inside_a_line() {
    let (pane, _) = make_pane();
    search_pane_output(&pane, "foo foo\r\nfoo");
    let results = smol::block_on(pane.search(
        Pattern::CaseSensitiveString("foo".to_string()),
        0..ROWS as isize,
        Some(1),
    ))
    .expect("search should succeed");
    assert_eq!(results.len(), 1);
}

#[test]
fn search_uses_the_last_regex_capture() {
    let (pane, _) = make_pane();
    search_pane_output(&pane, "foo-bar");
    let results = smol::block_on(pane.search(
        Pattern::Regex("(foo)-(bar)".to_string()),
        0..ROWS as isize,
        None,
    ))
    .expect("search should succeed");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].start_x, 4);
    assert_eq!(results[0].end_x, 7);
}

#[test]
fn search_case_insensitive_preserves_expanding_and_contextual_unicode() {
    let (pane, _) = make_pane();
    search_pane_output(&pane, "\u{130}x \u{39f}\u{3a3}");

    let expanding = smol::block_on(pane.search(
        Pattern::CaseInSensitiveString("i\u{307}".to_string()),
        0..ROWS as isize,
        None,
    ))
    .expect("search should succeed");
    assert_eq!(expanding.len(), 1);
    assert_eq!((expanding[0].start_x, expanding[0].end_x), (0, 1));

    let combining_mark = smol::block_on(pane.search(
        Pattern::CaseInSensitiveString("\u{307}".to_string()),
        0..ROWS as isize,
        None,
    ))
    .expect("search should succeed");
    assert_eq!(combining_mark.len(), 1);
    assert_eq!((combining_mark[0].start_x, combining_mark[0].end_x), (0, 1));

    let contextual = smol::block_on(pane.search(
        Pattern::CaseInSensitiveString("\u{3bf}\u{3c2}".to_string()),
        0..ROWS as isize,
        None,
    ))
    .expect("search should succeed");
    assert_eq!(contextual.len(), 1);
}

#[test]
fn search_maps_cjk_end_to_the_full_cell_width() {
    let (pane, _) = make_pane();
    search_pane_output(&pane, "\u{754c}");
    let results = smol::block_on(pane.search(
        Pattern::CaseSensitiveString("\u{754c}".to_string()),
        0..ROWS as isize,
        None,
    ))
    .expect("search should succeed");
    assert_eq!(results.len(), 1);
    assert_eq!((results[0].start_x, results[0].end_x), (0, 2));
}

#[test]
fn search_for_combining_mark_highlights_its_original_cell() {
    let (pane, _) = make_pane();
    search_pane_output(&pane, "a\u{301}z");
    for pattern in [
        Pattern::CaseSensitiveString("\u{301}".into()),
        Pattern::Regex("\u{301}".into()),
    ] {
        let found = smol::block_on(pane.search(pattern, 0..ROWS as isize, None)).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].start_x, found[0].end_x), (0, 1));
    }
}

#[test]
fn search_matches_across_a_wrapped_logical_line() {
    let (pane, _) = make_pane();
    search_pane_output(&pane, &format!("{}BC", "a".repeat(COLS - 1)));
    let results = smol::block_on(pane.search(
        Pattern::CaseSensitiveString("aBC".to_string()),
        0..ROWS as isize,
        None,
    ))
    .expect("search should succeed");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].start_x, COLS - 2);
    assert_eq!(results[0].end_x, 1);
    assert_eq!(results[0].end_y, results[0].start_y + 1);
}

#[test]
fn search_nonzero_range_does_not_drop_requested_physical_rows() {
    let (pane, _) = make_pane();
    search_pane_output(&pane, "foo\r\nfoo\r\nfoo\r\nfoo");
    let results =
        smol::block_on(pane.search(Pattern::CaseSensitiveString("foo".into()), 1..3, None))
            .unwrap();
    assert_eq!(
        results.iter().map(|m| m.start_y).collect::<Vec<_>>(),
        [1, 2]
    );
}

#[test]
fn search_expands_both_ends_of_a_wrapped_line() {
    let (pane, _) = make_pane();
    search_pane_output(&pane, &format!("{}BC", "a".repeat(COLS - 1)));
    for range in [0..1, 1..2] {
        let results =
            smol::block_on(pane.search(Pattern::CaseSensitiveString("aBC".into()), range, None))
                .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!((results[0].start_y, results[0].start_x), (0, COLS - 2));
        assert_eq!((results[0].end_y, results[0].end_x), (1, 1));
    }
}

#[test]
fn search_continues_a_logical_line_across_physical_snapshot_chunks() {
    let (pane, _) = make_pane();
    let output = format!("{}BC", "a".repeat(COLS * 260 - 1));
    search_pane_output(&pane, &output);
    let results = smol::block_on(pane.search(
        Pattern::CaseSensitiveString("aBC".to_string()),
        0..1024,
        None,
    ))
    .expect("search should succeed");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].end_x, 1);
    assert!(results[0].end_y > results[0].start_y);
}
