use super::frecency::Frecency;
use crate::overlay::selector::{matcher_pattern, matcher_score};
use crate::termwindow::box_model::*;
use crate::termwindow::menu_style::MenuStyle;
use crate::termwindow::modal::Modal;
use crate::termwindow::DimensionContext;
use crate::utilsprites::RenderMetrics;
use crate::TermWindow;
use emojis::{Emoji, Group};
use onlyterm_config::keyassignment::{
    CharSelectArguments, CharSelectGroup, ClipboardCopyDestination, KeyAssignment,
};
use onlyterm_config::Dimension;
use onlyterm_term::{KeyCode, KeyModifiers, MouseEvent};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::cell::{Ref, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use termwiz::input::Modifiers;
use window::WindowOps;

struct MatchResults {
    selection: String,
    matches: Vec<usize>,
    group: CharSelectGroup,
}

pub struct CharSelector {
    group: RefCell<CharSelectGroup>,
    element: RefCell<Option<Vec<ComputedElement>>>,
    selection: RefCell<String>,
    aliases: Vec<Alias>,
    matches: RefCell<Option<MatchResults>>,
    selected_row: RefCell<usize>,
    top_row: RefCell<usize>,
    max_rows_on_screen: RefCell<usize>,
    copy_on_select: bool,
    copy_to: ClipboardCopyDestination,
}

enum Move {
    Up(usize),
    Down(usize),
    PageUp,
    PageDown,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum Character {
    Unicode { name: &'static str, value: char },
    Emoji(&'static Emoji),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Alias {
    name: Cow<'static, str>,
    character: Character,
    group: CharSelectGroup,
}

impl Alias {
    fn name(&self) -> &str {
        &self.name
    }

    fn glyph(&self) -> String {
        match &self.character {
            Character::Unicode { value, .. } => value.to_string(),
            Character::Emoji(emoji) => emoji.as_str().to_string(),
        }
    }

    fn codepoints(&self) -> String {
        let mut res = String::new();
        for c in self.glyph().chars() {
            if !res.is_empty() {
                res.push(' ');
            }
            res.push_str(&format!("U+{:X}", c as u32));
        }
        res
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Recent {
    glyph: String,
    name: String,
    frecency: Frecency,
}

fn recent_file_name() -> PathBuf {
    onlyterm_config::DATA_DIR.join("recent-emoji.json")
}

fn load_recents() -> anyhow::Result<Vec<Recent>> {
    let file_name = recent_file_name();
    let f = std::fs::File::open(&file_name)?;
    let mut recents: Vec<Recent> = serde_json::from_reader(f)?;
    recents.sort_by(|a, b| b.frecency.score().partial_cmp(&a.frecency.score()).unwrap());
    Ok(recents)
}

fn save_recent(alias: &Alias) -> anyhow::Result<()> {
    let mut recents = load_recents().unwrap_or_else(|_| vec![]);
    let glyph = alias.glyph();
    if let Some(recent_idx) = recents.iter().position(|r| r.glyph == glyph) {
        let recent = recents.get_mut(recent_idx).unwrap();
        recent.frecency.register_access();
    } else {
        let mut frecency = Frecency::new();
        frecency.register_access();
        recents.push(Recent {
            glyph,
            name: alias.name().to_string(),
            frecency,
        });
    }

    let json = serde_json::to_string(&recents)?;
    let file_name = recent_file_name();
    std::fs::write(&file_name, json)?;
    Ok(())
}

fn build_aliases() -> Vec<Alias> {
    let mut aliases = vec![];
    let start = std::time::Instant::now();

    fn push(aliases: &mut Vec<Alias>, alias: Alias) {
        aliases.push(alias);
    }

    if let Ok(recents) = load_recents() {
        for r in recents {
            let character = if let Some(emoji) = emojis::get(&r.glyph) {
                Character::Emoji(emoji)
            } else {
                Character::Unicode {
                    name: "",
                    value: r.glyph.chars().next().unwrap(),
                }
            };

            aliases.push(Alias {
                name: Cow::Owned(r.name.clone()),
                character,
                group: CharSelectGroup::RecentlyUsed,
            });
        }
    }

    for emoji in emojis::iter() {
        let group = match emoji.group() {
            Group::SmileysAndEmotion => CharSelectGroup::SmileysAndEmotion,
            Group::PeopleAndBody => CharSelectGroup::PeopleAndBody,
            Group::AnimalsAndNature => CharSelectGroup::AnimalsAndNature,
            Group::FoodAndDrink => CharSelectGroup::FoodAndDrink,
            Group::TravelAndPlaces => CharSelectGroup::TravelAndPlaces,
            Group::Activities => CharSelectGroup::Activities,
            Group::Objects => CharSelectGroup::Objects,
            Group::Symbols => CharSelectGroup::Symbols,
            Group::Flags => CharSelectGroup::Flags,
        };
        match emoji.skin_tones() {
            Some(iter) => {
                for entry in iter {
                    push(
                        &mut aliases,
                        Alias {
                            name: Cow::Borrowed(entry.name()),
                            character: Character::Emoji(entry),
                            group,
                        },
                    );
                }
            }
            None => {
                push(
                    &mut aliases,
                    Alias {
                        name: Cow::Borrowed(emoji.name()),
                        character: Character::Emoji(emoji),
                        group,
                    },
                );
            }
        }
        for short in emoji.shortcodes() {
            push(
                &mut aliases,
                Alias {
                    name: Cow::Borrowed(short),
                    character: Character::Emoji(emoji),
                    group: CharSelectGroup::ShortCodes,
                },
            );
        }
    }

    for (name, value) in termwiz::nerdfonts::NERD_FONT_GLYPHS {
        push(
            &mut aliases,
            Alias {
                name: Cow::Borrowed(name),
                character: Character::Unicode {
                    name,
                    value: *value,
                },
                group: CharSelectGroup::NerdFonts,
            },
        );
    }

    log::trace!(
        "Took {:?} to build {} aliases",
        start.elapsed(),
        aliases.len()
    );

    aliases
}

#[derive(Debug, Copy, Clone)]
struct MatchResult {
    row_idx: usize,
    score: u32,
}

impl MatchResult {
    fn new(row_idx: usize, score: u32, selection: &str, aliases: &[Alias]) -> Self {
        Self {
            row_idx,
            score: if aliases[row_idx].name == selection {
                // Pump up the score for an exact match, otherwise
                // the order may be undesirable if there are a lot
                // of candidates with the same score
                u32::MAX
            } else {
                score
            },
        }
    }
}

fn compute_matches(selection: &str, aliases: &[Alias], group: CharSelectGroup) -> Vec<usize> {
    if selection.is_empty() {
        aliases
            .iter()
            .enumerate()
            .filter(|(_idx, a)| a.group == group)
            .map(|(idx, _a)| idx)
            .collect()
    } else {
        let pattern = matcher_pattern(selection);

        let numeric_selection = if selection.chars().all(|c| c.is_ascii_hexdigit()) {
            // Make this uppercase so that eg: `e1` matches `U+E1` rather
            // than HENTAIGANA LETTER E-1.
            // <https://github.com/wezterm/wezterm/issues/2581#issuecomment-1267662040>
            Some(format!("U+{}", selection.to_ascii_uppercase()))
        } else if selection.starts_with("U+") {
            Some(selection.to_string())
        } else {
            None
        };
        let start = std::time::Instant::now();

        let all_matches: Vec<(String, MatchResult)> = aliases
            .par_iter()
            .enumerate()
            .filter_map(|(row_idx, entry)| {
                let glyph = entry.glyph();

                let alias_result = matcher_score(&pattern, &entry.name)
                    .map(|score| MatchResult::new(row_idx, score, selection, aliases));

                match &numeric_selection {
                    Some(sel) => {
                        let codepoints = entry.codepoints();
                        if codepoints == *sel {
                            Some((
                                glyph,
                                MatchResult {
                                    row_idx,
                                    score: u32::MAX,
                                },
                            ))
                        } else {
                            let number_result = matcher_score(&pattern, &codepoints)
                                .map(|score| MatchResult::new(row_idx, score, selection, aliases));

                            match (alias_result, number_result) {
                                (
                                    Some(MatchResult { score: a, .. }),
                                    Some(MatchResult { score: b, .. }),
                                ) => Some((
                                    glyph,
                                    MatchResult {
                                        row_idx,
                                        score: a.max(b),
                                    },
                                )),
                                (Some(a), None) | (None, Some(a)) => Some((glyph, a)),
                                (None, None) => None,
                            }
                        }
                    }
                    None => alias_result.map(|a| (glyph, a)),
                }
            })
            .collect();

        let mut matches = HashMap::<String, MatchResult>::new();
        for (glyph, value) in all_matches {
            let entry = matches.entry(glyph).or_insert(value);
            // Retain the best scoring match for a given glyph
            if entry.score < value.score {
                *entry = value;
            }
        }
        let mut scores: Vec<MatchResult> = matches.into_values().collect();
        scores.sort_by(|a, b| a.score.cmp(&b.score).reverse());
        log::trace!(
            "matching took {:?} for {} entries",
            start.elapsed(),
            scores.len()
        );

        scores.iter().map(|result| result.row_idx).collect()
    }
}

impl CharSelector {
    pub fn new(_term_window: &mut TermWindow, args: &CharSelectArguments) -> Self {
        let aliases = build_aliases();
        let has_recents = aliases[0].group == CharSelectGroup::RecentlyUsed;
        let group = args.group.unwrap_or_else(|| {
            if has_recents {
                CharSelectGroup::RecentlyUsed
            } else {
                CharSelectGroup::default()
            }
        });

        Self {
            element: RefCell::new(None),
            selection: RefCell::new(String::new()),
            group: RefCell::new(group),
            aliases,
            matches: RefCell::new(None),
            selected_row: RefCell::new(0),
            top_row: RefCell::new(0),
            max_rows_on_screen: RefCell::new(0),
            copy_on_select: args.copy_on_select,
            copy_to: args.copy_to,
        }
    }

    #[allow(clippy::too_many_arguments)] // overlay layout: params are the inherent element/grid context
    fn compute(
        term_window: &mut TermWindow,
        selection: &str,
        group: CharSelectGroup,
        aliases: &[Alias],
        matches: &MatchResults,
        max_rows_on_screen: usize,
        selected_row: usize,
        top_row: usize,
    ) -> anyhow::Result<Vec<ComputedElement>> {
        let font = term_window.fonts.char_select_font()?;
        let heading = term_window.fonts.title_font()?;
        let ui_font = term_window.fonts.command_palette_font()?;
        let metrics = RenderMetrics::with_font_metrics(&font.metrics());
        let style = MenuStyle::new(
            term_window.config.char_select_bg_color,
            term_window.config.char_select_fg_color,
        );
        let dimensions = term_window.dimensions;
        let width_limit = dimensions.pixel_width as f32;
        let height_limit = dimensions.pixel_height as f32;
        let width = (width_limit - 32.).clamp(1., 1000.);
        let label = match group {
            CharSelectGroup::RecentlyUsed => "Recent",
            CharSelectGroup::SmileysAndEmotion => "Emotion",
            CharSelectGroup::PeopleAndBody => "People",
            CharSelectGroup::AnimalsAndNature => "Animals",
            CharSelectGroup::FoodAndDrink => "Food",
            CharSelectGroup::TravelAndPlaces => "Travel",
            CharSelectGroup::Activities => "Activities",
            CharSelectGroup::Objects => "Objects",
            CharSelectGroup::Symbols => "Symbols",
            CharSelectGroup::Flags => "Flags",
            CharSelectGroup::NerdFonts => "Nerd Fonts",
            CharSelectGroup::ShortCodes => "Short Codes",
        };
        let mut choices = vec![];
        for (display_idx, &index) in matches
            .matches
            .iter()
            .enumerate()
            .skip(top_row)
            .take(max_rows_on_screen)
        {
            let alias = &aliases[index];
            let description = Element::new(
                &font,
                ElementContent::Children(vec![
                    style.text(&heading, alias.name().to_string(), false),
                    style.text(&heading, alias.codepoints(), true),
                ]),
            )
            .display(DisplayType::Inline)
            .margin(BoxDimension {
                left: Dimension::Pixels(12.),
                ..BoxDimension::default()
            });
            choices.push(
                Element::new(
                    &font,
                    ElementContent::Children(vec![style.keycap(&font, alias.glyph()), description]),
                )
                .display(DisplayType::Block)
                .colors(if display_idx == selected_row {
                    style.focus.clone()
                } else {
                    style.card.clone()
                })
                .padding(BoxDimension::new(Dimension::Pixels(6.)))
                .border(BoxDimension::new(Dimension::Pixels(1.)))
                .border_corners(Some(crate::termwindow::menu_style::corners()))
                .min_width(Some(Dimension::Pixels((width - 68.).max(1.))))
                .max_width(Some(Dimension::Pixels((width - 54.).max(1.))))
                .margin(BoxDimension {
                    bottom: Dimension::Pixels(4.),
                    ..BoxDimension::default()
                })
                .min_height(Some(Dimension::Pixels(
                    metrics.cell_size.height as f32 * 2. + 8.,
                ))),
            );
        }
        if choices.is_empty() {
            choices.push(style.text(&ui_font, "No matching characters".into(), true));
        }
        let root = style.panel(
            &ui_font,
            vec![
                style.text(&ui_font, "Character Picker".into(), false),
                style.card(
                    &ui_font,
                    vec![
                        style.text(&heading, format!("GROUP · {label}"), false),
                        style.text(&ui_font, format!("{selection}│"), false),
                    ],
                    (width - 34.).max(1.),
                ),
                style.viewport(
                    &font,
                    choices,
                    (width - 34.).max(1.),
                    max_rows_on_screen as f32 * (metrics.cell_size.height as f32 * 2. + 26.),
                    crate::termwindow::menu_style::ScrollPosition {
                        offset: top_row,
                        visible: max_rows_on_screen,
                        total: matches.matches.len(),
                    },
                ),
                style.text(
                    &ui_font,
                    "Type to filter · Ctrl+R: group · Arrows / Enter · Esc".into(),
                    true,
                ),
            ],
            width,
        );
        let mut computed = term_window.compute_element(
            &LayoutContext {
                height: DimensionContext {
                    dpi: dimensions.dpi as f32,
                    pixel_max: height_limit,
                    pixel_cell: metrics.cell_size.height as f32,
                },
                width: DimensionContext {
                    dpi: dimensions.dpi as f32,
                    pixel_max: width_limit,
                    pixel_cell: metrics.cell_size.width as f32,
                },
                bounds: euclid::rect((width_limit - width) / 2., 0., width, height_limit),
                metrics: &metrics,
                gl_state: term_window.render_state.as_ref().unwrap(),
                zindex: 100,
            },
            &root,
        )?;
        computed.translate(euclid::vec2(
            0.,
            ((height_limit - computed.bounds.height()) / 2.).max(0.),
        ));
        Ok(vec![computed])
    }

    fn updated_input(&self) {
        *self.selected_row.borrow_mut() = 0;
        *self.top_row.borrow_mut() = 0;
    }

    fn do_move(&self, how: Move) {
        let page_size = *self.max_rows_on_screen.borrow();
        let current_row = *self.selected_row.borrow();
        let dest = match how {
            Move::Up(n) => current_row.saturating_sub(n),
            Move::PageUp => current_row.saturating_sub(page_size),
            Move::Down(n) => current_row.saturating_add(n),
            Move::PageDown => current_row.saturating_add(page_size),
        };
        *self.selected_row.borrow_mut() = dest;
        self.nav_selection();
    }

    /// handles selection constraints, moving list, keeping selection centered
    fn nav_selection(&self) {
        let max_rows_on_screen = *self.max_rows_on_screen.borrow();
        let limit = self
            .matches
            .borrow()
            .as_ref()
            .map(|m| m.matches.len())
            .unwrap_or_else(|| self.aliases.len());
        {
            let mut row = self.selected_row.borrow_mut();
            let mut top_row = self.top_row.borrow_mut();
            *row = row.min(limit.saturating_sub(1));
            if *row < *top_row {
                *top_row = *row;
            }
            if *row + *top_row > max_rows_on_screen / 2 {
                *top_row = row.saturating_sub(max_rows_on_screen / 2);
            }
        }
    }
}

impl Modal for CharSelector {
    fn blocks_terminal_input(&self) -> bool {
        true
    }

    fn scroll_position(&self) -> Option<crate::termwindow::menu_style::ScrollPosition> {
        Some(crate::termwindow::menu_style::ScrollPosition {
            offset: *self.top_row.borrow(),
            visible: *self.max_rows_on_screen.borrow(),
            total: self
                .matches
                .borrow()
                .as_ref()
                .map_or(0, |m| m.matches.len()),
        })
    }

    fn set_scroll_offset(&self, offset: usize, window: &mut TermWindow) {
        let position = self.scroll_position().unwrap();
        let offset = offset.min(position.limit());
        *self.top_row.borrow_mut() = offset;
        let selected = (*self.selected_row.borrow()).clamp(
            offset,
            offset.saturating_add(position.visible.saturating_sub(1)),
        );
        *self.selected_row.borrow_mut() = selected;
        self.element.borrow_mut().take();
        if let Some(window) = window.window.as_ref() {
            window.invalidate();
        }
    }

    fn perform_assignment(
        &self,
        _assignment: &KeyAssignment,
        _term_window: &mut TermWindow,
    ) -> bool {
        false
    }

    fn mouse_event(&self, _event: MouseEvent, _term_window: &mut TermWindow) -> anyhow::Result<()> {
        Ok(())
    }

    fn key_down(
        &self,
        key: KeyCode,
        mods: KeyModifiers,
        term_window: &mut TermWindow,
    ) -> anyhow::Result<bool> {
        const CTRL_AND_SHIFT: Modifiers = KeyModifiers::CTRL.union(KeyModifiers::SHIFT);

        match (key, mods) {
            (KeyCode::Escape, KeyModifiers::NONE) | (KeyCode::Char('g'), KeyModifiers::CTRL) => {
                term_window.cancel_modal();
            }
            (KeyCode::Char('r'), KeyModifiers::CTRL) => {
                // Cycle the selected group
                let mut group = self.group.borrow_mut();
                *group = group.next();
                self.selection.borrow_mut().clear();
                self.updated_input();
            }
            (KeyCode::Char('R'), KeyModifiers::CTRL) | (KeyCode::Char('r'), CTRL_AND_SHIFT) => {
                // Cycle the selected group in reverse direction
                let mut group = self.group.borrow_mut();
                *group = group.previous();
                self.selection.borrow_mut().clear();
                self.updated_input();
            }
            (KeyCode::PageUp, KeyModifiers::NONE) => {
                self.do_move(Move::PageUp);
            }
            (KeyCode::PageDown, KeyModifiers::NONE) => {
                self.do_move(Move::PageDown);
            }
            (KeyCode::UpArrow, KeyModifiers::NONE) => {
                self.do_move(Move::Up(1));
            }
            (KeyCode::DownArrow, KeyModifiers::NONE) => {
                self.do_move(Move::Down(1));
            }
            (KeyCode::Char(c), KeyModifiers::NONE) | (KeyCode::Char(c), KeyModifiers::SHIFT) => {
                // Type to add to the selection
                let mut selection = self.selection.borrow_mut();
                selection.push(c);
                self.updated_input();
            }
            (KeyCode::Backspace, KeyModifiers::NONE) => {
                // Backspace to edit the selection
                let mut selection = self.selection.borrow_mut();
                selection.pop();
                self.updated_input();
            }
            (KeyCode::Char('u'), KeyModifiers::CTRL) => {
                // CTRL-u to clear the selection
                let mut selection = self.selection.borrow_mut();
                selection.clear();
                self.updated_input();
            }
            (KeyCode::Enter, KeyModifiers::NONE) => {
                // Enter the selected character to the current pane
                let selected_idx = *self.selected_row.borrow();
                let alias_idx = match self.matches.borrow().as_ref() {
                    None => return Ok(true),
                    Some(results) => match results.matches.get(selected_idx) {
                        Some(i) => *i,
                        None => return Ok(true),
                    },
                };
                let item = &self.aliases[alias_idx];
                if let Err(err) = save_recent(item) {
                    log::error!("Error while saving recents: {err:#}");
                }
                let glyph = item.glyph();
                log::trace!(
                    "selected: {glyph}. copy_on_select={} -> {:?}",
                    self.copy_on_select,
                    self.copy_to
                );

                if self.copy_on_select {
                    term_window.copy_to_clipboard(self.copy_to, glyph.clone());
                }
                if let Some(pane) = term_window.get_active_pane_or_overlay() {
                    pane.writer().write_all(glyph.as_bytes()).ok();
                }
                term_window.cancel_modal();
                return Ok(true);
            }
            _ => return Ok(false),
        }
        term_window.invalidate_modal();
        Ok(true)
    }

    fn computed_element(
        &self,
        term_window: &mut TermWindow,
    ) -> anyhow::Result<Ref<'_, [ComputedElement]>> {
        let selection = self.selection.borrow();
        let selection = selection.as_str();

        let group = *self.group.borrow();

        let mut results = self.matches.borrow_mut();

        let font = term_window
            .fonts
            .char_select_font()
            .expect("to resolve char selection font");
        let metrics = RenderMetrics::with_font_metrics(&font.metrics());

        let max_rows_on_screen = (term_window.dimensions.pixel_height.saturating_sub(200)
            / (metrics.cell_size.height as usize * 2 + 26))
            .max(1);
        *self.max_rows_on_screen.borrow_mut() = max_rows_on_screen;

        let rebuild_matches = results
            .as_ref()
            .map(|m| m.selection != selection || m.group != group)
            .unwrap_or(true);
        if rebuild_matches {
            results.replace(MatchResults {
                selection: selection.to_string(),
                matches: compute_matches(selection, &self.aliases, group),
                group,
            });
        };
        let matches = results.as_ref().unwrap();

        if self.element.borrow().is_none() {
            let element = Self::compute(
                term_window,
                selection,
                group,
                &self.aliases,
                matches,
                max_rows_on_screen,
                *self.selected_row.borrow(),
                *self.top_row.borrow(),
            )?;
            self.element.borrow_mut().replace(element);
        }
        Ok(Ref::map(self.element.borrow(), |v| {
            v.as_ref().unwrap().as_slice()
        }))
    }

    fn reconfigure(&self, _term_window: &mut TermWindow) {
        self.element.borrow_mut().take();
    }
}
