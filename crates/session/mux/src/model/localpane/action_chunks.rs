use super::*;
use finl_unicode::grapheme_clusters::Graphemes;

const MAX_PRINT_BYTES_PER_CHUNK: usize = 64 * 1024;

enum ActionChunk<'a> {
    Actions(&'a mut Vec<Action>),
    Text(&'a str),
}

impl ActionChunk<'_> {
    fn len(&self) -> usize {
        match self {
            Self::Actions(actions) => actions.len(),
            Self::Text(_) => 1,
        }
    }

    fn print_bytes(&self) -> usize {
        match self {
            Self::Actions(actions) => actions.iter().map(print_work).sum(),
            Self::Text(text) => text.len(),
        }
    }

    fn apply(self, term: &mut onlyterm_term::Terminal) {
        match self {
            Self::Actions(actions) => term.perform_actions_reusing(actions),
            Self::Text(text) => term.perform_print_fragment(text),
        }
    }
}

fn print_work(action: &Action) -> usize {
    match action {
        Action::Print(c) => c.len_utf8(),
        Action::PrintString(text) => text.len(),
        _ => 0,
    }
}

fn print_fragment_len(text: &str) -> usize {
    if text.len() <= MAX_PRINT_BYTES_PER_CHUNK {
        return text.len();
    }
    let bytes = text.as_bytes();
    let end = MAX_PRINT_BYTES_PER_CHUNK;
    if bytes[end - 1].is_ascii() && bytes[end].is_ascii() {
        return if bytes[end - 1] == b'\r' && bytes[end] == b'\n' {
            end - 1
        } else {
            end
        };
    }
    let mut end = 0;
    for grapheme in Graphemes::new(text) {
        if end != 0 && grapheme.len() > MAX_PRINT_BYTES_PER_CHUNK - end {
            break;
        }
        end += grapheme.len();
        // An indivisible grapheme can exceed the soft byte budget.
        if end >= MAX_PRINT_BYTES_PER_CHUNK {
            break;
        }
    }
    end
}

impl LocalPane {
    // Release terminal.lock between chunks; retain resize_guard for the batch.
    pub(super) fn perform_actions_chunked(&self, actions: Vec<Action>, chunk_size: usize) {
        let profile_enabled = onlyterm_metrics::profile_pipeline_enabled();
        self.with_action_chunks(actions, chunk_size, |chunk| {
            if profile_enabled {
                onlyterm_metrics::cached_histogram!("localpane.perform_actions.chunk_actions.size")
                    .record(chunk.len() as f64);
                onlyterm_metrics::cached_histogram!(
                    "localpane.perform_actions.chunk_print_bytes.size"
                )
                .record(chunk.print_bytes() as f64);
            }
            lock_terminal_timed(
                &self.terminal,
                "localpane.terminal_lock.wait.perform_actions",
                |term| {
                    let hold_start = if profile_enabled {
                        Some(Instant::now())
                    } else {
                        None
                    };
                    chunk.apply(term);
                    if let Some(hold_start) = hold_start {
                        onlyterm_metrics::cached_histogram!(
                            "localpane.terminal_lock.hold.perform_actions"
                        )
                        .record(hold_start.elapsed());
                    }
                },
            );
        });
    }

