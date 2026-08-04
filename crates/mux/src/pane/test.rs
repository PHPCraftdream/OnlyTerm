    use super::*;
    use k9::snapshot;
    use parking_lot::{MappedMutexGuard, Mutex};
    use std::borrow::Cow;
    use termwiz::surface::SEQ_ZERO;

    struct FakePane {
        lines: Mutex<Vec<Line>>,
    }

    impl Pane for FakePane {
        fn pane_id(&self) -> PaneId {
            unimplemented!()
        }
        fn get_cursor_position(&self) -> StableCursorPosition {
            unimplemented!()
        }

        fn get_current_seqno(&self) -> SequenceNo {
            unimplemented!()
        }

        fn get_changed_since(
            &self,
            _: Range<StableRowIndex>,
            _: SequenceNo,
        ) -> RangeSet<StableRowIndex> {
            unimplemented!()
        }

        fn with_lines_mut(
            &self,
            stable_range: Range<StableRowIndex>,
            with_lines: &mut dyn WithPaneLines,
        ) {
            let mut line_refs = vec![];
            let mut lines = self.lines.lock();
            for line in lines
                .iter_mut()
                .skip(stable_range.start as usize)
                .take((stable_range.end - stable_range.start) as usize)
            {
                line_refs.push(line);
            }
            with_lines.with_lines_mut(stable_range.start, &mut line_refs);
        }

        fn for_each_logical_line_in_stable_range_mut(
            &self,
            lines: Range<StableRowIndex>,
            for_line: &mut dyn ForEachPaneLogicalLine,
        ) {
            crate::pane::impl_for_each_logical_line_via_get_logical_lines(self, lines, for_line)
        }

        fn get_logical_lines(&self, lines: Range<StableRowIndex>) -> Vec<LogicalLine> {
            crate::pane::impl_get_logical_lines_via_get_lines(self, lines)
        }

        fn get_lines(&self, lines: Range<StableRowIndex>) -> (StableRowIndex, Vec<Line>) {
            let first = lines.start;
            (
                first,
                self.lines
                    .lock()
                    .iter()
                    .skip(lines.start as usize)
                    .take((lines.end - lines.start) as usize)
                    .cloned()
                    .collect(),
            )
        }
        fn get_dimensions(&self) -> RenderableDimensions {
            unimplemented!()
        }

        fn get_title(&self) -> String {
            unimplemented!()
        }
        fn send_paste(&self, _: &str) -> anyhow::Result<()> {
            unimplemented!()
        }
        fn reader(&self) -> anyhow::Result<Option<Box<dyn std::io::Read + Send>>> {
            Ok(None)
        }
        fn writer(&self) -> MappedMutexGuard<'_, dyn std::io::Write> {
            unimplemented!()
        }
        fn resize(&self, _: TerminalSize) -> anyhow::Result<()> {
            unimplemented!()
        }

        fn mouse_event(&self, _: MouseEvent) -> anyhow::Result<()> {
            unimplemented!()
        }
        fn is_dead(&self) -> bool {
            unimplemented!()
        }
        fn palette(&self) -> ColorPalette {
            unimplemented!()
        }
        fn domain_id(&self) -> DomainId {
            unimplemented!()
        }

        fn is_mouse_grabbed(&self) -> bool {
            false
        }
        fn is_alt_screen_active(&self) -> bool {
            false
        }
        fn get_current_working_dir(&self, _policy: CachePolicy) -> Option<Url> {
            None
        }
        fn key_down(&self, _: KeyCode, _: KeyModifiers) -> anyhow::Result<()> {
            unimplemented!()
        }
        fn key_up(&self, _: KeyCode, _: KeyModifiers) -> anyhow::Result<()> {
            unimplemented!()
        }
    }

    fn physical_lines_from_text(text: &str, width: usize) -> Vec<Line> {
        let mut physical_lines = vec![];
        for logical in text.split('\n') {
            let chunks = logical
                .chars()
                .collect::<Vec<char>>()
                .chunks(width)
                .map(|c| c.iter().collect::<String>())
                .collect::<Vec<String>>();
            let n_chunks = chunks.len();
            for (idx, chunk) in chunks.into_iter().enumerate() {
                let mut line = Line::from_text(&chunk, &Default::default(), 1, None);
                if idx < n_chunks - 1 {
                    line.set_last_cell_was_wrapped(true, 1);
                }
                physical_lines.push(line);
            }
        }
        physical_lines
    }

    fn summarize_logical_lines(lines: &[LogicalLine]) -> Vec<(StableRowIndex, Cow<'_, str>)> {
        lines
            .iter()
            .map(|l| (l.first_row, l.logical.as_str()))
            .collect::<Vec<_>>()
    }

    #[test]
    fn logical_lines() {
        let text = "Hello there this is a long line.\nlogical line two\nanother long line here\nlogical line four\nlogical line five\ncap it off with another long line";
        let width = 20;
        let physical_lines = physical_lines_from_text(text, width);

        fn text_from_lines(lines: &[Line]) -> Vec<Cow<'_, str>> {
            lines.iter().map(|l| l.as_str()).collect::<Vec<_>>()
        }

        let line_text = text_from_lines(&physical_lines);
        snapshot!(
            line_text,
            r#"
[
    "Hello there this is ",
    "a long line.",
    "logical line two",
    "another long line he",
    "re",
    "logical line four",
    "logical line five",
    "cap it off with anot",
    "her long line",
]
"#
        );

        let pane = FakePane {
            lines: Mutex::new(physical_lines),
        };

        let logical = pane.get_logical_lines(0..30);
        snapshot!(
            summarize_logical_lines(&logical),
            r#"
[
    (
        0,
        "Hello there this is a long line.",
    ),
    (
        2,
        "logical line two",
    ),
    (
        3,
        "another long line here",
    ),
    (
        5,
        "logical line four",
    ),
    (
        6,
        "logical line five",
    ),
    (
        7,
        "cap it off with another long line",
    ),
]
"#
        );

        // Now try with offset bounds
        let offset = pane.get_logical_lines(1..3);
        snapshot!(
            summarize_logical_lines(&offset),
            r#"
[
    (
        0,
        "Hello there this is a long line.",
    ),
    (
        2,
        "logical line two",
    ),
]
"#
        );

        let offset = pane.get_logical_lines(1..4);
        snapshot!(
            summarize_logical_lines(&offset),
            r#"
[
    (
        0,
        "Hello there this is a long line.",
    ),
    (
        2,
        "logical line two",
    ),
    (
        3,
        "another long line here",
    ),
]
"#
        );

        let offset = pane.get_logical_lines(1..5);
        snapshot!(
            summarize_logical_lines(&offset),
            r#"
[
    (
        0,
        "Hello there this is a long line.",
    ),
    (
        2,
        "logical line two",
    ),
    (
        3,
        "another long line here",
    ),
]
"#
        );

        let offset = pane.get_logical_lines(1..6);
        snapshot!(
            summarize_logical_lines(&offset),
            r#"
[
    (
        0,
        "Hello there this is a long line.",
    ),
    (
        2,
        "logical line two",
    ),
    (
        3,
        "another long line here",
    ),
    (
        5,
        "logical line four",
    ),
]
"#
        );

        let offset = pane.get_logical_lines(1..7);
        snapshot!(
            summarize_logical_lines(&offset),
            r#"
[
    (
        0,
        "Hello there this is a long line.",
    ),
    (
        2,
        "logical line two",
    ),
    (
        3,
        "another long line here",
    ),
    (
        5,
        "logical line four",
    ),
    (
        6,
        "logical line five",
    ),
]
"#
        );

        let offset = pane.get_logical_lines(1..8);
        snapshot!(
            summarize_logical_lines(&offset),
            r#"
[
    (
        0,
        "Hello there this is a long line.",
    ),
    (
        2,
        "logical line two",
    ),
    (
        3,
        "another long line here",
    ),
    (
        5,
        "logical line four",
    ),
    (
        6,
        "logical line five",
    ),
    (
        7,
        "cap it off with another long line",
    ),
]
"#
        );

        let line = &offset[0];
        let coords = (0..line.logical.len())
            .map(|idx| line.logical_x_to_physical_coord(idx))
            .collect::<Vec<_>>();
        snapshot!(
            coords,
            "
[
    (
        0,
        0,
    ),
    (
        0,
        1,
    ),
    (
        0,
        2,
    ),
    (
        0,
        3,
    ),
    (
        0,
        4,
    ),
    (
        0,
        5,
    ),
    (
        0,
        6,
    ),
    (
        0,
        7,
    ),
    (
        0,
        8,
    ),
    (
        0,
        9,
    ),
    (
        0,
        10,
    ),
    (
        0,
        11,
    ),
    (
        0,
        12,
    ),
    (
        0,
        13,
    ),
    (
        0,
        14,
    ),
    (
        0,
        15,
    ),
    (
        0,
        16,
    ),
    (
        0,
        17,
    ),
    (
        0,
        18,
    ),
    (
        0,
        19,
    ),
    (
        1,
        0,
    ),
    (
        1,
        1,
    ),
    (
        1,
        2,
    ),
    (
        1,
        3,
    ),
    (
        1,
        4,
    ),
    (
        1,
        5,
    ),
    (
        1,
        6,
    ),
    (
        1,
        7,
    ),
    (
        1,
        8,
    ),
    (
        1,
        9,
    ),
    (
        1,
        10,
    ),
    (
        1,
        11,
    ),
]
"
        );
    }

    fn is_double_click_word(s: &str) -> bool {
        match s.chars().count() {
            1 => !" \t\n{[}]()\"'`".contains(s),
            0 => false,
            _ => true,
        }
    }

    #[test]
    fn double_click() {
        let attr = Default::default();
        let logical = LogicalLine {
            physical_lines: vec![
                Line::from_text("hello", &attr, SEQ_ZERO, None),
                Line::from_text("yo", &attr, SEQ_ZERO, None),
            ],
            logical: Line::from_text("helloyo", &attr, SEQ_ZERO, None),
            first_row: 0,
        };

        assert_eq!(logical.xy_to_logical_x(2, -1), 0);
        assert_eq!(logical.xy_to_logical_x(20, 1), 25);

        let start_idx = logical.xy_to_logical_x(2, 1);

        use termwiz::surface::line::DoubleClickRange;

        assert_eq!(start_idx, 7);
        match logical
            .logical
            .compute_double_click_range(start_idx, is_double_click_word)
        {
            DoubleClickRange::Range(click_range) => {
                assert_eq!(click_range, 7..7);
            }
            _ => unreachable!(),
        }
    }
