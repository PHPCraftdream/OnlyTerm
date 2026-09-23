//! Rule N0: resolve paired brackets.

use super::super::runs::IsolatingRunSequence;
use super::super::BidiContext;
use crate::data::bidi_brackets::{self, BracketType};
use crate::{BidiClass, Direction, Level};
use alloc::vec;
use alloc::vec::Vec;
use log::trace;

struct Pair {
    opening_pos: usize,
    closing_pos: usize,
}

impl core::fmt::Debug for Pair {
    fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(fmt, "Pair{{{},{}}}", self.opening_pos, self.closing_pos)
    }
}

const MAX_PAIRING_DEPTH: usize = 63;
struct BracketStack {
    closing_bracket: [char; MAX_PAIRING_DEPTH],
    position: [usize; MAX_PAIRING_DEPTH],
    depth: usize,
    pairs: Vec<Pair>,
}

impl core::fmt::Debug for BracketStack {
    fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
        fmt.debug_struct("BracketStack")
            .field("closing_bracket", &&self.closing_bracket[0..self.depth])
            .field("position", &&self.position[0..self.depth])
            .field("depth", &self.depth)
            .field("pairs", &self.pairs)
            .finish()
    }
}

impl BracketStack {
    pub fn new() -> Self {
        Self {
            closing_bracket: [' '; MAX_PAIRING_DEPTH],
            position: [0; MAX_PAIRING_DEPTH],
            depth: 0,
            pairs: vec![],
        }
    }

    pub fn clear(&mut self) {
        self.pairs.clear();
        self.depth = 0;
    }

    pub fn push(&mut self, closing_bracket: char, pos: usize) -> bool {
        let depth = self.depth;
        if depth >= MAX_PAIRING_DEPTH {
            return false;
        }
        self.closing_bracket[depth] = closing_bracket;
        self.position[depth] = pos;
        self.depth += 1;
        true
    }

    /// Seek an opening bracket pair for the closing bracket
    /// passed in.
    ///
    /// This is a stack based search.
    /// Start with the top element in the stack and search
    /// downwards until we either find a match or reach the
    /// bottom of the stack.
    ///
    /// If we find a match, construct and append the bracket
    /// pair to the pairList. Then pop the stack for all the
    /// levels down to the level where we found the match.
    /// (This approach is designed to discard pairs that
    /// are not cleanly nested.)
    ///
    /// If we search all the way to the bottom of the stack
    /// without finding a match, just return without changing
    /// state. This represents a closing bracket with no
    /// opening bracket to match it. Just discard and move on.
    pub fn seek_matching_open_bracket(&mut self, closing_bracket: char, pos: usize) -> bool {
        trace!(
            "seek_matching_open_bracket: closing_bracket={:?} pos={}\n{:?}",
            closing_bracket,
            pos,
            self
        );
        for depth in (0..self.depth).rev() {
            trace!("seek_matching_open_bracket: consider depth={}", depth);
            // The basic test is for the closingcp equal to the bpb value
            // stored in the bracketData. But to account for the canonical
            // equivalences for U+2329 and U+232A, tack on extra checks here
            // for the asymmetrical matches. This hard-coded check avoids
            // having to require full normalization of all the bracket code
            // points before checking. It is highly unlikely that additional
            // canonical singletons for bracket pairs will be added to future
            // versions of the UCD.
            if self.closing_bracket[depth] == closing_bracket
                || (self.closing_bracket[depth] == '\u{232a}' && closing_bracket == '\u{3009}')
                || (self.closing_bracket[depth] == '\u{3009}' && closing_bracket == '\u{232a}')
            {
                self.pairs.push(Pair {
                    opening_pos: self.position[depth],
                    closing_pos: pos,
                });
                // Pop back to this depth, pruning out any intermediates;
                // they are mismatched brackets
                self.depth = depth;
                return true;
            }
        }
        false
    }
}

fn lookup_closing(c: char) -> Option<(char, BracketType)> {
    use bidi_brackets::BIDI_BRACKETS;
    if let Ok(idx) = BIDI_BRACKETS.binary_search_by_key(&c, |&(left, _, _)| left) {
        let entry = &BIDI_BRACKETS[idx];
        return Some((entry.1, entry.2));
    }
    None
}