    fn with_action_chunks(
        &self,
        mut actions: Vec<Action>,
        chunk_size: usize,
        mut apply: impl FnMut(ActionChunk<'_>),
    ) {
        let chunk_size = chunk_size.max(1);
        if actions.len() <= chunk_size
            && actions
                .iter()
                .try_fold(MAX_PRINT_BYTES_PER_CHUNK, |remaining, action| {
                    remaining.checked_sub(print_work(action))
                })
                .is_some()
        {
            apply(ActionChunk::Actions(&mut actions));
            return;
        }
        let _resize_guard = self.resize_guard.lock();
        let capacity = chunk_size.min(actions.len());
        let mut chunk = Vec::new();
        let mut print_bytes = 0;
        let mut actions = actions.into_iter().peekable();
        while let Some(mut action) = actions.next() {
            if matches!(action, Action::Print(_) | Action::PrintString(_))
                && matches!(
                    actions.peek(),
                    Some(Action::Print(_) | Action::PrintString(_))
                )
            {
                let mut text = match action {
                    Action::Print(c) => c.to_string(),
                    Action::PrintString(text) => text,
                    _ => unreachable!(),
                };
                while matches!(
                    actions.peek(),
                    Some(Action::Print(_) | Action::PrintString(_))
                ) {
                    match actions.next().expect("the next print was inspected") {
                        Action::Print(c) => text.push(c),
                        Action::PrintString(next) => text.push_str(&next),
                        _ => unreachable!(),
                    }
                }
                action = Action::PrintString(text);
            }
            match action {
                Action::PrintString(text) if text.len() > MAX_PRINT_BYTES_PER_CHUNK => {
                    if !chunk.is_empty() {
                        apply(ActionChunk::Actions(&mut chunk));
                        print_bytes = 0;
                    }
                    let mut remaining = text.as_str();
                    while !remaining.is_empty() {
                        let (fragment, tail) = remaining.split_at(print_fragment_len(remaining));
                        apply(ActionChunk::Text(fragment));
                        remaining = tail;
                    }
                }
                action => {
                    let work = print_work(&action);
                    if work > MAX_PRINT_BYTES_PER_CHUNK - print_bytes {
                        apply(ActionChunk::Actions(&mut chunk));
                        print_bytes = 0;
                    }
                    if chunk.capacity() == 0 {
                        chunk.reserve(capacity);
                    }
                    chunk.push(action);
                    print_bytes += work;
                    if chunk.len() == chunk_size {
                        apply(ActionChunk::Actions(&mut chunk));
                        print_bytes = 0;
                    }
                }
            }
        }
        if !chunk.is_empty() {
            apply(ActionChunk::Actions(&mut chunk));
        }
    }

    #[cfg(test)]
    pub(crate) fn perform_actions_chunked_timed(
        &self,
        actions: Vec<Action>,
        chunk_size: usize,
    ) -> Vec<(Duration, Duration)> {
        self.perform_actions_chunked_measured(actions, chunk_size).0
    }

    #[cfg(test)]
    pub(crate) fn perform_actions_chunked_measured(
        &self,
        actions: Vec<Action>,
        chunk_size: usize,
    ) -> (Vec<(Duration, Duration)>, Vec<usize>) {
        let mut samples = Vec::new();
        let mut sizes = Vec::new();
        self.with_action_chunks(actions, chunk_size, |chunk| {
            let wait_start = Instant::now();
            let mut term = self.terminal.lock();
            let waited = wait_start.elapsed();
            let hold_start = Instant::now();
            sizes.push(chunk.len());
            chunk.apply(&mut term);
            samples.push((waited, hold_start.elapsed()));
        });
        (samples, sizes)
    }
}

#[cfg(test)]
mod budget_tests {
    use super::*;
    use onlyterm_term::{Alert, AlertHandler, TerminalConfiguration, TerminalSize};

    struct NoAlerts;
    impl AlertHandler for NoAlerts {
        fn alert(&mut self, _alert: Alert) {}
    }

    #[derive(Debug)]
    struct Config {
        normalize: bool,
    }

    impl TerminalConfiguration for Config {
        fn color_palette(&self) -> onlyterm_term::color::ColorPalette {
            onlyterm_term::color::ColorPalette::default()
        }

        fn normalize_output_to_unicode_nfc(&self) -> bool {
            self.normalize
        }
    }

    fn terminal(normalize: bool, conpty: bool) -> onlyterm_term::Terminal {
        let mut term = onlyterm_term::Terminal::new(
            TerminalSize {
                rows: 24,
                cols: 80,
                pixel_width: 640,
                pixel_height: 384,
                dpi: 0,
            },
            Arc::new(Config { normalize }),
            "OnlyTerm",
            "test",
            Box::new(Vec::new()),
        );
        term.set_notification_handler(Box::new(NoAlerts));
        if conpty {
            term.enable_conpty_quirks();
        }
        term
    }

