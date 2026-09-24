use crate::termwindow::box_model::*;
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
}

impl PaneLayoutMenu {
    pub(crate) fn new() -> Self {
        Self {
            element: RefCell::new(None),
        }
    }

    fn compute(term_window: &mut TermWindow) -> anyhow::Result<Vec<ComputedElement>> {
        let font = term_window.fonts.command_palette_font()?;
        let metrics = RenderMetrics::with_font_metrics(&font.metrics());
        let dimensions = &term_window.dimensions;
        let bg = term_window.config.command_palette_bg_color.to_linear();
        let fg = term_window.config.command_palette_fg_color.to_linear();
        let colors = ElementColors {
            border: BorderColor::new(bg),
            bg: bg.into(),
            text: fg.into(),
        };
        let can_close = Mux::get()
            .get_active_tab_for_window(term_window.mux_window_id)
            .is_some_and(|tab| can_close_pane(tab.iter_panes_ignoring_zoom().len()));

        let mut rows = vec![
            Element::new(&font, ElementContent::Text("Pane Layout".to_string()))
                .colors(colors.clone())
                .display(DisplayType::Block)
                .padding(BoxDimension::new(Dimension::Cells(0.5))),
        ];
        for (index, label) in [
            "Split left / right",
            "Split top / bottom",
            "Split into three columns",
            "Split into three rows",
            "Close active pane",
            "Close menu",
        ]
        .iter()
        .enumerate()
        {
            let number = (index + 1) as u8;
            let enabled = number != 5 || can_close;
            let suffix = if enabled { "" } else { " (only pane)" };
            let mut row = Element::new(
                &font,
                ElementContent::Text(format!("{number}. {label}{suffix}")),
            )
            .colors(colors.clone())
            .display(DisplayType::Block)
            .padding(BoxDimension {
                left: Dimension::Cells(0.5),
                right: Dimension::Cells(0.5),
                top: Dimension::Cells(0.2),
                bottom: Dimension::Cells(0.2),
            });
            if enabled {
                row.item_type = Some(UIItemType::PaneLayoutMenuItem(number));
            }
            rows.push(row);
        }
        rows.push(
            Element::new(
                &font,
                ElementContent::Text("Esc or F3 to close".to_string()),
            )
            .colors(colors.clone())
            .display(DisplayType::Block)
            .padding(BoxDimension::new(Dimension::Cells(0.5))),
        );

        let width_limit = dimensions.pixel_width as f32;
        let height_limit = dimensions.pixel_height as f32;
        let width = (54.0 * term_window.render_metrics.cell_size.width as f32)
            .min(width_limit)
            .max(1.0);
        let top = ((height_limit - 10.0 * metrics.cell_size.height as f32) / 2.0).max(0.0);
        let root = Element::new(&font, ElementContent::Children(rows))
            .colors(colors)
            .padding(BoxDimension::new(Dimension::Cells(0.25)))
            .border(BoxDimension::new(Dimension::Pixels(1.0)))
            .min_width(Some(Dimension::Pixels(width)));
        let computed = term_window.compute_element(
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
                bounds: euclid::rect((width_limit - width) / 2.0, top, width, height_limit - top),
                metrics: &metrics,
                gl_state: term_window.render_state.as_ref().unwrap(),
                zindex: 100,
            },
            &root,
        )?;
        Ok(vec![computed])
    }
}

impl Modal for PaneLayoutMenu {
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
            let element = Self::compute(term_window)?;
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
    fn close_action_requires_another_pane_in_the_tab() {
        assert!(!can_close_pane(0));
        assert!(!can_close_pane(1));
        assert!(can_close_pane(2));
        assert!(can_close_pane(3));
    }
}
