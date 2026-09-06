use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

struct CountAlerts(Arc<AtomicUsize>);

impl AlertHandler for CountAlerts {
    fn alert(&mut self, _: Alert) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

#[test]
fn application_title_sequences_are_ignored() {
    let mut term = TestTerm::new(2, 8, 0);
    let alerts = Arc::new(AtomicUsize::new(0));
    term.term
        .set_notification_handler(Box::new(CountAlerts(Arc::clone(&alerts))));

    term.print(b"\x1b]0;both\x1b\\\x1b]1;icon\x07\x1b]2;window\x1b\\");

    std::assert_eq!(term.term.get_title(), crate::DEFAULT_TERMINAL_TITLE);
    std::assert_eq!(alerts.load(Ordering::Relaxed), 0);
}

#[test]
fn application_title_sequences_work_when_enabled() {
    let mut term = Terminal::new(
        TerminalSize {
            rows: 2,
            cols: 8,
            pixel_width: 64,
            pixel_height: 32,
            dpi: 0,
        },
        Arc::new(TestTermConfig {
            scrollback: 0,
            allow_process_title_updates: true,
        }),
        "OnlyTerm",
        "test",
        Box::new(Vec::new()),
    );

    term.advance_bytes(b"\x1b]2;from-process\x1b\\");

    std::assert_eq!(term.get_title(), "from-process");
}

#[test]
fn tmux_title_sequence_is_discarded_without_reaching_the_screen() {
    let mut term = TestTerm::new(2, 8, 0);
    term.print(b"\x1bkignored title\x1b\\visible");

    std::assert_eq!(term.term.get_title(), crate::DEFAULT_TERMINAL_TITLE);
    std::assert_eq!(
        term.term.screen().lines_in_phys_range(0..1)[0].as_str(),
        "visible"
    );
}
