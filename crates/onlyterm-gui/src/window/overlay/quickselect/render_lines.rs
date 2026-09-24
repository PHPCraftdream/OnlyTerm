use super::*;

#[derive(Debug)]
pub(super) struct MatchResult {
    pub(super) range: Range<usize>,
    pub(super) label: String,
}

/// Plain-data view of a search-bar render for a single line, used by
/// [`decorate_quickselect_line`]. Kept as owned/borrowed data (no `&self`,
/// no `Window`) so the decoration logic can be exercised directly against a
/// real `Line` from a unit test.
pub(super) struct SearchBarInfo<'a> {
    pub(super) cols: usize,
    pub(super) selection: &'a str,
    pub(super) label: &'a str,
}

/// The matches (if any) that fall on the line being decorated, plus the
/// selection-prefix filter used by [`decorate_quickselect_line`] to decide
/// which labels are currently visible.
pub(super) struct QuickSelectMatchHighlights<'a> {
    pub(super) matches: Option<&'a [MatchResult]>,
    // `with_lines_mut` filters out labels that don't match the current
    // selection prefix (so unmatched labels visually disappear as the user
    // types); `get_lines` historically does not apply this filter. Preserve
    // that pre-existing asymmetry via this flag rather than changing
    // behavior as part of this refactor.
    pub(super) filter_by_selection: Option<&'a str>,
}

/// Decorates a single cloned pane `Line` for one QuickSelect render pass:
/// either replacing it with the search UI bar, or highlighting any matches/
/// labels that fall on this row. This is the exact per-line logic used by
/// both `Pane::with_lines_mut` and `Pane::get_lines` for `QuickSelectOverlay`
/// -- factored out so it can be unit tested against a real `Line` without a
/// live `window::Window`.
///
/// `render_seqno` must be used for every mutating call so that downstream
/// seqno-keyed caches (e.g. the GUI's shape_hash_cache) see each render pass
/// as a distinct version of the line; see `QuickSelectRenderable::render_seqno`.
pub(super) fn decorate_quickselect_line(
    line: &mut Line,
    render_seqno: SequenceNo,
    disable_attr: bool,
    is_search_row: bool,
    search_bar: &SearchBarInfo,
    highlights: QuickSelectMatchHighlights,
    colors: &config::Palette,
) {
    if disable_attr {
        line.cells_mut_for_attr_changes_only()
            .iter_mut()
            .for_each(|cell| cell.attrs_mut().clear());
        line.update_last_change_seqno(render_seqno);
        line.clear_appdata();
    }

    if is_search_row {
        // Replace with search UI
        let rev = CellAttributes::default().set_reverse(true).clone();
        line.fill_range(
            0..search_bar.cols,
            &Cell::new(' ', rev.clone()),
            render_seqno,
        );
        line.overlay_text_with_attribute(
            0,
            &format!(
                "Select: {}  (type highlighted prefix to {}, uppercase pastes, ESC to cancel)",
                search_bar.selection,
                if search_bar.label.is_empty() {
                    "copy"
                } else {
                    search_bar.label
                },
            ),
            rev,
            render_seqno,
        );
        line.clear_appdata();
        return;
    }

    let Some(matches) = highlights.matches else {
        return;
    };

    for m in matches {
        if let Some(lowered_prefix) = highlights.filter_by_selection {
            if !label_matches_selection(&m.label, lowered_prefix) {
                // Skip displaying this label, it doesn't match the current filter.
                continue;
            }
        }
        // highlight
        for cell_idx in m.range.clone() {
            if let Some(cell) = line.cells_mut_for_attr_changes_only().get_mut(cell_idx) {
                cell.attrs_mut()
                    .set_background(
                        colors
                            .quick_select_match_bg
                            .unwrap_or(AnsiColor::Black.into()),
                    )
                    .set_foreground(
                        colors
                            .quick_select_match_fg
                            .unwrap_or(AnsiColor::Green.into()),
                    )
                    .set_reverse(false)
                    .set_intensity(Intensity::Bold);
            }
        }
        for (idx, c) in m.label.chars().enumerate() {
            let mut attr = line
                .get_cell(idx)
                .map(|cell| cell.attrs().clone())
                .unwrap_or_default();
            attr.set_background(
                colors
                    .quick_select_label_bg
                    .unwrap_or(AnsiColor::Black.into()),
            )
            .set_foreground(
                colors
                    .quick_select_label_fg
                    .unwrap_or(AnsiColor::Olive.into()),
            )
            .set_reverse(false)
            .set_intensity(Intensity::Bold);
            line.set_cell(m.range.start + idx, Cell::new(c, attr), render_seqno);
        }
    }
    // cells_mut_for_attr_changes_only() above mutates cells directly
    // without bumping the line's seqno; do it explicitly so downstream
    // seqno-keyed caches see this pass as a new version of the line.
    line.update_last_change_seqno(render_seqno);
    line.clear_appdata();
}

