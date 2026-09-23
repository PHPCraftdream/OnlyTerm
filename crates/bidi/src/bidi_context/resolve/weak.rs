//! Rules W1 through W7: resolve weak types (combining marks, European and
//! Arabic numbers, separators and terminators).

use super::super::runs::IsolatingRunSequence;
use super::super::BidiContext;
use crate::BidiClass;

impl BidiContext {
    /// This is the method for Rule W1.
    ///
    /// Resolve combining marks for a single text chain.
    ///
    /// For each character in the text chain, examine its
    /// Bidi_Class. For characters of bc=NSM, change the Bidi_Class
    /// value to that of the preceding character. Formatting characters
    /// (Bidi_Class RLE, LRE, RLO, LRO, PDF) and boundary neutral (Bidi_Class BN)
    /// are skipped over in this calculation, because they have been
    /// "deleted" by Rule X9.
    ///
    /// If a bc=NSM character occurs at the start of a text chain, it is given
    /// the Bidi_Class of sot (either R or L).
    pub(crate) fn resolve_combining_marks(&mut self, iso_runs: &[IsolatingRunSequence]) {
        for iso_run in iso_runs {
            let mut prior_bc = iso_run.sos;
            for &idx in &iso_run.indices {
                if self.char_types[idx] == BidiClass::NonspacingMark {
                    self.char_types[idx] = prior_bc;
                } else if !self.levels[idx].removed_by_x9() {
                    prior_bc = self.char_types[idx];
                }
            }
        }
    }

    /// This is the method for Rule W2.
    ///
    /// Resolve European numbers for a single text chain.
    ///
    /// For each character in the text chain, examine its
    /// Bidi_Class. For characters of bc=EN, scan back to find the first
    /// character of strong type (or sot). If the strong type is bc=AL,
    /// change the Bidi_Class EN to AN. Formatting characters
    /// (Bidi_Class RLE, LRE, RLO, LRO, PDF) and boundary neutral (Bidi_Class BN)
    /// are skipped over in this calculation, because they have been
    /// "deleted" by Rule X9.
    pub(crate) fn resolve_european_numbers(&mut self, iso_runs: &[IsolatingRunSequence]) {
        for iso_run in iso_runs {
            for (ridx, &cidx) in iso_run.indices.iter().enumerate() {
                if self.char_types[cidx] == BidiClass::EuropeanNumber {
                    // Scan backwards to find the first strong type
                    let mut first_strong_bc = iso_run.sos;

                    if ridx > 0 {
                        for &pidx in iso_run.indices.get(0..ridx).unwrap().iter().rev() {
                            match self.char_types[pidx] {
                                bc @ BidiClass::LeftToRight
                                | bc @ BidiClass::RightToLeft
                                | bc @ BidiClass::ArabicLetter => {
                                    first_strong_bc = bc;
                                    break;
                                }
                                _ => {}
                            }
                        }
                    }

                    // Check if the first strong type is AL. If so
                    // reset this EN to AN.
                    if first_strong_bc == BidiClass::ArabicLetter {
                        self.char_types[cidx] = BidiClass::ArabicNumber;
                    }
                }
            }
        }
    }

    /// This is the method for Rule W3.
    ///
    /// Resolve Bidi_Class=AL for a single text chain.
    ///
    /// For each character in the text chain, examine its
    /// Bidi_Class. For characters of bc=AL, change the Bidi_Class
    /// value to R.
    pub(crate) fn resolve_arabic_letters(&mut self, iso_runs: &[IsolatingRunSequence]) {
        for iso_run in iso_runs {
            for &idx in &iso_run.indices {
                if self.char_types[idx] == BidiClass::ArabicLetter {
                    self.char_types[idx] = BidiClass::RightToLeft;
                }
            }
        }
    }

    /// Look back ahead of `index_idx` and return true if the
    /// bidi class == bc.  However, skip backwards over entries
    /// that were removed by X9; they will have NO_LEVEL.
    /// Returns the char index of the match.
    fn is_prior_context(
        &self,
        index_idx: usize,
        indices: &[usize],
        bc: BidiClass,
    ) -> Option<usize> {
        if index_idx == 0 {
            return None;
        }
        for &idx in indices[0..index_idx].iter().rev() {
            if self.char_types[idx] == bc {
                return Some(idx);
            }
            if !self.levels[idx].removed_by_x9() {
                break;
            }
        }
        None
    }

    /// Look ahead of `index_idx` and return true if the
    /// bidi class == bc.  However, skip over entries
    /// that were removed by X9; they will have NO_LEVEL.
    /// Returns the char index of the match.
    fn is_following_context(
        &self,
        index_idx: usize,
        indices: &[usize],
        bc: BidiClass,
    ) -> Option<usize> {
        for &idx in &indices[index_idx + 1..] {
            if self.char_types[idx] == bc {
                return Some(idx);
            }
            if !self.levels[idx].removed_by_x9() {
                break;
            }
        }
        None
    }

    fn is_in_context(&self, index_idx: usize, indices: &[usize], bc: BidiClass) -> bool {
        self.is_prior_context(index_idx, indices, bc).is_some()
            && self.is_following_context(index_idx, indices, bc).is_some()
    }

