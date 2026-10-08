use crate::termwindow::box_model::*;
use crate::termwindow::modal::Modal;
use crate::termwindow::{DimensionContext, TermWindow, TermWindowNotif, UIItemType};
use crate::utilsprites::RenderMetrics;
use onlyterm_config::keyassignment::{ClipboardPasteSource, KeyAssignment};
use onlyterm_config::Dimension;
use onlyterm_mux::tab::TabId;
use onlyterm_mux::Mux;
use onlyterm_term::{KeyCode, KeyModifiers, MouseEvent};
use std::cell::{Ref, RefCell};
use std::sync::Arc;
use termwiz::cell::grapheme_column_width;
use termwiz::lineedit::{LineEditBuffer, Movement};
use unicode_segmentation::UnicodeSegmentation;
use window::{Clipboard, WindowOps};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenameTabAction {
    Input,
    Cancel,
    Apply,
}

struct NameEditor {
    buffer: LineEditBuffer,
    selected_all: bool,
}

impl NameEditor {
    fn new(title: &str) -> Self {
        Self {
            buffer: LineEditBuffer::new(title, title.len()),
            selected_all: !title.is_empty(),
        }
    }

    fn replace_selection(&mut self) {
        if self.selected_all {
            self.buffer.clear();
            self.selected_all = false;
        }
    }

    fn insert_char(&mut self, character: char) {
        self.replace_selection();
        self.buffer.insert_char(character);
    }

    fn insert_text(&mut self, text: &str) {
        self.replace_selection();
        self.buffer.insert_text(text);
    }

    fn delete(&mut self, backwards: bool) {
        if self.selected_all {
            self.replace_selection();
        } else if backwards {
            self.buffer
                .kill_text(Movement::BackwardChar(1), Movement::BackwardChar(1));
        } else {
            self.buffer
                .kill_text(Movement::ForwardChar(1), Movement::None);
        }
    }

    fn move_cursor(&mut self, movement: Movement) {
        if self.selected_all {
            self.buffer.exec_movement(match movement {
                Movement::BackwardChar(_) | Movement::StartOfLine => Movement::StartOfLine,
                _ => Movement::EndOfLine,
            });
            self.selected_all = false;
        } else {
            self.buffer.exec_movement(movement);
        }
    }

    fn visible_start(&self, columns: usize) -> usize {
        let cursor = self.buffer.get_cursor();
        let mut start = cursor;
        let mut remaining = columns.saturating_sub(1);
        for (index, grapheme) in self.buffer.get_line()[..cursor]
            .grapheme_indices(true)
            .rev()
        {
            let width = grapheme_column_width(grapheme, None);
            if width > remaining {
                break;
            }
            remaining -= width;
            start = index;
        }
        start
    }

    fn visible_end(&self, start: usize, columns: usize) -> usize {
        let mut remaining = columns.saturating_sub(1);
        let mut end = start;
        for (index, grapheme) in self.buffer.get_line()[start..].grapheme_indices(true) {
            let width = grapheme_column_width(grapheme, None);
            if width > remaining {
                break;
            }
            remaining -= width;
            end = start + index + grapheme.len();
        }
        end.max(self.buffer.get_cursor())
    }
}

pub(crate) struct RenameTabMenu {
    tab_id: TabId,
    editor: RefCell<NameEditor>,
    focused: RefCell<RenameTabAction>,
    elements: RefCell<Option<Vec<ComputedElement>>>,
    identity: Arc<()>,
}

impl RenameTabMenu {
    pub(crate) fn new(term_window: &TermWindow) -> Option<Self> {
        let tab = Mux::get().get_active_tab_for_window(term_window.mux_window_id)?;
        Some(Self {
            tab_id: tab.tab_id(),
            editor: RefCell::new(NameEditor::new(&tab.get_title())),
            focused: RefCell::new(RenameTabAction::Input),
            elements: RefCell::new(None),
            identity: Arc::new(()),
        })
    }

    fn invalidate(&self, term_window: &TermWindow) {
        self.elements.borrow_mut().take();
        if let Some(window) = term_window.window.as_ref() {
            window.invalidate();
        }
    }