impl BidiContext {
    /// This is the method for Rule N0. (New in UBA63)
    /// Resolve paired brackets for a single text chain.
    ///
    /// For each character in the text chain, examine its
    /// Bidi_Class. For any character with the bpt value open or close,
    /// scan its context seeking a matching paired bracket. If found,
    /// resolve the type of both brackets to match the embedding
    /// direction.
    ///
    /// For UBA63 (and unchanged in UBA70), the error handling for
    /// a stack overflow was unspecified for this rule.
    ///
    /// Starting with UBA80, the exact stack size is specified (63),
    /// and the specification declares that if a stack overflow
    /// condition is encountered, the BD16 processing for this
    /// particular isolating run ceases immediately. This condition
    /// does not treated as a fatal error, however, so the rule
    /// should not return an error code here, which would stop
    /// all processing for *all* runs of the input string.
    pub(crate) fn resolve_paired_brackets(
        &mut self,
        iso_runs: &[IsolatingRunSequence],
        paragraph: &[char],
    ) {
        if paragraph.is_empty() {
            // BidiTest cases don't populate the paragraph, but they
            // also don't contain any bracket related tests either,
            // so we have nothing to do here.
            return;
        }

        let mut stack = BracketStack::new();
        for iso_run in iso_runs {
            stack.clear();
            for (ridx, &cidx) in iso_run.indices.iter().enumerate() {
                if let Some((closing_bracket, bpt)) = lookup_closing(paragraph[cidx]) {
                    trace!("ridx={} cidx={} {:?} bracket", ridx, cidx, paragraph[cidx]);
                    if self.char_types[cidx] == BidiClass::OtherNeutral {
                        if bpt == BracketType::Open {
                            trace!("push open ridx={}", ridx);
                            if !stack.push(closing_bracket, ridx) {
                                // Stack overflow: halt processing
                                return;
                            }
                        } else {
                            // a closing bracket
                            trace!("close at ridx={}, search for opener", ridx);
                            stack.seek_matching_open_bracket(paragraph[cidx], ridx);
                        }
                    }
                }
            }

            if stack.pairs.is_empty() {
                // The pairList pointer will still be NULL if no paired brackets
                // were found. In this case, no further processing is necessary.
                continue;
            }

            // Because of the way the stack
            // processing works, the pairs may not be in the best order
            // in the pair list for further processing. Sort them
            // by position order of the opening bracket.
            stack.pairs.sort_unstable_by_key(|p| p.opening_pos);
            trace!("\nPairs: {:?}", stack.pairs);

            for pair in &stack.pairs {
                // Now for each pair, we have the first and last position
                // of the substring in this isolating run sequence
                // enclosed by those brackets (inclusive
                // of the brackets). Resolve that individual pair.
                self.resolve_one_pair(pair, iso_run);
            }
        }
    }

    /// Set the Bidi_Class of a bracket pair, based on the
    /// direction determined by the N0 rule processing in
    /// br_ResolveOnePair().
    ///
    /// The direction passed in will either be BIDI_R or BIDI_L.
    ///
    /// This setting is abstracted in a function here, rather than
    /// simply being done inline, because of
    /// an edge case added to rule N0 as of UBA80. For UBA63 (and
    /// UBA70), no special handling of combining marks following
    /// either of the brackets is done. However, starting with UBA80,
    /// there is an edge case fix-up done which echoes the processing
    /// of rule W1. The text run needs to be scanned to find any
    /// combining marks (orig_bc=NSM) following a bracket which has
    /// its Bidi_Class changed by N0. Then those combining marks
    /// can again be adjusted to match the Bidi_Class of the
    /// bracket they apply to. This is an odd edge case, as combining
    /// marks do not typically occur with brackets, but the UBA80
    /// specification is now explicit about requiring this fix-up
    /// to be done.
    fn set_bracket_pair_bc(
        pair: &Pair,
        indices: &[usize],
        direction: Direction,
        char_types: &mut [BidiClass],
        orig_char_types: &[BidiClass],
        levels: &[Level],
    ) {
        let opening_pos = indices[pair.opening_pos];
        let closing_pos = indices[pair.closing_pos];
        let bc = match direction {
            Direction::LeftToRight => BidiClass::LeftToRight,
            Direction::RightToLeft => BidiClass::RightToLeft,
        };
        trace!(
            "set_bracket_pair_bc index={} from {:?} -> {:?}",
            opening_pos,
            char_types[opening_pos],
            bc
        );
        trace!(
            "set_bracket_pair_bc index={} from {:?} -> {:?}",
            closing_pos,
            char_types[closing_pos],
            bc
        );
        char_types[opening_pos] = bc;
        char_types[closing_pos] = bc;

        // Here is the tricky part.
        //
        // First scan from the opening bracket for any subsequent
        // character whose *original* Bidi_Class was NSM, and set
        // the current bc for it to direction also, to match the bracket.
        // Break out of the loop at the first character with any other
        // original Bidi_Class, so that this change only impacts
        // actual combining mark sequences.
        //
        // 2020-03-27 note: This scanning for original combining marks
        // must also scan past any intervening NO_LEVEL characters,
        // typically bc=BN characters removed earlier by rule X9.
        // Such sequences may, for example involve a ZWJ or ZWNJ,
        // or in bizarre edge cases involve other bc=BN characters
        // such as ZWSP. The latter would be defective combining character
        // sequences, but also need to be handled here.
        //
        // Then repeat the process for the matching closing bracket.
        //
        // The processing for the opening bracket is bounded to the
        // right by the position of the matching closing bracket.
        // The processing for the closing bracket is bounded to the
        // right by the end of the text run.
        for &cidx in &indices[pair.opening_pos + 1..pair.closing_pos] {
            if orig_char_types[cidx] == BidiClass::NonspacingMark {
                char_types[cidx] = bc;
            } else if !levels[cidx].removed_by_x9() {
                break;
            }
        }
        for &cidx in &indices[pair.closing_pos + 1..] {
            if orig_char_types[cidx] == BidiClass::NonspacingMark {
                char_types[cidx] = bc;
            } else if !levels[cidx].removed_by_x9() {
                break;
            }
        }
    }