    #[test]
    fn byte_budget_preserves_unicode_attributes_and_cursor() {
        let cases = [
            format!(
                "{}e\u{301}{}🧑‍🤝‍🧑ж",
                "a".repeat(MAX_PRINT_BYTES_PER_CHUNK - 1),
                "b".repeat(MAX_PRINT_BYTES_PER_CHUNK - 2)
            ),
            format!(
                "{}{}\u{600}Z",
                "c".repeat(MAX_PRINT_BYTES_PER_CHUNK - 2),
                "界".repeat(MAX_PRINT_BYTES_PER_CHUNK)
            ),
            format!("x{}", "\u{301}".repeat(MAX_PRINT_BYTES_PER_CHUNK)),
        ];
        for normalize in [false, true] {
            for conpty in [false, true] {
                for text in &cases {
                    let stream = format!(
                        "\x1b[31;44m\x1b]8;;https://example.invalid/budget\x1b\\{}\x1b]8;;\x1b\\\x1b[0m\r\nEND",
                        text
                    );
                    let mut parser = termwiz::escape::parser::Parser::new();
                    let mut actions = Vec::new();
                    parser.parse(stream.as_bytes(), |action| action.append_to(&mut actions));
                    let mut reference = terminal(normalize, conpty);
                    reference.perform_actions(actions.clone());
                    let (pane, _) = super::super::tests::make_pane();
                    *pane.terminal.lock() = terminal(normalize, conpty);
                    pane.perform_actions_chunked(actions, 512);
                    let actual = pane.terminal.lock();
                    let cursor = actual.cursor_pos();
                    let expected_cursor = reference.cursor_pos();
                    assert_eq!(
                        (cursor.x, cursor.y, cursor.shape, cursor.visibility),
                        (
                            expected_cursor.x,
                            expected_cursor.y,
                            expected_cursor.shape,
                            expected_cursor.visibility
                        )
                    );
                    assert_eq!(
                        actual.screen().scrollback_rows(),
                        reference.screen().scrollback_rows()
                    );
                    actual.screen().for_each_phys_line(|row, line| {
                        reference
                            .screen()
                            .with_phys_lines(row..row + 1, |expected| {
                                let expected = expected[0];
                                assert_eq!(line.as_str(), expected.as_str());
                                assert_eq!(line.len(), expected.len());
                                assert_eq!(
                                    line.last_cell_was_wrapped(),
                                    expected.last_cell_was_wrapped()
                                );
                                let observe = |line: &Line| {
                                    line.visible_cells()
                                        .map(|cell| {
                                            (
                                                cell.cell_index(),
                                                cell.str().to_owned(),
                                                cell.width(),
                                                cell.attrs().clone(),
                                            )
                                        })
                                        .collect::<Vec<_>>()
                                };
                                assert_eq!(observe(line), observe(expected));
                            });
                    });
                }
            }
        }
    }

    #[test]
    fn oversized_print_keeps_resize_guard_for_the_batch() {
        let (pane, _) = super::super::tests::make_pane();
        let actions = vec![Action::PrintString(
            "x".repeat(MAX_PRINT_BYTES_PER_CHUNK * 2),
        )];
        pane.with_action_chunks(actions, 512, |chunk| {
            assert!(
                pane.resize_guard.try_lock().is_none(),
                "resize must not enter a split repaint"
            );
            chunk.apply(&mut pane.terminal.lock());
        });
    }

    #[test]
    fn byte_budget_preserves_graphemes_across_action_boundaries() {
        let actions = vec![
            Action::PrintString("a".repeat(MAX_PRINT_BYTES_PER_CHUNK - 1)),
            Action::Print('e'),
            Action::PrintString(format!("\u{301}{}", "b".repeat(MAX_PRINT_BYTES_PER_CHUNK))),
        ];
        let mut reference = terminal(true, false);
        reference.perform_actions(actions.clone());
        let (pane, _) = super::super::tests::make_pane();
        *pane.terminal.lock() = terminal(true, false);
        pane.perform_actions_chunked(actions, 512);
        let actual = pane.terminal.lock();
        assert_eq!(
            actual.screen().scrollback_rows(),
            reference.screen().scrollback_rows()
        );
        actual.screen().for_each_phys_line(|row, line| {
            reference
                .screen()
                .with_phys_lines(row..row + 1, |expected| {
                    assert_eq!(line.as_str(), expected[0].as_str());
                });
        });
    }
}