    pub(crate) fn perform_action(&self, action: RenameTabAction, term_window: &mut TermWindow) {
        match action {
            RenameTabAction::Input => {
                self.focused.replace(RenameTabAction::Input);
            }
            RenameTabAction::Cancel => term_window.cancel_modal(),
            RenameTabAction::Apply => {
                if let Some(tab) = Mux::get().get_tab(self.tab_id) {
                    tab.set_title(self.editor.borrow().buffer.get_line());
                }
                term_window.cancel_modal();
            }
        }
        self.invalidate(term_window);
    }

    fn move_focus(&self, direction: i64, horizontal: bool, term_window: &TermWindow) {
        let current = *self.focused.borrow();
        let next = if horizontal {
            match (current, direction < 0) {
                (RenameTabAction::Cancel, false) => RenameTabAction::Apply,
                (RenameTabAction::Apply, true) => RenameTabAction::Cancel,
                _ => current,
            }
        } else if direction < 0 {
            match current {
                RenameTabAction::Input | RenameTabAction::Cancel => RenameTabAction::Input,
                RenameTabAction::Apply => RenameTabAction::Cancel,
            }
        } else {
            match current {
                RenameTabAction::Input => RenameTabAction::Cancel,
                RenameTabAction::Cancel | RenameTabAction::Apply => RenameTabAction::Apply,
            }
        };
        self.focused.replace(next);
        self.invalidate(term_window);
    }

    fn paste(&self, source: ClipboardPasteSource, term_window: &TermWindow) {
        if *self.focused.borrow() != RenameTabAction::Input {
            return;
        }
        let Some(window) = term_window.window.as_ref().cloned() else {
            return;
        };
        let identity = Arc::clone(&self.identity);
        let clipboard = match source {
            ClipboardPasteSource::Clipboard => Clipboard::Clipboard,
            ClipboardPasteSource::PrimarySelection => Clipboard::PrimarySelection,
        };
        onlyterm_promise::spawn::spawn(async move {
            match window.get_clipboard(clipboard).await {
                Ok(text) => window.notify(TermWindowNotif::Apply(Box::new(move |term_window| {
                    if let Some(modal) = term_window.get_modal() {
                        if let Some(menu) = modal.downcast_ref::<RenameTabMenu>() {
                            if Arc::ptr_eq(&menu.identity, &identity)
                                && *menu.focused.borrow() == RenameTabAction::Input
                            {
                                menu.editor.borrow_mut().insert_text(&text);
                                menu.invalidate(term_window);
                            }
                        }
                    }
                }))),
                Err(error) => log::warn!("Cannot paste tab name: {error:#}"),
            }
        })
        .detach();
    }

