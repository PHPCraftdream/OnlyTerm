use crate::termwindow::box_model::*;
use crate::termwindow::render::corners::{
    BOTTOM_LEFT_ROUNDED_CORNER, BOTTOM_RIGHT_ROUNDED_CORNER, TOP_LEFT_ROUNDED_CORNER,
    TOP_RIGHT_ROUNDED_CORNER,
};
use onlyterm_config::{Dimension, RgbaColor};
use onlyterm_font::LoadedFont;
use std::rc::Rc;
use window::color::LinearRgba;

pub(crate) struct MenuStyle {
    pub base: ElementColors,
    pub card: ElementColors,
    pub focus: ElementColors,
    pub muted: LinearRgba,
    pub foreground: LinearRgba,
    pub chip: LinearRgba,
    pub edge: LinearRgba,
}

impl MenuStyle {
    pub fn new(background: RgbaColor, foreground: RgbaColor) -> Self {
        let bg = *background;
        let fg = *foreground;
        let edge = bg.interpolate(fg, 0.22).to_linear();
        let ink = fg.to_linear();
        Self {
            base: ElementColors {
                border: BorderColor::new(edge),
                bg: bg.to_linear().into(),
                text: ink.into(),
            },
            card: ElementColors {
                border: BorderColor::new(edge),
                bg: bg.interpolate(fg, 0.06).to_linear().into(),
                text: ink.into(),
            },
            focus: ElementColors {
                border: BorderColor::new(ink),
                bg: bg.interpolate(fg, 0.12).to_linear().into(),
                text: ink.into(),
            },
            muted: bg.interpolate(fg, 0.85).to_linear(),
            foreground: ink,
            chip: bg.interpolate(fg, 0.14).to_linear(),
            edge,
        }
    }

    pub fn text(&self, font: &Rc<LoadedFont>, value: String, muted: bool) -> Element {
        Element::new(font, ElementContent::Text(value))
            .colors(ElementColors {
                text: if muted { self.muted } else { self.foreground }.into(),
                ..ElementColors::default()
            })
            .display(DisplayType::Block)
            .padding(BoxDimension {
                top: Dimension::Pixels(2.),
                bottom: Dimension::Pixels(2.),
                ..BoxDimension::default()
            })
    }

    pub fn keycap(&self, font: &Rc<LoadedFont>, value: String) -> Element {
        Element::new(font, ElementContent::Text(value))
            .colors(ElementColors {
                border: BorderColor::new(self.edge),
                bg: self.chip.into(),
                text: self.foreground.into(),
            })
            .padding(BoxDimension {
                left: Dimension::Pixels(8.),
                right: Dimension::Pixels(8.),
                top: Dimension::Pixels(4.),
                bottom: Dimension::Pixels(4.),
            })
            .border(BoxDimension::new(Dimension::Pixels(1.)))
            .border_corners(Some(corners()))
    }

    pub fn panel(
        &self,
        font: &Rc<LoadedFont>,
        children: Vec<Element>,
        outer_width: f32,
    ) -> Element {
        Element::new(font, ElementContent::Children(children))
            .colors(self.base.clone())
            .display(DisplayType::Block)
            .padding(BoxDimension::new(Dimension::Pixels(16.)))
            .border(BoxDimension::new(Dimension::Pixels(1.)))
            .border_corners(Some(corners()))
            .min_width(Some(Dimension::Pixels((outer_width - 34.).max(1.))))
            .max_width(Some(Dimension::Pixels(outer_width)))
    }

    pub fn card(&self, font: &Rc<LoadedFont>, children: Vec<Element>, outer_width: f32) -> Element {
        Element::new(font, ElementContent::Children(children))
            .colors(self.card.clone())
            .display(DisplayType::Block)
            .padding(BoxDimension::new(Dimension::Pixels(10.)))
            .border(BoxDimension::new(Dimension::Pixels(1.)))
            .border_corners(Some(corners()))
            .min_width(Some(Dimension::Pixels((outer_width - 22.).max(1.))))
            .max_width(Some(Dimension::Pixels(outer_width)))
            .margin(BoxDimension {
                bottom: Dimension::Pixels(8.),
                ..BoxDimension::default()
            })
    }

