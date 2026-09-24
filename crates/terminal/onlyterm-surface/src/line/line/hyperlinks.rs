use super::Line;
use crate::hyperlink::Rule;
use crate::line::linebits::LineBits;
use crate::line::storage::CellStorage;
use crate::SequenceNo;
use onlyterm_cell::Cell;

impl Line {
    /// If we have any cells with an implicit hyperlink, remove the hyperlink
    /// from the cell attributes but leave the remainder of the attributes alone.
    #[inline]
    pub fn invalidate_implicit_hyperlinks(&mut self, seqno: SequenceNo) {
        if (self.bits & (LineBits::SCANNED_IMPLICIT_HYPERLINKS | LineBits::HAS_IMPLICIT_HYPERLINKS))
            == LineBits::NONE
        {
            return;
        }

        self.bits &= !LineBits::SCANNED_IMPLICIT_HYPERLINKS;
        if (self.bits & LineBits::HAS_IMPLICIT_HYPERLINKS) == LineBits::NONE {
            return;
        }

        self.invalidate_implicit_hyperlinks_impl(seqno);
    }

    fn invalidate_implicit_hyperlinks_impl(&mut self, seqno: SequenceNo) {
        let cells = self.coerce_vec_storage();
        for cell in cells.iter_mut() {
            let replace = match cell.attrs().hyperlink() {
                Some(link) if link.is_implicit() => Some(Cell::new_grapheme(
                    cell.str(),
                    cell.attrs().clone().set_hyperlink(None).clone(),
                    None,
                )),
                _ => None,
            };
            if let Some(replace) = replace {
                *cell = replace;
            }
        }

        self.bits &= !LineBits::HAS_IMPLICIT_HYPERLINKS;
        self.update_last_change_seqno(seqno);
    }

    /// Scan through the line and look for sequences that match the provided
    /// rules.  Matching sequences are considered to be implicit hyperlinks
    /// and will have a hyperlink attribute associated with them.
    /// This function will only make changes if the line has been invalidated
    /// since the last time this function was called.
    /// This function does not remember the values of the `rules` slice, so it
    /// is the responsibility of the caller to call `invalidate_implicit_hyperlinks`
    /// if it wishes to call this function with different `rules`.
    pub fn scan_and_create_hyperlinks(&mut self, rules: &[Rule]) {
        if (self.bits & LineBits::SCANNED_IMPLICIT_HYPERLINKS)
            == LineBits::SCANNED_IMPLICIT_HYPERLINKS
        {
            // Has not changed since last time we scanned
            return;
        }

        // FIXME: let's build a string and a byte-to-cell map here, and
        // use this as an opportunity to rebuild HAS_HYPERLINK, skip matching
        // cells with existing non-implicit hyperlinks, and avoid matching
        // text with zero-width cells.
        self.bits |= LineBits::SCANNED_IMPLICIT_HYPERLINKS;
        self.bits &= !LineBits::HAS_IMPLICIT_HYPERLINKS;
        let line = self.as_str();

        let matches = Rule::match_hyperlinks(&line, rules);
        if matches.is_empty() {
            return;
        }

        let line = line.into_owned();
        let cells = self.coerce_vec_storage();
        if cells.scan_and_create_hyperlinks(&line, matches) {
            self.bits |= LineBits::HAS_IMPLICIT_HYPERLINKS;
        }
    }

    /// Scan through a logical line that is comprised of an array of
    /// physical lines and look for sequences that match the provided
    /// rules.  Matching sequences are considered to be implicit hyperlinks
    /// and will have a hyperlink attribute associated with them.
    /// This function will only make changes if the line has been invalidated
    /// since the last time this function was called.
    /// This function does not remember the values of the `rules` slice, so it
    /// is the responsibility of the caller to call `invalidate_implicit_hyperlinks`
    /// if it wishes to call this function with different `rules`.
    ///
    /// This function will call Line::clear_appdata on lines where
    /// hyperlinks are adjusted.
    pub fn apply_hyperlink_rules(rules: &[Rule], logical_line: &mut [&mut Line]) {
        if rules.is_empty() || logical_line.is_empty() {
            return;
        }

        let mut need_scan = false;
        for line in logical_line.iter() {
            if !line.bits.contains(LineBits::SCANNED_IMPLICIT_HYPERLINKS) {
                need_scan = true;
                break;
            }
        }
        if !need_scan {
            return;
        }

        let mut logical = logical_line[0].clone();
        for line in &logical_line[1..] {
            let seqno = logical.current_seqno().max(line.current_seqno());
            logical.append_line((**line).clone(), seqno);
        }
        let seq = logical.current_seqno();

        logical.invalidate_implicit_hyperlinks(seq);
        logical.scan_and_create_hyperlinks(rules);

        if !logical.has_hyperlink() {
            for line in logical_line.iter_mut() {
                line.bits.set(LineBits::SCANNED_IMPLICIT_HYPERLINKS, true);
                #[cfg(feature = "appdata")]
                line.clear_appdata();
            }
            return;
        }

        // Re-compute the physical lines that comprise this logical line
        for phys in logical_line.iter_mut() {
            let wrapped = phys.last_cell_was_wrapped();
            let is_cluster = matches!(&phys.cells, CellStorage::C(_));
            let len = phys.len();
            let remainder = logical.split_off(len, seq);
            **phys = logical;
            logical = remainder;
            phys.set_last_cell_was_wrapped(wrapped, seq);
            #[cfg(feature = "appdata")]
            phys.clear_appdata();
            if is_cluster {
                phys.compress_for_scrollback();
            }
        }
    }

    /// Returns true if the line contains a hyperlink
    #[inline]
    pub fn has_hyperlink(&self) -> bool {
        (self.bits & (LineBits::HAS_HYPERLINK | LineBits::HAS_IMPLICIT_HYPERLINKS))
            != LineBits::NONE
    }
}