/// Regression tests for the overlay-render-seqno fix.
///
/// Both CopyOverlay and QuickSelectOverlay mutate a *clone* of the
/// delegate pane's `Line` on every render pass (search bar text, match
/// highlighting, quickselect labels). Those clones inherit whatever seqno
/// the delegate's real line already has. Before this fix the overlays
/// tagged their own mutations with `SEQ_ZERO`; since
/// `Line::update_last_change_seqno` only ever increases a line's seqno
/// (`self.seqno = self.seqno.max(seqno)`), tagging with SEQ_ZERO on a line
/// whose seqno was already > 0 was a silent no-op. Downstream per-frame
/// caches keyed on (pane_id, stable_row, seqno) -- e.g. the GUI's
/// shape_hash_cache -- would then treat the overlay's mutated frame as
/// identical to a stale pre-overlay frame and keep serving the old shape,
/// so the search bar / highlights / labels could silently fail to appear.
///
/// These tests call the real `decorate_quickselect_line` function used by
/// `QuickSelectOverlay::with_lines_mut`/`get_lines` directly against a real
/// `Line`, without needing a live TermWindow/GUI (that function takes no
/// `&self`/`Window`, only plain data). This means reverting
/// `decorate_quickselect_line` (or its callers) back to tagging mutations
/// with `SEQ_ZERO` -- the bug fixed by facb0646e -- makes these tests fail,
/// unlike the old tests in this module which reimplemented the seqno-bump
/// counter locally and so passed regardless of what the production code did.
#[cfg(test)]
mod render_seqno_test {
    use super::*;
    use termwiz::surface::SEQ_ZERO;

    /// A line that already has server/terminal content at a real seqno,
    /// simulating a static (unchanging) pane that has been rendered once
    /// before an overlay was activated.
    fn make_line_with_seqno(seqno: usize) -> Line {
        let mut line = Line::with_width(10, SEQ_ZERO);
        line.fill_range(0..10, &Cell::new(' ', CellAttributes::default()), seqno);
        line
    }

    #[test]
    fn seq_zero_mutation_is_a_no_op_on_nonzero_line() {
        // This documents *why* the bug existed: update_last_change_seqno
        // never decreases the seqno, so re-tagging with SEQ_ZERO after a
        // real mutation leaves current_seqno() unchanged.
        let mut line = make_line_with_seqno(5);
        assert_eq!(line.current_seqno(), 5);

        line.fill_range(0..10, &Cell::new('x', CellAttributes::default()), SEQ_ZERO);
        assert_eq!(
            line.current_seqno(),
            5,
            "SEQ_ZERO must not appear to change a line that already has a higher seqno"
        );
    }

