use crate::termwindow::menu_style::MenuStyle;
use onlyterm_config::configuration;
use termwiz::cell::{CellAttributes, Intensity};
use termwiz::color::ColorAttribute;
use termwiz::surface::{Change, Position};
use unicode_segmentation::UnicodeSegmentation;

pub(crate) const LIST_TOP: usize = 4;
pub(crate) const ROW_OVERHEAD: usize = 8;

pub(crate) struct TextMenuStyle {
    pub background: ColorAttribute,
    pub normal: CellAttributes,
    pub heading: CellAttributes,
    pub selected: CellAttributes,
    pub label: CellAttributes,
    pub muted: CellAttributes,
}

impl TextMenuStyle {
    pub fn new() -> Self {
        let config = configuration();
        let style = MenuStyle::new(
            config.command_palette_bg_color,
            config.command_palette_fg_color,
        );
        let background =
            ColorAttribute::TrueColorWithDefaultFallback(*config.command_palette_bg_color);
        let foreground = ColorAttribute::TrueColorWithDefaultFallback(style.foreground.to_srgb());
        let mut normal = CellAttributes::default();
        normal.set_background(background).set_foreground(foreground);
        let mut heading = normal.clone();
        heading.set_intensity(Intensity::Bold);
        let mut selected = normal.clone();
        selected
            .set_background(foreground)
            .set_foreground(background);
        let mut label = normal.clone();
        label
            .set_background(ColorAttribute::TrueColorWithDefaultFallback(
                style.chip.to_srgb(),
            ))
            .set_intensity(Intensity::Bold);
        let mut muted = normal.clone();
        muted.set_foreground(ColorAttribute::TrueColorWithDefaultFallback(
            style.muted.to_srgb(),
        ));
        Self {
            background,
            normal,
            heading,
            selected,
            label,
            muted,
        }
    }

    pub fn frame(
        &self,
        cols: usize,
        rows: usize,
        title: &str,
        description: &str,
        footer: &str,
    ) -> Vec<Change> {
        let width = cols.saturating_sub(2);
        let rule = "─".repeat(width.saturating_sub(2));
        let mut changes = vec![
            Change::ClearScreen(self.background),
            Change::AllAttributes(self.normal.clone()),
        ];
        if cols < 4 || rows < 6 {
            return changes;
        }
        for (y, value) in [(0, format!("╭{rule}╮")), (rows - 1, format!("╰{rule}╯"))] {
            changes.push(Change::CursorPosition {
                x: Position::Absolute(1),
                y: Position::Absolute(y),
            });
            changes.push(Change::Text(value));
        }
        for y in 1..rows - 1 {
            for x in [1, cols - 2] {
                changes.push(Change::CursorPosition {
                    x: Position::Absolute(x),
                    y: Position::Absolute(y),
                });
                changes.push(Change::Text("│".into()));
            }
        }
        changes.extend([
            Change::CursorPosition {
                x: Position::Absolute(3),
                y: Position::Absolute(1),
            },
            Change::AllAttributes(self.heading.clone()),
            Change::Text(truncate(title, cols.saturating_sub(6))),
            Change::CursorPosition {
                x: Position::Absolute(3),
                y: Position::Absolute(2),
            },
            Change::AllAttributes(self.muted.clone()),
            Change::Text(truncate(description, cols.saturating_sub(6))),
            Change::CursorPosition {
                x: Position::Absolute(3),
                y: Position::Absolute(rows.saturating_sub(2)),
            },
            Change::Text(truncate(footer, cols.saturating_sub(6))),
            Change::AllAttributes(self.normal.clone()),
        ]);
        changes
    }
    pub fn scrollbar(
        &self,
        cols: usize,
        rows: usize,
        offset: usize,
        visible: usize,
        total: usize,
    ) -> Vec<Change> {
        if total <= visible || cols < 6 || rows < ROW_OVERHEAD {
            return vec![];
        }
        let position = crate::termwindow::menu_style::ScrollPosition {
            offset,
            visible,
            total,
        };
        let (top, height) = position.thumb(visible as f32 * 24.);
        let mut changes = vec![];
        for row in 0..visible {
            changes.push(Change::CursorPosition {
                x: Position::Absolute(cols - 3),
                y: Position::Absolute(LIST_TOP + row),
            });
            changes.push(Change::AllAttributes(self.muted.clone()));
            changes.push(Change::Text(
                if (row as f32) >= (top / 24.).floor()
                    && (row as f32) < ((top + height) / 24.).ceil()
                {
                    "█"
                } else {
                    "│"
                }
                .into(),
            ));
        }
        changes
    }
}

pub(crate) fn truncate(text: &str, columns: usize) -> String {
    let mut width = 0;
    let mut end = 0;
    for (index, grapheme) in text.grapheme_indices(true) {
        let len = termwiz::cell::grapheme_column_width(grapheme, None);
        if width + len > columns {
            break;
        }
        width += len;
        end = index + grapheme.len();
    }
    text[..end].to_string()
}

#[cfg(test)]
mod tests {
    use super::truncate;

    #[test]
    fn truncation_keeps_combining_marks_and_emoji_sequences_intact() {
        assert_eq!(truncate("e\u{301}界🙂", 1), "e\u{301}");
        assert_eq!(truncate("e\u{301}界🙂", 3), "e\u{301}界");
        assert_eq!(truncate("👨‍👩‍👧‍👦x", 1), "");
        assert_eq!(truncate("👨‍👩‍👧‍👦x", 2), "👨‍👩‍👧‍👦");
    }
}
