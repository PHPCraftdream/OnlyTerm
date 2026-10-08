use crate::gui_api::guiwin::GuiWin;
use crate::overlay::menu_style::{TextMenuStyle, LIST_TOP, ROW_OVERHEAD};
use crate::overlay::quickselect;
use nucleo_matcher::pattern::Pattern;
use nucleo_matcher::{Matcher, Utf32Str};
use onlyterm_config::configuration;
use onlyterm_config::keyassignment::{InputSelector, InputSelectorEntry, KeyAssignment};
use onlyterm_mux::termwiztermtab::TermWizTerminal;
use onlyterm_mux_funcs::MuxPane;
use rayon::prelude::*;
use std::cell::RefCell;
use termwiz::input::{InputEvent, KeyCode, KeyEvent, Modifiers, MouseButtons, MouseEvent};
use termwiz::surface::{Change, Position};
use termwiz::terminal::Terminal;

thread_local! {
    pub static MATCHER: RefCell<Matcher> = RefCell::new(Matcher::new(nucleo_matcher::Config::DEFAULT));
}

pub fn matcher_score(pattern: &Pattern, s: &str) -> Option<u32> {
    MATCHER.with_borrow_mut(|matcher| {
        let mut buf = vec![];
        pattern.score(Utf32Str::new(s, &mut buf), matcher)
    })
}

pub fn matcher_pattern(s: &str) -> Pattern {
    nucleo_matcher::pattern::Pattern::parse(
        s,
        nucleo_matcher::pattern::CaseMatching::Ignore,
        nucleo_matcher::pattern::Normalization::Smart,
    )
}

struct SelectorState {
    active_idx: usize,
    max_items: usize,
    top_row: usize,
    filter_term: String,
    filtered_entries: Vec<InputSelectorEntry>,
    filtering: bool,
    always_fuzzy: bool,
    args: InputSelector,
    selection: String,
    labels: Vec<String>,
}

impl SelectorState {
    fn update_filter(&mut self) {
        if self.filter_term.is_empty() {
            self.filtered_entries = self.args.choices.clone();
            return;
        }

        self.filtered_entries.clear();

        struct MatchResult {
            row_idx: usize,
            score: u32,
        }

        let pattern = matcher_pattern(&self.filter_term);

        let mut scores: Vec<MatchResult> = self
            .args
            .choices
            .par_iter()
            .enumerate()
            .filter_map(|(row_idx, entry)| {
                let score = matcher_score(&pattern, &entry.label)?;
                Some(MatchResult { row_idx, score })
            })
            .collect();

        scores.sort_by(|a, b| a.score.cmp(&b.score).reverse());

        for result in scores {
            self.filtered_entries
                .push(self.args.choices[result.row_idx].clone());
        }

        self.active_idx = 0;
        self.top_row = 0;
    }

    #[allow(clippy::result_large_err)] // returns termwiz::Result; Err (termwiz::Error) is an external 136-byte type, boxing ripples through callers
    fn render(&mut self, term: &mut TermWizTerminal) -> termwiz::Result<()> {
        let size = term.get_screen_size()?;
        let max_width = size.cols.saturating_sub(8);
        let max_items = size.rows.saturating_sub(ROW_OVERHEAD);
        if max_items != self.max_items {
            self.labels = quickselect::compute_labels_for_alphabet_with_preserved_case(
                &self.args.alphabet,
                self.filtered_entries.len().min(max_items + 1),
            );
            self.max_items = max_items;
        }
        let style = TextMenuStyle::new();
        let description = if self.filtering || !self.filter_term.is_empty() {
            format!("{}{}", self.args.fuzzy_description, self.filter_term)
        } else {
            self.args.description.clone()
        };
        let mut changes = style.frame(
            size.cols,
            size.rows,
            if self.args.title.is_empty() {
                "Select an Item"
            } else {
                &self.args.title
            },
            &description,
            "Arrows: select  Enter: accept  /: filter  Esc: cancel",
        );
        let max_label_len = self.labels.iter().map(|s| s.len()).max().unwrap_or(0);
        let config = configuration();
        for (row_num, (entry_idx, entry)) in self
            .filtered_entries
            .iter()
            .enumerate()
            .skip(self.top_row)
            .take(max_items + 1)
            .enumerate()
        {
            let attr = if entry_idx == self.active_idx {
                style.selected.clone()
            } else {
                style.normal.clone()
            };
            changes.push(Change::CursorPosition {
                x: Position::Absolute(3),
                y: Position::Absolute(LIST_TOP + row_num),
            });
            changes.push(Change::AllAttributes(attr.clone()));
            if !self.filtering {
                if let Some(label) = self.labels.get(row_num) {
                    let mut key_attr = if entry_idx == self.active_idx {
                        style.selected.clone()
                    } else {
                        style.label.clone()
                    };
                    if let Some(bg) = config.resolved_palette.input_selector_label_bg {
                        key_attr.set_background(bg);
                    }
                    if let Some(fg) = config.resolved_palette.input_selector_label_fg {
                        key_attr.set_foreground(fg);
                    }
                    changes.push(Change::AllAttributes(key_attr));
                    changes.push(Change::Text(format!(" {label:>max_label_len$} ")));
                    changes.push(Change::AllAttributes(attr.clone()));
                    changes.push(Change::Text("  ".into()));
                } else {
                    changes.push(Change::Text(" ".repeat(max_label_len + 4)));
                }
            }
            let mut line = crate::tabbar::parse_status_text(&entry.label, attr.clone());
            if line.len() > max_width.saturating_sub(max_label_len + 4) {
                line.resize(
                    max_width.saturating_sub(max_label_len + 4),
                    termwiz::surface::SEQ_ZERO,
                );
            }
            changes.append(&mut line.changes(&attr));
            changes.push(Change::AllAttributes(style.normal.clone()));
        }
        changes.extend(style.scrollbar(
            size.cols,
            size.rows,
            self.top_row,
            max_items + 1,
            self.filtered_entries.len(),
        ));
        term.render(&changes)
    }