    /// Two successive real render passes (via `decorate_quickselect_line`)
    /// over the SAME underlying static delegate line (same seqno both
    /// times, as happens while the user types into the search bar but the
    /// underlying pane content does not change) must produce distinct
    /// `current_seqno()` values on the decorated clones, and those values
    /// must exceed the delegate's real seqno. If `decorate_quickselect_line`
    /// were reverted to stamp mutations with `SEQ_ZERO` instead of
    /// `render_seqno`, both assertions below would fail.
    #[test]
    fn quickselect_search_bar_render_bumps_seqno_past_delegate_and_across_passes() {
        let delegate_seqno = 5;
        let search_bar_1 = SearchBarInfo {
            cols: 10,
            selection: "a",
            label: "",
        };
        let search_bar_2 = SearchBarInfo {
            cols: 10,
            selection: "ab",
            label: "",
        };

        let mut line1 = make_line_with_seqno(delegate_seqno);
        let pass1_seqno = usize::MAX / 2 + 1;
        decorate_quickselect_line(
            &mut line1,
            pass1_seqno,
            false,
            true, // is_search_row
            &search_bar_1,
            QuickSelectMatchHighlights {
                matches: None,
                filter_by_selection: None,
            },
            &config::Palette::default(),
        );

        let mut line2 = make_line_with_seqno(delegate_seqno);
        let pass2_seqno = usize::MAX / 2 + 2;
        decorate_quickselect_line(
            &mut line2,
            pass2_seqno,
            false,
            true, // is_search_row
            &search_bar_2,
            QuickSelectMatchHighlights {
                matches: None,
                filter_by_selection: None,
            },
            &config::Palette::default(),
        );

        assert_eq!(line1.current_seqno(), pass1_seqno);
        assert_eq!(line2.current_seqno(), pass2_seqno);
        assert_ne!(
            line1.current_seqno(),
            line2.current_seqno(),
            "two different overlay render passes over the same underlying \
             line must produce different current_seqno() values, so a \
             (pane_id, stable_row, seqno)-keyed cache warmed by pass 1 is \
             correctly invalidated for pass 2's different content"
        );
        assert!(line1.current_seqno() > delegate_seqno);
        assert!(line2.current_seqno() > delegate_seqno);

        // Content sanity: the search bar text was actually written.
        let text: String = (0..10)
            .filter_map(|i| line1.get_cell(i).map(|c| c.str().to_string()))
            .collect();
        assert!(text.starts_with("Select"));
    }

    /// The match/label-highlight branch of `decorate_quickselect_line` uses
    /// `cells_mut_for_attr_changes_only()`, which does NOT itself bump the
    /// line's seqno tracking -- the function must call
    /// `update_last_change_seqno` explicitly afterwards. This exercises
    /// that branch directly.
    #[test]
    fn quickselect_match_highlight_requires_explicit_seqno_bump() {
        let delegate_seqno = 5;
        let mut line = make_line_with_seqno(delegate_seqno);
        assert_eq!(line.current_seqno(), delegate_seqno);

        let matches = vec![MatchResult {
            range: 0..1,
            label: "a".to_string(),
        }];
        let search_bar = SearchBarInfo {
            cols: 10,
            selection: "",
            label: "",
        };
        let render_seqno = usize::MAX / 2 + 1;
        decorate_quickselect_line(
            &mut line,
            render_seqno,
            false,
            false, // not the search row: takes the match-highlight branch
            &search_bar,
            QuickSelectMatchHighlights {
                matches: Some(&matches),
                filter_by_selection: None,
            },
            &config::Palette::default(),
        );

        assert_eq!(
            line.current_seqno(),
            render_seqno,
            "decorate_quickselect_line's match-highlight branch mutates \
             cells directly via cells_mut_for_attr_changes_only(), which \
             does not itself bump the seqno -- an explicit \
             update_last_change_seqno(render_seqno) call is required, and \
             tagging with SEQ_ZERO here would be a silent no-op on this \
             already-nonzero-seqno line"
        );
        assert!(line.current_seqno() > delegate_seqno);
    }
}
