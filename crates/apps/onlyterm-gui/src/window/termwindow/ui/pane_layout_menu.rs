use crate::termwindow::box_model::*;
use crate::termwindow::menu_style::MenuStyle;
use crate::termwindow::modal::Modal;
use crate::termwindow::{DimensionContext, TermWindow, UIItemType};
use crate::utilsprites::RenderMetrics;
use onlyterm_config::Dimension;
use onlyterm_mux::Mux;
use onlyterm_term::{KeyCode, KeyModifiers, MouseEvent};
use std::cell::{Ref, RefCell};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PaneLayoutChoice {
    SplitHorizontal,
    SplitVertical,
    SplitThreeHorizontal,
    SplitThreeVertical,
    ClosePane,
    CloseMenu,
}

impl PaneLayoutChoice {
    const ALL: [Self; 6] = [
        Self::SplitHorizontal,
        Self::SplitVertical,
        Self::SplitThreeHorizontal,
        Self::SplitThreeVertical,
        Self::ClosePane,
        Self::CloseMenu,
    ];

    fn next(self, direction: i64, can_close: bool) -> Self {
        let mut index = Self::ALL.iter().position(|choice| *choice == self).unwrap();
        loop {
            index = if direction < 0 {
                index.checked_sub(1).unwrap_or(Self::ALL.len() - 1)
            } else {
                (index + 1) % Self::ALL.len()
            };
            let choice = Self::ALL[index];
            if choice != Self::ClosePane || can_close {
                return choice;
            }
        }
    }

    pub(crate) fn from_number(number: u8) -> Option<Self> {
        match number {
            1 => Some(Self::SplitHorizontal),
            2 => Some(Self::SplitVertical),
            3 => Some(Self::SplitThreeHorizontal),
            4 => Some(Self::SplitThreeVertical),
            5 => Some(Self::ClosePane),
            6 => Some(Self::CloseMenu),
            _ => None,
        }
    }

    pub(crate) fn from_key(key: KeyCode) -> Option<Self> {
        let number = match key {
            KeyCode::Char('1') | KeyCode::Numpad1 => 1,
            KeyCode::Char('2') | KeyCode::Numpad2 => 2,
            KeyCode::Char('3') | KeyCode::Numpad3 => 3,
            KeyCode::Char('4') | KeyCode::Numpad4 => 4,
            KeyCode::Char('5') | KeyCode::Numpad5 => 5,
            KeyCode::Char('6') | KeyCode::Numpad6 => 6,
            _ => return None,
        };
        Self::from_number(number)
    }
}

pub(crate) fn can_close_pane(pane_count: usize) -> bool {
    pane_count > 1
}

pub(crate) struct PaneLayoutMenu {
    element: RefCell<Option<Vec<ComputedElement>>>,
    selected: RefCell<PaneLayoutChoice>,
}

impl PaneLayoutMenu {
    pub(crate) fn new() -> Self {
        Self {
            element: RefCell::new(None),
            selected: RefCell::new(PaneLayoutChoice::SplitHorizontal),
        }
    }