    /// Resolve the embedding levels of one pair of matched brackets.
    ///
    /// This determination is based on the embedding direction.
    /// See BD3 in the UBA specification.
    ///
    /// If embedding level is even, embedding direction = L.
    /// If embedding level is odd,  embedding direction = R.
    fn resolve_one_pair(&mut self, pair: &Pair, iso_run: &IsolatingRunSequence) {
        let embedding_direction = iso_run.level.direction();
        let opposite_direction = embedding_direction.opposite();

        let mut strong_type_found = false;
        // Next check for a strong type (R or L)
        // between the matched brackets. If a strong type is found
        // which matches the embedding direction, then set the type of both
        // brackets to match the embedding direction, too.
        if pair.opening_pos < pair.closing_pos.saturating_sub(1) {
            trace!("pair: {:?}", pair);
            for &cidx in &iso_run.indices[pair.opening_pos + 1..pair.closing_pos] {
                let direction = match self.char_types[cidx] {
                    BidiClass::RightToLeft
                    | BidiClass::EuropeanNumber
                    | BidiClass::ArabicNumber => Some(Direction::RightToLeft),
                    BidiClass::LeftToRight => Some(Direction::LeftToRight),
                    _ => None,
                };

                if direction == Some(embedding_direction) {
                    // N0 step b
                    trace!("Strong direction e between brackets");
                    Self::set_bracket_pair_bc(
                        pair,
                        &iso_run.indices,
                        embedding_direction,
                        &mut self.char_types,
                        &self.orig_char_types,
                        &self.levels,
                    );
                    return;
                } else if direction == Some(opposite_direction) {
                    strong_type_found = true;
                }
            }
        }

        if strong_type_found {
            // First attempt to resolve direction by checking the prior context for
            // a strong type matching the opposite direction. N0 Step c1.
            if (opposite_direction == Direction::LeftToRight
                && self.is_prior_context_left(pair.opening_pos, &iso_run.indices, iso_run.sos))
                || (opposite_direction == Direction::RightToLeft
                    && self.is_prior_context_right(pair.opening_pos, &iso_run.indices, iso_run.sos))
            {
                Self::set_bracket_pair_bc(
                    pair,
                    &iso_run.indices,
                    opposite_direction,
                    &mut self.char_types,
                    &self.orig_char_types,
                    &self.levels,
                );
            } else {
                // No strong type matching the oppositedirection was found either
                // before or after these brackets in this text chain. Resolve the
                // brackets based on the embedding direction. N0 Step c2.
                Self::set_bracket_pair_bc(
                    pair,
                    &iso_run.indices,
                    embedding_direction,
                    &mut self.char_types,
                    &self.orig_char_types,
                    &self.levels,
                );
            }
        } else {
            // No strong type was found between the brackets. Leave
            // the brackets with unresolved direction.
        }
    }
}

#[cfg(test)]
mod tests {
    use super::lookup_closing;
    use crate::data::bidi_brackets::BracketType;
    use alloc::format;
    use k9::assert_equal as assert_eq;

    #[test]
    fn mirror() {
        assert_eq!(lookup_closing('{'), Some(('}', BracketType::Open)));
        assert_eq!(lookup_closing('['), Some((']', BracketType::Open)));
        assert_eq!(lookup_closing(']'), Some(('[', BracketType::Close)));
    }
}