    /// This is the method for Rule W4.
    ///
    /// Resolve Bidi_Class=ES and CS for a single text chain.
    ///
    /// For each character in the text chain, examine its
    /// Bidi_Class.
    ///
    /// For characters of bc=ES, check if they are *between* EN.
    /// If so, change their Bidi_Class to EN.
    ///
    /// For characters of bc=CS, check if they are *between* EN
    /// or between AN. If so, change their Bidi_Class to match.
    ///
    pub(crate) fn resolve_separators(&mut self, iso_runs: &[IsolatingRunSequence]) {
        for iso_run in iso_runs {
            for (index_idx, &idx) in iso_run.indices.iter().enumerate() {
                if self.char_types[idx] == BidiClass::EuropeanSeparator {
                    if self.is_in_context(index_idx, &iso_run.indices, BidiClass::EuropeanNumber) {
                        self.char_types[idx] = BidiClass::EuropeanNumber;
                    }
                } else if self.char_types[idx] == BidiClass::CommonSeparator {
                    if self.is_in_context(index_idx, &iso_run.indices, BidiClass::EuropeanNumber) {
                        self.char_types[idx] = BidiClass::EuropeanNumber;
                    } else if self.is_in_context(
                        index_idx,
                        &iso_run.indices,
                        BidiClass::ArabicNumber,
                    ) {
                        self.char_types[idx] = BidiClass::ArabicNumber;
                    }
                }
            }
        }
    }

    /// This is the method for Rule W5.
    ///
    /// Resolve Bidi_Class=ET for a single text chain.
    ///
    /// For each character in the text chain, examine its
    /// Bidi_Class.
    ///
    /// For characters of bc=ET, check if they are *next to* EN.
    /// If so, change their Bidi_Class to EN. This includes
    /// ET on either side of EN, so the context on both sides
    /// needs to be checked.
    ///
    /// Because this rule applies to indefinite sequences of ET,
    /// and because the context which triggers any change is
    /// adjacency to EN, the strategy taken here is to seek for
    /// EN first. If found, scan backwards, changing any eligible
    /// ET to EN. Then scan forwards, changing any eligible ET
    /// to EN. Then continue the search from the point of the
    /// last ET changed (if any).
    ///
    pub(crate) fn resolve_terminators(&mut self, iso_runs: &[IsolatingRunSequence]) {
        for iso_run in iso_runs {
            for (index_idx, &idx) in iso_run.indices.iter().enumerate() {
                if self.char_types[idx] == BidiClass::EuropeanNumber {
                    for &prior_idx in iso_run.indices[0..index_idx].iter().rev() {
                        if self.char_types[prior_idx] == BidiClass::EuropeanTerminator {
                            self.char_types[prior_idx] = BidiClass::EuropeanNumber;
                        } else if !self.levels[prior_idx].removed_by_x9() {
                            break;
                        }
                    }
                    for &next_idx in &iso_run.indices[index_idx + 1..] {
                        if self.char_types[next_idx] == BidiClass::EuropeanTerminator {
                            self.char_types[next_idx] = BidiClass::EuropeanNumber;
                        } else if !self.levels[next_idx].removed_by_x9() {
                            break;
                        }
                    }
                }
            }
        }
    }

    /// This is the method for Rule W6.
    ///
    /// Resolve remaining Bidi_Class=ES, CS, or ET for a single text chain.
    ///
    /// For each character in the text chain, examine its
    /// Bidi_Class. For characters of bc=ES, bc=CS, or bc=ET, change
    /// the Bidi_Class value to ON. This resolves any remaining
    /// separators or terminators which were not already processed
    /// by Rules W4 and W5.
    pub(crate) fn resolve_es_cs_et(&mut self, iso_runs: &[IsolatingRunSequence]) {
        for iso_run in iso_runs {
            for &idx in &iso_run.indices {
                match self.char_types[idx] {
                    BidiClass::EuropeanSeparator
                    | BidiClass::CommonSeparator
                    | BidiClass::EuropeanTerminator => {
                        self.char_types[idx] = BidiClass::OtherNeutral;
                    }
                    _ => {}
                }
            }
        }
    }

    /// This is the method for Rule W7.
    ///
    /// Resolve Bidi_Class=EN for a single level text chain.
    ///
    /// Process the text chain in reverse order. For each character in the text chain, examine its
    /// Bidi_Class. For characters of bc=EN, scan back to find the first strong
    /// directional type. If that type is L, change the Bidi_Class
    /// value of the number to L.
    pub(crate) fn resolve_en(&mut self, iso_runs: &[IsolatingRunSequence]) {
        for iso_run in iso_runs {
            for (ridx, &cidx) in iso_run.indices.iter().enumerate().rev() {
                if self.char_types[cidx] == BidiClass::EuropeanNumber {
                    // Scan backwards to find the first strong type
                    let mut first_strong_bc = iso_run.sos;

                    if ridx > 0 {
                        for &pidx in iso_run.indices.get(0..ridx).unwrap().iter().rev() {
                            match self.char_types[pidx] {
                                bc @ BidiClass::LeftToRight | bc @ BidiClass::RightToLeft => {
                                    first_strong_bc = bc;
                                    break;
                                }
                                _ => {}
                            }
                        }
                    }

                    if first_strong_bc == BidiClass::LeftToRight {
                        self.char_types[cidx] = BidiClass::LeftToRight;
                    }
                }
            }
        }
    }
}