    fn compute(&self, term_window: &mut TermWindow) -> anyhow::Result<Vec<ComputedElement>> {
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
        let selected_colors = ElementColors {
            border: BorderColor::new(fg),
            bg: fg.into(),
            text: bg.into(),
        };
        let focus = *self.focused.borrow();
        let width_limit = dimensions.pixel_width as f32;
        let height_limit = dimensions.pixel_height as f32;
        let width = (54. * term_window.render_metrics.cell_size.width as f32)
            .min(width_limit)
            .max(1.);
        let row = |text: String| {
            Element::new(&font, ElementContent::Text(text))
                .colors(colors.clone())
                .display(DisplayType::Block)
                .padding(BoxDimension::new(Dimension::Cells(0.5)))
        };
        let editor = self.editor.borrow();
        let columns = ((width - 2. * metrics.cell_size.width as f32)
            / metrics.cell_size.width as f32)
            .floor()
            .max(1.) as usize;
        let start = editor.visible_start(columns);
        let end = editor.visible_end(start, columns);
        let cursor = editor.buffer.get_cursor();
        let line = editor.buffer.get_line();
        let input_content = if focus == RenameTabAction::Input && !editor.selected_all {
            ElementContent::Children(vec![
                Element::new(&font, ElementContent::Text(line[start..cursor].to_string()))
                    .colors(colors.clone()),
                Element::new(&font, ElementContent::Text("│".into()))
                    .colors(selected_colors.clone()),
                Element::new(&font, ElementContent::Text(line[cursor..end].to_string()))
                    .colors(colors.clone()),
            ])
        } else {
            ElementContent::Children(vec![Element::new(
                &font,
                ElementContent::Text(line[start..end].to_string()),
            )
            .colors(if focus == RenameTabAction::Input && editor.selected_all {
                selected_colors.clone()
            } else {
                colors.clone()
            })])
        };
        let input = Element::new(&font, input_content)
            .colors(colors.clone())
            .display(DisplayType::Block)
            .item_type(UIItemType::RenameTabMenuItem(RenameTabAction::Input))
            .padding(BoxDimension::new(Dimension::Cells(0.5)))
            .border(BoxDimension::new(Dimension::Pixels(1.)))
            .min_width(Some(Dimension::Pixels(
                (width - metrics.cell_size.width as f32 - 2.).max(0.),
            )));
        let button = |text: &str, action: RenameTabAction| {
            Element::new(&font, ElementContent::Text(text.into()))
                .colors(if focus == action {
                    selected_colors.clone()
                } else {
                    colors.clone()
                })
                .hover_colors(Some(selected_colors.clone()))
                .item_type(UIItemType::RenameTabMenuItem(action))
                .padding(BoxDimension::new(Dimension::Cells(0.5)))
        };
        let buttons = Element::new(
            &font,
            ElementContent::Children(vec![
                button("Cancel", RenameTabAction::Cancel),
                button("Rename", RenameTabAction::Apply),
            ]),
        )
        .display(DisplayType::Block)
        .colors(colors.clone());
        let root = Element::new(
            &font,
            ElementContent::Children(vec![
                row("Rename Tab".into()),
                input,
                buttons,
                row("Enter to rename; Esc/F2 to cancel; Ctrl+A to select all".into()),
            ]),
        )
        .colors(colors)
        .padding(BoxDimension::new(Dimension::Cells(0.25)))
        .border(BoxDimension::new(Dimension::Pixels(1.)))
        .min_width(Some(Dimension::Pixels(width)));
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

impl Modal for RenameTabMenu {
    fn blocks_terminal_input(&self) -> bool {
        true
    }

    fn composed_text(&self, text: &str, term_window: &mut TermWindow) -> bool {
        if *self.focused.borrow() == RenameTabAction::Input {
            self.editor.borrow_mut().insert_text(text);
            self.invalidate(term_window);
        }
        true
    }

    fn perform_assignment(&self, assignment: &KeyAssignment, term_window: &mut TermWindow) -> bool {
        if let KeyAssignment::PasteFrom(source) = assignment {
            self.paste(*source, term_window);
            true
        } else {
            false
        }
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
        let focus = *self.focused.borrow();
        match (key, mods) {
            (KeyCode::Escape, _)
            | (KeyCode::Function(2), KeyModifiers::NONE)
            | (KeyCode::Char('c'), KeyModifiers::CTRL) => {
                self.perform_action(RenameTabAction::Cancel, term_window)
            }
            (KeyCode::Enter, KeyModifiers::NONE) => self.perform_action(
                if focus == RenameTabAction::Cancel {
                    RenameTabAction::Cancel
                } else {
                    RenameTabAction::Apply
                },
                term_window,
            ),
            (KeyCode::UpArrow, KeyModifiers::NONE) => self.move_focus(-1, false, term_window),
            (KeyCode::DownArrow, KeyModifiers::NONE) => self.move_focus(1, false, term_window),
            (KeyCode::Tab, KeyModifiers::NONE) => self.move_focus(1, false, term_window),
            (KeyCode::Tab, KeyModifiers::SHIFT) => self.move_focus(-1, false, term_window),
            (KeyCode::Char(' '), KeyModifiers::NONE) if focus != RenameTabAction::Input => {
                self.perform_action(focus, term_window)
            }
            (KeyCode::LeftArrow, KeyModifiers::NONE) if focus != RenameTabAction::Input => {
                self.move_focus(-1, true, term_window)
            }
            (KeyCode::RightArrow, KeyModifiers::NONE) if focus != RenameTabAction::Input => {
                self.move_focus(1, true, term_window)
            }
            (KeyCode::Char(character), KeyModifiers::NONE | KeyModifiers::SHIFT)
                if focus == RenameTabAction::Input && !character.is_control() =>
            {
                self.editor.borrow_mut().insert_char(character);
                self.invalidate(term_window);
            }
            (KeyCode::Char('a'), KeyModifiers::CTRL) if focus == RenameTabAction::Input => {
                self.editor.borrow_mut().selected_all = true;
                self.invalidate(term_window);
            }
            (KeyCode::Char('v'), mods)
                if mods == KeyModifiers::CTRL
                    || mods == (KeyModifiers::CTRL | KeyModifiers::SHIFT) =>
            {
                self.paste(ClipboardPasteSource::Clipboard, term_window)
            }
            (KeyCode::Char('u'), KeyModifiers::CTRL) if focus == RenameTabAction::Input => {
                self.editor.borrow_mut().buffer.clear();
                self.editor.borrow_mut().selected_all = false;
                self.invalidate(term_window);
            }
            (KeyCode::Backspace, KeyModifiers::NONE) if focus == RenameTabAction::Input => {
                self.editor.borrow_mut().delete(true);
                self.invalidate(term_window);
            }
            (KeyCode::Delete, KeyModifiers::NONE) if focus == RenameTabAction::Input => {
                self.editor.borrow_mut().delete(false);
                self.invalidate(term_window);
            }
            (KeyCode::LeftArrow, KeyModifiers::NONE) if focus == RenameTabAction::Input => {
                self.editor
                    .borrow_mut()
                    .move_cursor(Movement::BackwardChar(1));
                self.invalidate(term_window);
            }
            (KeyCode::RightArrow, KeyModifiers::NONE) if focus == RenameTabAction::Input => {
                self.editor
                    .borrow_mut()
                    .move_cursor(Movement::ForwardChar(1));
                self.invalidate(term_window);
            }
            (KeyCode::Home, KeyModifiers::NONE) if focus == RenameTabAction::Input => {
                self.editor.borrow_mut().move_cursor(Movement::StartOfLine);
                self.invalidate(term_window);
            }
            (KeyCode::End, KeyModifiers::NONE) if focus == RenameTabAction::Input => {
                self.editor.borrow_mut().move_cursor(Movement::EndOfLine);
                self.invalidate(term_window);
            }
            _ => {}
        }
        Ok(true)
    }

    fn computed_element(
        &self,
        term_window: &mut TermWindow,
    ) -> anyhow::Result<Ref<'_, [ComputedElement]>> {
        if self.elements.borrow().is_none() {
            self.elements
                .borrow_mut()
                .replace(self.compute(term_window)?);
        }
        Ok(Ref::map(self.elements.borrow(), |elements| {
            elements.as_ref().unwrap().as_slice()
        }))
    }

    fn reconfigure(&self, _term_window: &mut TermWindow) {
        self.elements.borrow_mut().take();
    }
}

#[cfg(test)]
mod tests {
    use super::NameEditor;
    use termwiz::lineedit::Movement;

