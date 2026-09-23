use super::Line;
use crate::line::linebits::LineBits;
use crate::line::{CellRef, ZoneRange};
use crate::SequenceNo;
use alloc::sync::Arc;
use alloc::vec;
use onlyterm_bidi::{Direction, ParagraphDirectionHint};
use onlyterm_cell::Cell;

extern crate alloc;

impl Line {
    /// Check whether the line is single-width.
    #[inline]
    pub fn is_single_width(&self) -> bool {
        (self.bits
            & (LineBits::DOUBLE_WIDTH
                | LineBits::DOUBLE_HEIGHT_TOP
                | LineBits::DOUBLE_HEIGHT_BOTTOM))
            == LineBits::NONE
    }

    /// Force single-width.  This also implicitly sets
    /// double-height-(top/bottom) and dirty.
    #[inline]
    pub fn set_single_width(&mut self, seqno: SequenceNo) {
        self.bits.remove(LineBits::DOUBLE_WIDTH_HEIGHT_MASK);
        self.update_last_change_seqno(seqno);
    }

    /// Check whether the line is double-width and not double-height.
    #[inline]
    pub fn is_double_width(&self) -> bool {
        (self.bits & LineBits::DOUBLE_WIDTH_HEIGHT_MASK) == LineBits::DOUBLE_WIDTH
    }

    /// Force double-width.  This also implicitly sets
    /// double-height-(top/bottom) and dirty.
    #[inline]
    pub fn set_double_width(&mut self, seqno: SequenceNo) {
        self.bits
            .remove(LineBits::DOUBLE_HEIGHT_TOP | LineBits::DOUBLE_HEIGHT_BOTTOM);
        self.bits.insert(LineBits::DOUBLE_WIDTH);
        self.update_last_change_seqno(seqno);
    }

    /// Check whether the line is double-height-top.
    #[inline]
    pub fn is_double_height_top(&self) -> bool {
        (self.bits & LineBits::DOUBLE_WIDTH_HEIGHT_MASK)
            == LineBits::DOUBLE_WIDTH | LineBits::DOUBLE_HEIGHT_TOP
    }

    /// Force double-height top-half.  This also implicitly sets
    /// double-width and dirty.
    #[inline]
    pub fn set_double_height_top(&mut self, seqno: SequenceNo) {
        self.bits.remove(LineBits::DOUBLE_HEIGHT_BOTTOM);
        self.bits
            .insert(LineBits::DOUBLE_WIDTH | LineBits::DOUBLE_HEIGHT_TOP);
        self.update_last_change_seqno(seqno);
    }

    /// Check whether the line is double-height-bottom.
    #[inline]
    pub fn is_double_height_bottom(&self) -> bool {
        (self.bits & LineBits::DOUBLE_WIDTH_HEIGHT_MASK)
            == LineBits::DOUBLE_WIDTH | LineBits::DOUBLE_HEIGHT_BOTTOM
    }

    /// Force double-height bottom-half.  This also implicitly sets
    /// double-width and dirty.
    #[inline]
    pub fn set_double_height_bottom(&mut self, seqno: SequenceNo) {
        self.bits.remove(LineBits::DOUBLE_HEIGHT_TOP);
        self.bits
            .insert(LineBits::DOUBLE_WIDTH | LineBits::DOUBLE_HEIGHT_BOTTOM);
        self.update_last_change_seqno(seqno);
    }

    /// Set a flag the indicate whether the line should have the bidi
    /// algorithm applied during rendering
    pub fn set_bidi_enabled(&mut self, enabled: bool, seqno: SequenceNo) {
        self.bits.set(LineBits::BIDI_ENABLED, enabled);
        self.update_last_change_seqno(seqno);
    }

    /// Set the bidi direction for the line.
    /// This affects both the bidi algorithm (if enabled via set_bidi_enabled)
    /// and the layout direction of the line.
    /// `auto_detect` specifies whether the direction should be auto-detected
    /// before falling back to the specified direction.
    pub fn set_direction(&mut self, direction: Direction, auto_detect: bool, seqno: SequenceNo) {
        self.bits
            .set(LineBits::RTL, direction == Direction::LeftToRight);
        self.bits.set(LineBits::AUTO_DETECT_DIRECTION, auto_detect);
        self.update_last_change_seqno(seqno);
    }

    pub fn set_bidi_info(
        &mut self,
        enabled: bool,
        direction: ParagraphDirectionHint,
        seqno: SequenceNo,
    ) {
        self.bits.set(LineBits::BIDI_ENABLED, enabled);
        let (auto, rtl) = match direction {
            ParagraphDirectionHint::AutoRightToLeft => (true, true),
            ParagraphDirectionHint::AutoLeftToRight => (true, false),
            ParagraphDirectionHint::LeftToRight => (false, false),
            ParagraphDirectionHint::RightToLeft => (false, true),
        };
        self.bits.set(LineBits::AUTO_DETECT_DIRECTION, auto);
        self.bits.set(LineBits::RTL, rtl);
        self.update_last_change_seqno(seqno);
    }

    /// Returns a tuple of (BIDI_ENABLED, Direction), indicating whether
    /// the line should have the bidi algorithm applied and its base
    /// direction, respectively.
    pub fn bidi_info(&self) -> (bool, ParagraphDirectionHint) {
        (
            self.bits.contains(LineBits::BIDI_ENABLED),
            match (
                self.bits.contains(LineBits::AUTO_DETECT_DIRECTION),
                self.bits.contains(LineBits::RTL),
            ) {
                (true, true) => ParagraphDirectionHint::AutoRightToLeft,
                (false, true) => ParagraphDirectionHint::RightToLeft,
                (true, false) => ParagraphDirectionHint::AutoLeftToRight,
                (false, false) => ParagraphDirectionHint::LeftToRight,
            },
        )
    }

    fn compute_zones(&mut self) {
        let blank_cell = Cell::blank();
        let mut last_cell: Option<CellRef> = None;
        let mut current_zone: Option<ZoneRange> = None;
        let mut zones = vec![];

        // Rows may have trailing space+Output cells interleaved
        // with other zones as a result of clear-to-eol and
        // clear-to-end-of-screen sequences.  We don't want
        // those to affect the zones that we compute here
        let mut last_non_blank = self.len();
        for cell in self.visible_cells() {
            if cell.str() != blank_cell.str() || cell.attrs() != blank_cell.attrs() {
                last_non_blank = cell.cell_index();
            }
        }

        for cell in self.visible_cells() {
            if cell.cell_index() > last_non_blank {
                break;
            }
            let grapheme_idx = cell.cell_index() as u16;
            let semantic_type = cell.attrs().semantic_type();
            let new_zone = match last_cell {
                None => true,
                Some(ref c) => c.attrs().semantic_type() != semantic_type,
            };

            if new_zone {
                if let Some(zone) = current_zone.take() {
                    zones.push(zone);
                }

                current_zone.replace(ZoneRange {
                    range: grapheme_idx..grapheme_idx + 1,
                    semantic_type,
                });
            }

            if let Some(zone) = current_zone.as_mut() {
                zone.range.end = grapheme_idx;
            }

            last_cell.replace(cell);
        }

        if let Some(zone) = current_zone.take() {
            zones.push(zone);
        }
        self.zones = Arc::new(zones);
    }

    pub fn semantic_zone_ranges(&mut self) -> &[ZoneRange] {
        if self.zones.is_empty() {
            self.compute_zones();
        }
        &self.zones
    }
}
