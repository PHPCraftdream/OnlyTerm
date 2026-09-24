use crate::alloc::borrow::ToOwned;
use crate::change::{Change, LineAttribute};
#[cfg(feature = "use_image")]
use crate::change::{Image, TextureCoordinate};
use crate::cursor::position::{compute_position_change, Position};
use crate::line::Line;
use crate::{SequenceNo, Surface};
use alloc::vec::Vec;
use core::cmp::min;
use finl_unicode::grapheme_clusters::Graphemes;
use onlyterm_cell::color::ColorAttribute;
#[cfg(feature = "use_image")]
use onlyterm_cell::image::ImageCell;
use onlyterm_cell::{Cell, CellAttributes};

impl Surface {
    /// Efficiently apply a series of changes
    /// Returns the sequence number at the end of the change.
    pub fn add_changes(&mut self, mut changes: Vec<Change>) -> SequenceNo {
        let seq = self.seqno.saturating_sub(1) + changes.len();

        for change in &changes {
            self.apply_change(change);
        }

        self.seqno += changes.len();
        self.changes.append(&mut changes);

        seq
    }

    /// Apply a change and return the sequence number at the end of the change.
    pub fn add_change<C: Into<Change>>(&mut self, change: C) -> SequenceNo {
        let seq = self.seqno;
        self.seqno += 1;
        let change = change.into();
        self.apply_change(&change);
        self.changes.push(change);
        seq
    }

    fn apply_change(&mut self, change: &Change) {
        match change {
            Change::AllAttributes(attr) => self.attributes = attr.clone(),
            Change::Text(text) => self.print_text(text),
            Change::Attribute(change) => self.attributes.apply_change(change),
            Change::CursorPosition { x, y } => self.set_cursor_pos(x, y),
            Change::ClearScreen(color) => self.clear_screen(*color),
            Change::ClearToEndOfLine(color) => self.clear_eol(*color),
            Change::ClearToEndOfScreen(color) => self.clear_eos(*color),
            Change::CursorColor(color) => self.cursor_color = *color,
            Change::CursorShape(shape) => self.cursor_shape = Some(*shape),
            Change::CursorVisibility(visibility) => self.cursor_visibility = *visibility,
            #[cfg(feature = "use_image")]
            Change::Image(image) => self.add_image(image),
            Change::Title(text) => self.title = text.to_owned(),
            Change::ScrollRegionUp {
                first_row,
                region_size,
                scroll_count,
            } => self.scroll_region_up(*first_row, *region_size, *scroll_count),
            Change::ScrollRegionDown {
                first_row,
                region_size,
                scroll_count,
            } => self.scroll_region_down(*first_row, *region_size, *scroll_count),
            Change::LineAttribute(attr) => self.line_attribute(attr),
        }
    }

    #[cfg(feature = "use_image")]
    fn add_image(&mut self, image: &Image) {
        use ordered_float::NotNan;

        let xsize = (image.bottom_right.x - image.top_left.x) / image.width as f32;
        let ysize = (image.bottom_right.y - image.top_left.y) / image.height as f32;

        if self.ypos + image.height > self.height {
            let scroll = (self.ypos + image.height) - self.height;
            for _ in 0..scroll {
                self.scroll_screen_up();
            }
            self.ypos -= scroll;
        }

        let mut ypos = NotNan::new(0.0).unwrap();
        for y in 0..image.height {
            let mut xpos = NotNan::new(0.0).unwrap();
            for x in 0..image.width {
                self.lines[self.ypos + y].set_cell(
                    self.xpos + x,
                    Cell::new(
                        ' ',
                        self.attributes
                            .clone()
                            .set_image(Box::new(ImageCell::new(
                                TextureCoordinate::new(
                                    image.top_left.x + xpos,
                                    image.top_left.y + ypos,
                                ),
                                TextureCoordinate::new(
                                    image.top_left.x + xpos + xsize,
                                    image.top_left.y + ypos + ysize,
                                ),
                                image.image.clone(),
                            )))
                            .clone(),
                    ),
                    self.seqno,
                );

                xpos += xsize;
            }
            ypos += ysize;
        }

        self.xpos += image.width;
    }