    fn compute(&self, term_window: &mut TermWindow) -> anyhow::Result<Vec<ComputedElement>> {
        let font = term_window.fonts.command_palette_font()?;
        let heading = term_window.fonts.title_font()?;
        let metrics = RenderMetrics::with_font_metrics(&font.metrics());
        let dimensions = &term_window.dimensions;
        let style = MenuStyle::new(
            term_window.config.command_palette_bg_color,
            term_window.config.command_palette_fg_color,
        );
        let can_close = Mux::get()
            .get_active_tab_for_window(term_window.mux_window_id)
            .is_some_and(|tab| can_close_pane(tab.iter_panes_ignoring_zoom().len()));
        if *self.selected.borrow() == PaneLayoutChoice::ClosePane && !can_close {
            self.selected.replace(PaneLayoutChoice::CloseMenu);
        }
        let width_limit = dimensions.pixel_width as f32;
        let height_limit = dimensions.pixel_height as f32;
        let width = (60. * term_window.render_metrics.cell_size.width as f32)
            .min(width_limit - 32.)
            .max(1.);
        let inner_width = (width - 34.).max(1.);
        let mut choices = vec![];
        for (index, label) in [
            "Split left / right",
            "Split top / bottom",
            "Split into three columns",
            "Split into three rows",
            "Close active pane",
        ]
        .iter()
        .enumerate()
        {
            let number = (index + 1) as u8;
            let enabled = number != 5 || can_close;
            let label = if enabled {
                (*label).to_string()
            } else {
                format!("{label} (only pane)")
            };
            let description = style
                .text(&font, label, !enabled)
                .display(DisplayType::Inline)
                .margin(BoxDimension {
                    left: Dimension::Pixels(10.),
                    ..BoxDimension::default()
                });
            let mut row = Element::new(
                &font,
                ElementContent::Children(vec![
                    style.keycap(&font, number.to_string()),
                    description,
                ]),
            )
            .display(DisplayType::Block)
            .padding(BoxDimension::new(Dimension::Pixels(6.)))
            .border(BoxDimension::new(Dimension::Pixels(1.)))
            .border_corners(Some(crate::termwindow::menu_style::corners()))
            .min_width(Some(Dimension::Pixels((inner_width - 36.).max(1.))))
            .max_width(Some(Dimension::Pixels((inner_width - 22.).max(1.))))
            .colors(
                if enabled && PaneLayoutChoice::ALL[index] == *self.selected.borrow() {
                    style.focus.clone()
                } else {
                    style.card.clone()
                },
            )
            .margin(BoxDimension {
                bottom: Dimension::Pixels(6.),
                ..BoxDimension::default()
            });
            if enabled {
                row.item_type = Some(UIItemType::PaneLayoutMenuItem(number));
                row.hover_colors = Some(style.focus.clone());
            }
            choices.push(row);
        }
        let close = style
            .button(
                &font,
                "6 · Close",
                *self.selected.borrow() == PaneLayoutChoice::CloseMenu,
                true,
            )
            .item_type(UIItemType::PaneLayoutMenuItem(6));
        let root = style.panel(
            &font,
            vec![
                style.text(&font, "Pane Layout".into(), false),
                style.text(&font, "Split the active pane or close a pane".into(), true),
                style.card(
                    &font,
                    vec![
                        style.text(&heading, "PANE ACTIONS".into(), false),
                        Element::new(&font, ElementContent::Children(choices))
                            .display(DisplayType::Block),
                    ],
                    inner_width,
                ),
                Element::new(&font, ElementContent::Children(vec![close]))
                    .display(DisplayType::Block),
                style.text(&font, "Arrows / 1–6 · Enter · Esc / F3".into(), true),
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
}

impl Modal for PaneLayoutMenu {
    fn blocks_terminal_input(&self) -> bool {
        true
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
        match (key, mods) {
            (KeyCode::Escape, _) | (KeyCode::Function(3), KeyModifiers::NONE) => {
                term_window.cancel_modal();
            }
            (KeyCode::UpArrow | KeyCode::LeftArrow, KeyModifiers::NONE)
            | (KeyCode::DownArrow | KeyCode::RightArrow, KeyModifiers::NONE) => {
                let can_close = Mux::get()
                    .get_active_tab_for_window(term_window.mux_window_id)
                    .is_some_and(|tab| can_close_pane(tab.iter_panes_ignoring_zoom().len()));
                let direction = if matches!(key, KeyCode::UpArrow | KeyCode::LeftArrow) {
                    -1
                } else {
                    1
                };
                let next = self.selected.borrow().next(direction, can_close);
                self.selected.replace(next);
                term_window.invalidate_modal();
            }
            (KeyCode::Enter | KeyCode::Char(' '), KeyModifiers::NONE) => {
                let choice = *self.selected.borrow();
                term_window.perform_pane_layout_choice(choice);
            }
            (key, KeyModifiers::NONE) => {
                if let Some(choice) = PaneLayoutChoice::from_key(key) {
                    term_window.perform_pane_layout_choice(choice);
                }
            }
            _ => {}
        }
        Ok(true)
    }

    fn computed_element(
        &self,
        term_window: &mut TermWindow,
    ) -> anyhow::Result<Ref<'_, [ComputedElement]>> {
        if self.element.borrow().is_none() {
            let element = self.compute(term_window)?;
            self.element.borrow_mut().replace(element);
        }
        Ok(Ref::map(self.element.borrow(), |element| {
            element.as_ref().unwrap().as_slice()
        }))
    }

    fn reconfigure(&self, _term_window: &mut TermWindow) {
        self.element.borrow_mut().take();
    }
}

#[cfg(test)]
mod tests {
    use super::{can_close_pane, PaneLayoutChoice as Choice};
    use onlyterm_term::KeyCode;

    #[test]
    fn digits_and_numpad_select_the_same_six_actions() {
        for (number, digit, numpad, choice) in [
            (1, '1', KeyCode::Numpad1, Choice::SplitHorizontal),
            (2, '2', KeyCode::Numpad2, Choice::SplitVertical),
            (3, '3', KeyCode::Numpad3, Choice::SplitThreeHorizontal),
            (4, '4', KeyCode::Numpad4, Choice::SplitThreeVertical),
            (5, '5', KeyCode::Numpad5, Choice::ClosePane),
            (6, '6', KeyCode::Numpad6, Choice::CloseMenu),
        ] {
            assert_eq!(Choice::from_number(number), Some(choice));
            assert_eq!(Choice::from_key(KeyCode::Char(digit)), Some(choice));
            assert_eq!(Choice::from_key(numpad), Some(choice));
        }
        assert_eq!(Choice::from_number(7), None);
        assert_eq!(Choice::from_key(KeyCode::Char('7')), None);
        assert_eq!(Choice::from_key(KeyCode::Function(4)), None);
    }

    #[test]
    fn navigation_wraps_and_skips_disabled_close() {
        assert_eq!(Choice::SplitHorizontal.next(-1, false), Choice::CloseMenu);
        assert_eq!(Choice::CloseMenu.next(1, false), Choice::SplitHorizontal);
        assert_eq!(Choice::SplitThreeVertical.next(1, false), Choice::CloseMenu);
        assert_eq!(
            Choice::CloseMenu.next(-1, false),
            Choice::SplitThreeVertical
        );
        assert_eq!(Choice::SplitThreeVertical.next(1, true), Choice::ClosePane);
        assert_eq!(Choice::CloseMenu.next(-1, true), Choice::ClosePane);
    }

    #[test]
    fn close_action_requires_another_pane_in_the_tab() {
        assert!(!can_close_pane(0));
        assert!(!can_close_pane(1));
        assert!(can_close_pane(2));
        assert!(can_close_pane(3));
    }
}