    fn trigger_event(&self, _entry: Option<InputSelectorEntry>) {
        // The chosen entry (or `None` on cancel) used to be dispatched to a
        // rhai handler registered via `onlyterm.action_callback` under
        // `self.event_name`. With the scripting layer removed there is no
        // handler registry left to receive it, so the result is simply
        // discarded; the caller only needs the fact that selection is
        // finished in order to break out of `run_loop`.
    }

    fn launch(&self, active_idx: usize) -> bool {
        if let Some(entry) = self.filtered_entries.get(active_idx).cloned() {
            self.trigger_event(Some(entry));
            true
        } else {
            false
        }
    }

    fn move_up(&mut self) {
        self.active_idx = self.active_idx.saturating_sub(1);
        if self.active_idx < self.top_row {
            self.top_row = self.active_idx;
        }
    }

    fn move_down(&mut self) {
        self.active_idx = (self.active_idx + 1).min(self.filtered_entries.len() - 1);
        if self.active_idx > self.top_row + self.max_items {
            self.top_row = self.active_idx.saturating_sub(self.max_items);
        }
    }

    fn run_loop(&mut self, term: &mut TermWizTerminal) -> anyhow::Result<()> {
        while let Ok(Some(event)) = term.poll_input(None) {
            match event {
                InputEvent::Key(KeyEvent {
                    key: KeyCode::Char(c),
                    modifiers: Modifiers::NONE,
                }) if !self.filtering && self.args.alphabet.contains(c) => {
                    self.selection.push(c);
                    if let Some(pos) = self.labels.iter().position(|x| *x == self.selection) {
                        // since the number of labels is always <= self.max_items
                        // by construction, we have pos as usize <= self.max_items
                        // for free
                        self.active_idx = self.top_row + pos;
                        if self.launch(self.active_idx) {
                            break;
                        }
                    }
                }
                InputEvent::Key(KeyEvent {
                    key: KeyCode::Char('j'),
                    ..
                }) if !self.filtering => {
                    self.move_down();
                }
                InputEvent::Key(KeyEvent {
                    key: KeyCode::Char('k'),
                    ..
                }) if !self.filtering => {
                    self.move_up();
                }
                InputEvent::Key(KeyEvent {
                    key: KeyCode::Char('P' | 'K'),
                    modifiers: Modifiers::CTRL,
                }) => {
                    self.move_up();
                }
                InputEvent::Key(KeyEvent {
                    key: KeyCode::Char('N' | 'J'),
                    modifiers: Modifiers::CTRL,
                }) => {
                    self.move_down();
                }
                InputEvent::Key(KeyEvent {
                    key: KeyCode::Char('/'),
                    ..
                }) if !self.filtering => {
                    self.filtering = true;
                }
                InputEvent::Key(KeyEvent {
                    key: KeyCode::Backspace,
                    ..
                }) => {
                    if !self.filtering {
                        self.selection.pop();
                    } else {
                        if self.filter_term.pop().is_none() && !self.always_fuzzy {
                            self.filtering = false;
                        }
                        self.update_filter();
                    }
                }
                InputEvent::Key(KeyEvent {
                    key: KeyCode::Char('G' | 'C'),
                    modifiers: Modifiers::CTRL,
                })
                | InputEvent::Key(KeyEvent {
                    key: KeyCode::Escape,
                    ..
                }) => {
                    self.trigger_event(None);
                    break;
                }
                InputEvent::Key(KeyEvent {
                    key: KeyCode::Char(c),
                    ..
                }) if self.filtering => {
                    self.filter_term.push(c);
                    self.update_filter();
                }
                InputEvent::Key(KeyEvent {
                    key: KeyCode::UpArrow,
                    ..
                }) => {
                    self.move_up();
                }
                InputEvent::Key(KeyEvent {
                    key: KeyCode::DownArrow,
                    ..
                }) => {
                    self.move_down();
                }
                InputEvent::Mouse(MouseEvent {
                    y, mouse_buttons, ..
                }) if mouse_buttons.contains(MouseButtons::VERT_WHEEL) => {
                    if mouse_buttons.contains(MouseButtons::WHEEL_POSITIVE) {
                        self.top_row = self.top_row.saturating_sub(1);
                    } else {
                        self.top_row += 1;
                        self.top_row = self.top_row.min(
                            self.filtered_entries
                                .len()
                                .saturating_sub(self.max_items)
                                .saturating_sub(1),
                        );
                    }
                    if y as usize >= LIST_TOP
                        && y as usize - LIST_TOP <= self.max_items
                        && self.top_row + y as usize - LIST_TOP < self.filtered_entries.len()
                    {
                        self.active_idx = self.top_row + y as usize - LIST_TOP;
                    }
                }
                InputEvent::Mouse(MouseEvent {
                    x,
                    y,
                    mouse_buttons,
                    ..
                }) => {
                    let size = term.get_screen_size()?;
                    if x as usize == size.cols.saturating_sub(3)
                        && y as usize >= LIST_TOP
                        && y as usize <= LIST_TOP + self.max_items
                    {
                        if mouse_buttons.contains(MouseButtons::LEFT) {
                            let position = crate::termwindow::menu_style::ScrollPosition {
                                offset: self.top_row,
                                visible: self.max_items + 1,
                                total: self.filtered_entries.len(),
                            };
                            let height = position.visible as f32 * 24.;
                            let (_, thumb) = position.thumb(height);
                            let offset = position.offset_at(
                                (y as usize - LIST_TOP) as f32 * 24. - thumb / 2.,
                                height,
                            );
                            if self.top_row != offset {
                                self.top_row = offset;
                                self.active_idx =
                                    self.active_idx.clamp(offset, offset + self.max_items);
                                self.render(term)?;
                            }
                        }
                        continue;
                    }
                    if y as usize >= LIST_TOP
                        && y as usize - LIST_TOP <= self.max_items
                        && self.top_row + y as usize - LIST_TOP < self.filtered_entries.len()
                    {
                        self.active_idx = self.top_row + y as usize - LIST_TOP;

                        if mouse_buttons == MouseButtons::LEFT && self.launch(self.active_idx) {
                            break;
                        }
                    }
                    if mouse_buttons != MouseButtons::NONE {
                        // Treat any other mouse button as cancel
                        self.trigger_event(None);
                        break;
                    }
                }
                InputEvent::Key(KeyEvent {
                    key: KeyCode::Enter,
                    ..
                }) if self.launch(self.active_idx) => {
                    break;
                }
                _ => {}
            }
            self.render(term)?;
        }

        Ok(())
    }
}

// `window`/`pane` are unused now that the selected entry (or cancellation)
// is no longer dispatched to a rhai handler (see `trigger_event`), but the
// signature is kept stable since `termwindow/mod.rs` passes them uniformly
// alongside the other overlay entry points.
pub fn selector(
    mut term: TermWizTerminal,
    args: InputSelector,
    _window: GuiWin,
    _pane: MuxPane,
) -> anyhow::Result<()> {
    match *args.action {
        KeyAssignment::EmitEvent(_) => {}
        _ => {
            anyhow::bail!("InputSelector requires action to be defined by onlyterm.action_callback")
        }
    };
    let mut state = SelectorState {
        active_idx: 0,
        max_items: 0,
        top_row: 0,
        filter_term: String::new(),
        filtered_entries: vec![],
        filtering: args.fuzzy,
        always_fuzzy: args.fuzzy,
        args,
        selection: String::new(),
        labels: vec![],
    };

    term.set_raw_mode()?;
    term.render(&[Change::Title(state.args.title.to_string())])?;
    state.update_filter();
    state.render(&mut term)?;
    state.run_loop(&mut term)
}