    fn clear_screen(&mut self, color: ColorAttribute) {
        self.attributes = CellAttributes::default().set_background(color).clone();
        let cleared = Cell::new(' ', self.attributes.clone());
        for line in &mut self.lines {
            line.fill_range(0..self.width, &cleared, self.seqno);
        }
        self.xpos = 0;
        self.ypos = 0;
    }

    fn clear_eos(&mut self, color: ColorAttribute) {
        self.attributes = CellAttributes::default().set_background(color).clone();
        let cleared = Cell::new(' ', self.attributes.clone());
        self.lines[self.ypos].fill_range(self.xpos..self.width, &cleared, self.seqno);
        for line in &mut self.lines.iter_mut().skip(self.ypos + 1) {
            line.fill_range(0..self.width, &cleared, self.seqno);
        }
    }

    fn clear_eol(&mut self, color: ColorAttribute) {
        self.attributes = CellAttributes::default().set_background(color).clone();
        let cleared = Cell::new(' ', self.attributes.clone());
        self.lines[self.ypos].fill_range(self.xpos..self.width, &cleared, self.seqno);
    }

    fn scroll_screen_up(&mut self) {
        self.lines.remove(0);
        self.lines.push(Line::with_width(self.width, self.seqno));
    }

    fn scroll_region_up(&mut self, start: usize, size: usize, count: usize) {
        // Replace the first lines with empty lines
        for index in start..start + min(count, size) {
            self.lines[index] = Line::with_width(self.width, self.seqno);
        }
        // Rotate the remaining lines up the surface.
        if 0 < count && count < size {
            self.lines[start..start + size].rotate_left(count);
        }
    }

    fn scroll_region_down(&mut self, start: usize, size: usize, count: usize) {
        // Replace the last lines with empty lines
        for index in start + size - min(count, size)..start + size {
            self.lines[index] = Line::with_width(self.width, self.seqno);
        }
        // Rotate the remaining lines down the surface.
        if 0 < count && count < size {
            self.lines[start..start + size].rotate_right(count);
        }
    }

    fn line_attribute(&mut self, attr: &LineAttribute) {
        let line = &mut self.lines[self.ypos];
        match attr {
            LineAttribute::DoubleHeightTopHalfLine => line.set_double_height_top(self.seqno),
            LineAttribute::DoubleHeightBottomHalfLine => line.set_double_height_bottom(self.seqno),
            LineAttribute::DoubleWidthLine => line.set_double_width(self.seqno),
            LineAttribute::SingleWidthLine => line.set_single_width(self.seqno),
        }
    }

    fn print_text(&mut self, text: &str) {
        for g in Graphemes::new(text) {
            if g == "\r\n" {
                self.xpos = 0;
                let new_y = self.ypos + 1;
                if new_y >= self.height {
                    self.scroll_screen_up();
                } else {
                    self.ypos = new_y;
                }
                continue;
            }

            if g == "\r" {
                self.xpos = 0;
                continue;
            }

            if g == "\n" {
                let new_y = self.ypos + 1;
                if new_y >= self.height {
                    self.scroll_screen_up();
                } else {
                    self.ypos = new_y;
                }
                continue;
            }

            if self.xpos >= self.width {
                let new_y = self.ypos + 1;
                if new_y >= self.height {
                    self.scroll_screen_up();
                } else {
                    self.ypos = new_y;
                }
                self.xpos = 0;
            }

            let cell = Cell::new_grapheme(g, self.attributes.clone(), None);
            // the max(1) here is to ensure that we advance to the next cell
            // position for zero-width graphemes.  We want to make sure that
            // they occupy a cell so that we can re-emit them when we output them.
            // If we didn't do this, then we'd effectively filter them out from
            // the model, which seems like a lossy design choice.
            let width = cell.width().max(1);

            self.lines[self.ypos].set_cell(self.xpos, cell, self.seqno);

            // Increment the position now; we'll defer processing
            // wrapping until the next printed character, otherwise
            // we'll eagerly scroll when we reach the right margin.
            self.xpos += width;
        }
    }

    fn set_cursor_pos(&mut self, x: &Position, y: &Position) {
        self.xpos = compute_position_change(self.xpos, x, self.width);
        self.ypos = compute_position_change(self.ypos, y, self.height);
    }
}