    pub fn button(
        &self,
        font: &Rc<LoadedFont>,
        label: &str,
        focused: bool,
        enabled: bool,
    ) -> Element {
        let mut colors = if focused && enabled {
            self.focus.clone()
        } else {
            self.card.clone()
        };
        if !enabled {
            colors.text = self.muted.into();
        }
        Element::new(font, ElementContent::Text(label.into()))
            .colors(colors)
            .padding(BoxDimension {
                left: Dimension::Pixels(12.),
                right: Dimension::Pixels(12.),
                top: Dimension::Pixels(6.),
                bottom: Dimension::Pixels(6.),
            })
            .border(BoxDimension::new(Dimension::Pixels(1.)))
            .border_corners(Some(corners()))
            .hover_colors(enabled.then(|| self.focus.clone()))
            .margin(BoxDimension {
                right: Dimension::Pixels(8.),
                ..BoxDimension::default()
            })
    }
}

#[derive(Clone, Copy)]
pub struct ScrollPosition {
    pub offset: usize,
    pub visible: usize,
    pub total: usize,
}

impl ScrollPosition {
    pub fn limit(self) -> usize {
        self.total.saturating_sub(self.visible)
    }

    pub fn thumb(self, height: f32) -> (f32, f32) {
        let size = (height * self.visible as f32 / self.total.max(1) as f32)
            .clamp(24_f32.min(height), height);
        let top =
            (height - size) * self.offset.min(self.limit()) as f32 / self.limit().max(1) as f32;
        (top, size)
    }

    pub fn offset_at(self, top: f32, height: f32) -> usize {
        let (_, size) = self.thumb(height);
        let travel = height - size;
        if travel <= 0. {
            return 0;
        }
        ((top / travel).clamp(0., 1.) * self.limit() as f32).round() as usize
    }
}

impl MenuStyle {
    pub fn viewport(
        &self,
        font: &Rc<LoadedFont>,
        children: Vec<Element>,
        width: f32,
        height: f32,
        position: ScrollPosition,
    ) -> Element {
        let mut parts = vec![];
        if position.limit() > 0 {
            let (top, size) = position.thumb(height);
            let blank = |height: f32, color: LinearRgba| {
                Element::new(font, ElementContent::Children(vec![]))
                    .display(DisplayType::Block)
                    .min_width(Some(Dimension::Pixels(10.)))
                    .min_height(Some(Dimension::Pixels(height)))
                    .colors(ElementColors {
                        bg: color.into(),
                        ..ElementColors::default()
                    })
            };
            parts.push(
                Element::new(
                    font,
                    ElementContent::Children(vec![
                        blank(top, self.chip),
                        blank(size, self.muted),
                        blank((height - top - size).max(0.), self.chip),
                    ]),
                )
                .float(Float::Right)
                .item_type(crate::termwindow::UIItemType::ModalScrollBar),
            );
        }
        parts.push(
            Element::new(font, ElementContent::Children(children))
                .min_height(Some(Dimension::Pixels(height)))
                .min_width(Some(Dimension::Pixels((width - 20.).max(1.))))
                .max_width(Some(Dimension::Pixels((width - 20.).max(1.)))),
        );
        Element::new(font, ElementContent::Children(parts))
            .display(DisplayType::Block)
            .min_width(Some(Dimension::Pixels(width)))
            .max_width(Some(Dimension::Pixels(width)))
    }
}

pub(crate) fn corners() -> Corners {
    Corners {
        top_left: SizedPoly {
            width: Dimension::Pixels(6.),
            height: Dimension::Pixels(6.),
            poly: TOP_LEFT_ROUNDED_CORNER,
        },
        top_right: SizedPoly {
            width: Dimension::Pixels(6.),
            height: Dimension::Pixels(6.),
            poly: TOP_RIGHT_ROUNDED_CORNER,
        },
        bottom_left: SizedPoly {
            width: Dimension::Pixels(6.),
            height: Dimension::Pixels(6.),
            poly: BOTTOM_LEFT_ROUNDED_CORNER,
        },
        bottom_right: SizedPoly {
            width: Dimension::Pixels(6.),
            height: Dimension::Pixels(6.),
            poly: BOTTOM_RIGHT_ROUNDED_CORNER,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::ScrollPosition;

    #[test]
    fn scrollbar_maps_both_ends_and_round_trips_the_thumb() {
        let mut position = ScrollPosition {
            offset: 0,
            visible: 10,
            total: 100,
        };
        assert_eq!(position.offset_at(-100., 300.), 0);
        assert_eq!(position.offset_at(300., 300.), 90);
        for offset in 0..=90 {
            position.offset = offset;
            let (top, height) = position.thumb(300.);
            assert!(height >= 24.);
            assert_eq!(position.offset_at(top, 300.), offset);
        }
        position.total = 5;
        assert_eq!(position.thumb(300.), (0., 300.));
        assert_eq!(position.offset_at(300., 300.), 0);
    }
}
