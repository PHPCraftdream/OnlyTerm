use super::*;
use crate::color::ColorPalette;

impl Screen {
    pub(crate) fn discard_blank_resize_history(&mut self, palette: &ColorPalette) {
        while self.lines.len() > self.physical_rows {
            let line = &self.lines[0];
            if !line.is_whitespace()
                || line.last_cell_was_wrapped()
                || !line.visible_cells().all(|cell| {
                    let attrs = cell.attrs();
                    // Invisible prompt/input cells still define semantic zones.
                    attrs.semantic_type() == SemanticType::Output
                        && !attrs.reverse()
                        && attrs.underline() == Underline::None
                        && !attrs.overline()
                        && !attrs.strikethrough()
                        && attrs.hyperlink().is_none()
                        && attrs.images().is_none()
                        && palette.resolve_bg(attrs.background()) == palette.background
                })
            {
                break;
            }
            self.lines.pop_front();
            // Preserve the stable identities of every remaining row.
            self.stable_row_index_offset += 1;
        }
    }
}
