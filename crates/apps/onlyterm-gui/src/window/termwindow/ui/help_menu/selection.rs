use super::{line_style, HelpLine};
use crate::termwindow::box_model::{ComputedElement, ComputedElementContent, TextCell};
use crate::termwindow::UIItemType;
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;
use window::RectF;

#[derive(Default)]
pub(super) struct TextSelection {
    pub anchor: Option<usize>,
    pub focus: usize,
    pub dragging: bool,
}

impl TextSelection {
    pub fn range(&self) -> Option<Range<usize>> {
        let anchor = self.anchor?;
        (anchor != self.focus).then(|| anchor.min(self.focus)..anchor.max(self.focus))
    }
}

pub(super) fn wrap_document(document: &str, width: usize) -> Vec<HelpLine> {
    let mut result = vec![];
    let mut base = 0;
    for source in document.split_inclusive('\n') {
        let line = source.strip_suffix('\n').unwrap_or(source);
        let line = line.strip_suffix('\r').unwrap_or(line);
        let style = line_style(line);
        let mut cursor = 0;
        for value in textwrap::wrap(line, width) {
            let start = cursor
                + line[cursor..]
                    .find(value.as_ref())
                    .expect("default wrapping retains source substrings");
            cursor = start + value.len();
            result.push(HelpLine {
                text: value.into_owned(),
                style,
                source: base + start..base + cursor,
            });
        }
        base += source.len();
    }
    result
}

fn grapheme_boundary(text: &str, fraction: f32) -> usize {
    let count = text.graphemes(true).count();
    let index = (fraction.clamp(0., 1.) * count as f32).round() as usize;
    text.grapheme_indices(true)
        .nth(index)
        .map_or(text.len(), |(byte, _)| byte)
}

pub(super) fn caret(cells: &[TextCell], text: &str, x: f32) -> usize {
    for cell in cells {
        if x < cell.right {
            let fraction = if cell.right > cell.left {
                (x - cell.left) / (cell.right - cell.left)
            } else {
                0.
            };
            return cell.source.start + grapheme_boundary(&text[cell.source.clone()], fraction);
        }
    }
    cells.last().map_or(0, |cell| cell.source.end)
}

pub(super) fn selected_pixels(
    cells: &[TextCell],
    text: &str,
    selected: Range<usize>,
) -> Vec<Range<f32>> {
    let mut ranges: Vec<Range<f32>> = vec![];
    for cell in cells {
        let start = selected.start.max(cell.source.start);
        let end = selected.end.min(cell.source.end);
        if start >= end {
            continue;
        }
        let cluster = &text[cell.source.clone()];
        let count = cluster.graphemes(true).count().max(1) as f32;
        let before = text[cell.source.start..start].graphemes(true).count() as f32;
        let through = text[cell.source.start..end].graphemes(true).count() as f32;
        let width = cell.right - cell.left;
        let range = cell.left + width * before / count..cell.left + width * through / count;
        if let Some(previous) = ranges
            .last_mut()
            .filter(|previous| previous.end >= range.start)
        {
            previous.end = previous.end.max(range.end);
        } else {
            ranges.push(range);
        }
    }
    ranges
}

pub(super) fn text_at(
    elements: &[ComputedElement],
    document: &str,
    x: f32,
    y: f32,
) -> Option<usize> {
    fn visit(
        element: &ComputedElement,
        document: &str,
        x: f32,
        y: f32,
        nearest: &mut Option<(f32, usize)>,
    ) {
        if let Some(UIItemType::HelpText { start, end }) = element.item_type.as_ref() {
            let rect: RectF = element.bounds;
            let vertical = if y < rect.min_y() {
                rect.min_y() - y
            } else if y > rect.max_y() {
                y - rect.max_y()
            } else {
                0.
            };
            let horizontal = if x < rect.min_x() {
                rect.min_x() - x
            } else if x > rect.max_x() {
                x - rect.max_x()
            } else {
                0.
            };
            let distance = vertical * 10000. + horizontal;
            let position = *start
                + caret(
                    element.text_cells.as_deref().unwrap_or(&[]),
                    &document[*start..*end],
                    x - element.content_rect.min_x(),
                )
                .min(end - start);
            if nearest.as_ref().is_none_or(|(old, _)| distance < *old) {
                *nearest = Some((distance, position));
            }
        }
        if let ComputedElementContent::Children(children) = &element.content {
            for child in children {
                visit(child, document, x, y, nearest);
            }
        }
    }
    let mut nearest = None;
    for element in elements {
        visit(element, document, x, y, &mut nearest);
    }
    nearest.map(|(_, position)| position)
}

pub(super) fn is_text_region(elements: &[ComputedElement], x: f32, y: f32) -> bool {
    elements.iter().any(|element| {
        if element.item_type == Some(UIItemType::HelpTextRegion)
            && x >= element.bounds.min_x()
            && x <= element.bounds.max_x()
            && y >= element.bounds.min_y()
            && y <= element.bounds.max_y()
        {
            return true;
        }
        match &element.content {
            ComputedElementContent::Children(children) => is_text_region(children, x, y),
            _ => false,
        }
    })
}

pub(super) fn apply_selection(
    element: &mut ComputedElement,
    document: &str,
    selection: &Range<usize>,
    colors: &crate::termwindow::box_model::ElementColors,
) {
    if let Some(UIItemType::HelpText { start, end }) = element.item_type.as_ref() {
        let selected_start = selection.start.max(*start);
        let selected_end = selection.end.min(*end);
        if selected_start < selected_end {
            let ranges = selected_pixels(
                element.text_cells.as_deref().unwrap_or(&[]),
                &document[*start..*end],
                selected_start - start..selected_end - start,
            );
            if !ranges.is_empty() {
                element.text_selection = Some((ranges, colors.clone()));
            }
        }
    }
    if let ComputedElementContent::Children(children) = &mut element.content {
        for child in children {
            apply_selection(child, document, selection, colors);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrapping_preserves_original_spacing_and_hard_newlines() {
        let document = "alpha  beta\r\n\n  powershell.exe -NoLogo\n界e\u{301}🙂";
        let lines = wrap_document(document, 8);
        for line in &lines {
            assert_eq!(&document[line.source.clone()], line.text);
        }
        let first = &lines[0];
        let last = lines.last().unwrap();
        assert_eq!(&document[first.source.start..last.source.end], document);
    }

    #[test]
    fn selection_carets_keep_graphemes_intact_inside_shaped_clusters() {
        let text = "e\u{301}🙂fi";
        let cells = vec![
            TextCell {
                source: 0..3,
                left: 0.,
                right: 10.,
            },
            TextCell {
                source: 3..7,
                left: 10.,
                right: 30.,
            },
            TextCell {
                source: 7..9,
                left: 30.,
                right: 40.,
            },
        ];
        assert_eq!(caret(&cells, text, 2.), 0);
        assert_eq!(caret(&cells, text, 8.), 3);
        assert_eq!(caret(&cells, text, 28.), 7);
        assert_eq!(caret(&cells, text, 35.), 8);
        assert_eq!(selected_pixels(&cells, text, 7..8), vec![30.0..35.0]);
        let selection = TextSelection {
            anchor: Some(9),
            focus: 3,
            dragging: false,
        };
        assert_eq!(&text[selection.range().unwrap()], "🙂fi");
    }
}
