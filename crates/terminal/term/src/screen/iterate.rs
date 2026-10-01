use super::ranges::phys_intersection;
use super::*;

impl Screen {
    pub fn with_phys_lines<F>(&self, phys_range: Range<PhysRowIndex>, mut func: F)
    where
        F: FnMut(&[&Line]),
    {
        let (first, second) = self.lines.as_slices();
        let first_len = first.len();
        let first_range = 0..first.len();
        let second_range = first.len()..first.len() + second.len();
        let first_range = phys_intersection(&first_range, &phys_range);
        let second_range = phys_intersection(&second_range, &phys_range);

        let mut lines: Vec<&Line> = Vec::with_capacity(phys_range.end - phys_range.start);
        for line in &first[first_range] {
            lines.push(line);
        }
        for line in &second[second_range.start.saturating_sub(first_len)
            ..second_range.end.saturating_sub(first_len)]
        {
            lines.push(line);
        }
        func(&lines)
    }

    pub fn with_phys_lines_mut<F>(&mut self, phys_range: Range<PhysRowIndex>, mut func: F)
    where
        F: FnMut(&mut [&mut Line]),
    {
        let (first, second) = self.lines.as_mut_slices();
        let first_len = first.len();
        let first_range = 0..first.len();
        let second_range = first.len()..first.len() + second.len();
        let first_range = phys_intersection(&first_range, &phys_range);
        let second_range = phys_intersection(&second_range, &phys_range);

        let mut lines: Vec<&mut Line> = Vec::with_capacity(phys_range.end - phys_range.start);
        for line in &mut first[first_range] {
            lines.push(line);
        }
        for line in &mut second[second_range.start.saturating_sub(first_len)
            ..second_range.end.saturating_sub(first_len)]
        {
            lines.push(line);
        }
        func(&mut lines)
    }

    pub fn for_each_phys_line<F>(&self, mut f: F)
    where
        F: FnMut(usize, &Line),
    {
        for (idx, line) in self.lines.iter().enumerate() {
            f(idx, line);
        }
    }

    pub fn for_each_phys_line_mut<F>(&mut self, mut f: F)
    where
        F: FnMut(usize, &mut Line),
    {
        for (idx, line) in self.lines.iter_mut().enumerate() {
            f(idx, line);
        }
    }

    pub fn for_each_logical_line_in_stable_range_mut<F>(
        &mut self,
        stable_range: Range<StableRowIndex>,
        mut f: F,
    ) where
        F: FnMut(Range<StableRowIndex>, &mut [&mut Line]) -> bool,
    {
        let mut phys_range = self.stable_range(&stable_range);

        // Avoid pathological cases where we have eg: a really long logical line
        // (such as 1.5MB of json) that we previously wrapped.  We don't want to
        // un-wrap, scan, and re-wrap that thing.
        // This is an imperfect length constraint to partially manage the cost.
        const MAX_LOGICAL_LINE_LEN: usize = 1024;

        // Look backwards to find the start of the first logical line
        let mut back_len = 0;
        while phys_range.start > 0 {
            let prior = &mut self.lines[phys_range.start - 1];
            if !prior.last_cell_was_wrapped() {
                break;
            }
            if prior.len() + back_len > MAX_LOGICAL_LINE_LEN {
                break;
            }
            back_len += prior.len();
            phys_range.start -= 1
        }

        let mut phys_row = phys_range.start;
        while phys_row < phys_range.end {
            // Look forwards until we find the end of this logical line
            let mut total_len = 0;
            let mut end_inclusive = phys_row;

            // First pass to measure number of lines
            for idx in phys_row.. {
                if let Some(line) = self.lines.get(idx) {
                    if total_len > 0 && total_len + line.len() > MAX_LOGICAL_LINE_LEN {
                        break;
                    }
                    end_inclusive = idx;
                    total_len += line.len();
                    if !line.last_cell_was_wrapped() {
                        break;
                    }
                } else if idx == phys_row {
                    // No more rows exist
                    return;
                } else {
                    break;
                }
            }

            let phys_range = phys_row..end_inclusive + 1;

            let logical_stable_range = self.phys_to_stable_row_index(phys_row)
                ..self.phys_to_stable_row_index(end_inclusive + 1);

            phys_row = end_inclusive + 1;

            if logical_stable_range.end < stable_range.start {
                continue;
            }
            if logical_stable_range.start > stable_range.end {
                break;
            }

            let mut continue_iteration = false;
            self.with_phys_lines_mut(phys_range, |lines| {
                continue_iteration = f(logical_stable_range.clone(), lines);
            });

            if !continue_iteration {
                break;
            }
        }
    }

    /// Returns `true` if `for_each_logical_line_in_stable_range_mut` would
    /// find at least one physical line it actually visits (i.e. that
    /// passes the same in-range filter that function applies) whose
    /// `Line::implicit_hyperlinks_scanned()` bit is unset -- meaning a
    /// hyperlink-scanning walk over `stable_range` has real work to do.
    /// Returns `false` when every such line is already scanned, which lets
    /// a caller skip calling the mutating walk (and its per-logical-line
    /// `Vec<&mut Line>` allocation via `with_phys_lines_mut`) entirely.
    ///
    /// Mirrors `for_each_logical_line_in_stable_range_mut`'s backward
    /// (wrapped predecessor rows) and forward logical-line boundary
    /// extension exactly, including the `MAX_LOGICAL_LINE_LEN` pathological
    /// case guard and the `logical_stable_range` in-range filter, so this
    /// reports "no work to do" only when the real walk would genuinely
    /// invoke its callback zero times or with already-scanned lines only.
    pub fn hyperlink_scan_needed_in_stable_range(
        &self,
        stable_range: Range<StableRowIndex>,
    ) -> bool {
        let mut phys_range = self.stable_range(&stable_range);

        const MAX_LOGICAL_LINE_LEN: usize = 1024;

        let mut back_len = 0;
        while phys_range.start > 0 {
            let prior = &self.lines[phys_range.start - 1];
            if !prior.last_cell_was_wrapped() {
                break;
            }
            if prior.len() + back_len > MAX_LOGICAL_LINE_LEN {
                break;
            }
            back_len += prior.len();
            phys_range.start -= 1;
        }

        let mut phys_row = phys_range.start;
        while phys_row < phys_range.end {
            let mut total_len = 0;
            let mut end_inclusive = phys_row;
            let mut any_unscanned = false;

            for idx in phys_row.. {
                match self.lines.get(idx) {
                    Some(line) => {
                        if total_len > 0 && total_len + line.len() > MAX_LOGICAL_LINE_LEN {
                            break;
                        }
                        end_inclusive = idx;
                        total_len += line.len();
                        if !line.implicit_hyperlinks_scanned() {
                            any_unscanned = true;
                        }
                        if !line.last_cell_was_wrapped() {
                            break;
                        }
                    }
                    None => {
                        if idx == phys_row {
                            // No more rows exist.
                            return false;
                        }
                        break;
                    }
                }
            }

            let logical_stable_range = self.phys_to_stable_row_index(phys_row)
                ..self.phys_to_stable_row_index(end_inclusive + 1);

            phys_row = end_inclusive + 1;

            if logical_stable_range.end < stable_range.start {
                continue;
            }
            if logical_stable_range.start > stable_range.end {
                break;
            }

            if any_unscanned {
                return true;
            }
        }

        false
    }
}