    #[test]
    fn typing_replaces_the_initial_title_and_preserves_unicode() {
        let mut editor = NameEditor::new("previous title");
        editor.insert_text("Имя🙂");
        editor.move_cursor(Movement::BackwardChar(1));
        editor.insert_char('!');
        assert_eq!(editor.buffer.get_line(), "Имя!🙂");
        editor.delete(true);
        editor.delete(false);
        assert_eq!(editor.buffer.get_line(), "Имя");
    }

    #[test]
    fn deleting_initial_selection_can_restore_automatic_title() {
        let mut editor = NameEditor::new("custom");
        editor.delete(true);
        assert_eq!(editor.buffer.get_line(), "");
        assert_eq!(editor.buffer.get_cursor(), 0);
    }

    #[test]
    fn moving_to_start_keeps_long_title_inside_the_input_viewport() {
        let mut editor = NameEditor::new("первая🙂вторая🙂третья");
        editor.move_cursor(Movement::StartOfLine);
        let start = editor.visible_start(8);
        let end = editor.visible_end(start, 8);
        assert_eq!(start, 0);
        assert!(editor.buffer.get_line().is_char_boundary(end));
        assert!(
            termwiz::cell::unicode_column_width(&editor.buffer.get_line()[start..end], None) <= 7
        );
        assert!(end < editor.buffer.get_line().len());
    }

    #[test]
    fn long_unicode_title_window_keeps_cursor_on_valid_boundaries() {
        let editor = NameEditor::new("первое второе третье🙂");
        let start = editor.visible_start(5);
        assert!(editor.buffer.get_line().is_char_boundary(start));
        let visible = &editor.buffer.get_line()[start..];
        assert!(termwiz::cell::unicode_column_width(visible, None) <= 4);
        assert!(visible.ends_with('🙂'));
    }
}
