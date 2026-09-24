//! Rules N1 and N2: resolve neutrals, first by context and then by
//! embedding level.

use super::super::runs::IsolatingRunSequence;
use super::super::BidiContext;
use crate::BidiClass;
use log::trace;

impl BidiContext {
    /// This is the method for Rule N1.
    ///
    /// Resolve neutrals by context for a single text chain.
    ///
    /// For each character in the text chain, examine its
    /// Bidi_Class. For any character of neutral type, examine its
    /// context.
    ///
    /// L N L --> L L L
    /// R N R --> R R R [note that AN and EN count as R for this rule]
    ///
    /// Here "N" stands for "any sequence of neutrals", so the neutral
    /// does not have to be immediately adjacent to a strong type
    /// to be resolved this way.
    pub(crate) fn resolve_neutrals_by_context(&mut self, iso_runs: &[IsolatingRunSequence]) {
        for iso_run in iso_runs {
            for (ridx, &cidx) in iso_run.indices.iter().enumerate().rev() {
                if !self.char_types[cidx].is_neutral() {
                    continue;
                }

                if self.is_prior_context_left(ridx, &iso_run.indices, iso_run.sos)
                    && self.is_following_context_left(ridx, &iso_run.indices, iso_run.eos)
                {
                    trace!(
                        "ridx={} cidx={} was {:?}, setting to LeftToRight",
                        ridx,
                        cidx,
                        self.char_types[cidx]
                    );
                    self.char_types[cidx] = BidiClass::LeftToRight;
                } else if self.is_prior_context_right(ridx, &iso_run.indices, iso_run.sos)
                    && self.is_following_context_right(ridx, &iso_run.indices, iso_run.eos)
                {
                    trace!(
                        "ridx={} cidx={} was {:?}, setting to RightToLeft",
                        ridx,
                        cidx,
                        self.char_types[cidx]
                    );
                    self.char_types[cidx] = BidiClass::RightToLeft;
                }
            }
        }
    }

    /// Scan backwards in a text chain, checking if the first non-neutral character
    /// is an "L" type.  Skip over any "deleted" controls, which have NO_LEVEL,
    /// as well as any neutral types.
    pub(crate) fn is_prior_context_left(
        &self,
        index_idx: usize,
        indices: &[usize],
        sot: BidiClass,
    ) -> bool {
        if index_idx == 0 {
            trace!(
                "is_prior_context_left: short circuit because index_idx=0. sot is {:?}",
                sot
            );
            return sot == BidiClass::LeftToRight;
        }
        for &idx in indices[0..index_idx].iter().rev() {
            trace!(
                "is_prior_context_left considering idx={} {:?}",
                idx,
                self.char_types[idx]
            );
            if self.char_types[idx] == BidiClass::LeftToRight {
                return true;
            }
            if self.levels[idx].removed_by_x9() {
                continue;
            }
            if self.char_types[idx].is_neutral() {
                continue;
            }
            return false;
        }
        sot == BidiClass::LeftToRight
    }

    /// Scan forwards in a text chain, checking if the first non-neutral character is an "L" type.
    /// Skip over any "deleted" controls, which have NO_LEVEL, as well as any neutral types.
    fn is_following_context_left(
        &self,
        index_idx: usize,
        indices: &[usize],
        eot: BidiClass,
    ) -> bool {
        trace!(
            "is_following_context_left index_idx={} vs. len {}",
            index_idx,
            indices.len()
        );
        for &idx in &indices[index_idx + 1..] {
            if self.char_types[idx] == BidiClass::LeftToRight {
                trace!("is_following_context_left true because idx={} is left", idx);
                return true;
            }
            if self.levels[idx].removed_by_x9() {
                continue;
            }
            if self.char_types[idx].is_neutral() {
                continue;
            }
            return false;
        }
        trace!(
            "is_following_context_left fall through to bottom, check against eot={:?}",
            eot
        );
        eot == BidiClass::LeftToRight
    }

    /// Used by Rule N1.
    ///
    /// Scan backwards in a text chain, checking if the first non-neutral character is an "R" type.
    /// (BIDI_R, BIDI_AN, BIDI_EN) Skip over any "deleted" controls, which
    /// have NO_LEVEL, as well as any neutral types.
    pub(crate) fn is_prior_context_right(
        &self,
        index_idx: usize,
        indices: &[usize],
        sot: BidiClass,
    ) -> bool {
        if index_idx == 0 {
            return sot == BidiClass::RightToLeft;
        }
        for &idx in indices[0..index_idx].iter().rev() {
            match self.char_types[idx] {
                BidiClass::RightToLeft | BidiClass::ArabicNumber | BidiClass::EuropeanNumber => {
                    return true;
                }
                _ => {}
            }
            if self.levels[idx].removed_by_x9() {
                continue;
            }
            if self.char_types[idx].is_neutral() {
                continue;
            }
            return false;
        }
        sot == BidiClass::RightToLeft
    }

    fn is_following_context_right(
        &self,
        index_idx: usize,
        indices: &[usize],
        eot: BidiClass,
    ) -> bool {
        for &idx in &indices[index_idx + 1..] {
            match self.char_types[idx] {
                BidiClass::RightToLeft | BidiClass::ArabicNumber | BidiClass::EuropeanNumber => {
                    return true;
                }
                _ => {}
            }
            if self.levels[idx].removed_by_x9() {
                continue;
            }
            if self.char_types[idx].is_neutral() {
                continue;
            }
            return false;
        }
        eot == BidiClass::RightToLeft
    }

    /// This is the method for Rule N2.
    ///
    /// Resolve neutrals by level for a single text chain.
    ///
    /// For each character in the text chain, examine its
    /// Bidi_Class. For any character of neutral type, examine its
    /// embedding level and resolve accordingly.
    ///
    /// N --> e
    /// where e = L for an even level, R for an odd level
    pub(crate) fn resolve_neutrals_by_level(&mut self, iso_runs: &[IsolatingRunSequence]) {
        for iso_run in iso_runs {
            for &cidx in iso_run.indices.iter().rev() {
                if self.char_types[cidx].is_neutral() {
                    self.char_types[cidx] = self.levels[cidx].as_bidi_class();
                }
            }
        }
    }
}
