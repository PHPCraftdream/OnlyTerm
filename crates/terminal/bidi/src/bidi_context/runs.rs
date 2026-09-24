//! Rule X10 and BD13: identify level runs and assemble them into
//! isolating run sequences, plus the data types used to describe them.

use super::BidiContext;
use crate::{BidiClass, BidiRun, Level, NO_LEVEL};
use alloc::borrow::Cow;
use alloc::vec;
use alloc::vec::Vec;
use core::ops::Range;
use log::trace;

#[derive(Debug)]
pub(crate) struct RunIter<'a> {
    pub(crate) pos: usize,
    pub(crate) levels: Cow<'a, [Level]>,
    pub(crate) line_range: Range<usize>,
}

impl<'a> Iterator for RunIter<'a> {
    type Item = BidiRun;

    fn next(&mut self) -> Option<BidiRun> {
        loop {
            if self.pos >= self.levels.len() {
                return None;
            }

            let start = self.pos;
            let len = span_len(start, &self.levels);
            self.pos += len;

            let level = self.levels[start];
            if !level.removed_by_x9() {
                let range = start..start + len;

                let mut removed_by_x9 = vec![];
                for idx in range.clone() {
                    if self.levels[idx].removed_by_x9() {
                        removed_by_x9.push(idx + self.line_range.start);
                    }
                }

                return Some(BidiRun {
                    direction: level.direction(),
                    level,
                    range: self.line_range.start + range.start..self.line_range.start + range.end,
                    removed_by_x9,
                });
            }
        }
    }
}

pub(crate) fn span_len(start: usize, levels: &[Level]) -> usize {
    let starting_level = levels[start];
    levels
        .iter()
        .skip(start + 1)
        .position(|&l| l != starting_level)
        .unwrap_or(levels.len() - (start + 1))
        + 1
}

impl BidiContext {
    /// X10
    pub(crate) fn identify_runs(&mut self) {
        let mut idx = 0;
        let len = self.char_types.len();
        self.runs.clear();

        while idx < len {
            let (span_level, span_len) = span_one_run(&self.char_types, &self.levels, idx);
            if !span_level.removed_by_x9() {
                self.runs.push(Run {
                    start: idx,
                    end: idx + span_len,
                    len: span_len,
                    seq_id: 0,
                    level: span_level,
                    sor: BidiClass::OtherNeutral,
                    eor: BidiClass::OtherNeutral,
                });
            }

            assert!(span_len > 0);
            idx += span_len;
        }

        self.calculate_sor_eor();

        trace!("\nRuns: {:#?}", self.runs);
    }

    fn calculate_sor_eor(&mut self) {
        let mut prior_run_level = self.base_level;
        let mut iter = self.runs.iter_mut().peekable();
        while let Some(run) = iter.next() {
            let next_run_level = match iter.peek() {
                Some(next) => next.level,
                None => self.base_level,
            };

            // Set sor based on the higher of the prior_run_level and the current level.
            run.sor = prior_run_level.max(run.level).as_bidi_class();
            run.eor = next_run_level.max(run.level).as_bidi_class();

            prior_run_level = run.level;
        }
    }

    /// This function applies only to UBA63. Once the embedding
    /// levels are identified, UBA63 requires further processing
    /// to assign each of the level runs to an isolating run sequence.
    ///
    /// Each level run must be uniquely assigned to exactly one
    /// isolating run sequence. Each isolating run sequence must
    /// have at least one level run, but may have more.
    ///
    /// The exact details on how to match up isolating run sequences
    /// with level runs are specified in BD13.
    ///
    /// The strategy taken here is to scan the level runs in order.
    ///
    /// If a level run is not yet assigned to an isolating run sequence,
    /// its seqID will be zero. Create a new isolating run sequence
    /// and add this level run to it.
    ///
    /// If the last BIDIUNIT of *this* level run is an isolate
    /// initiator (LRI/RLI/FSI), then scan ahead in the list of
    /// level runs seeking the next level run which meets the
    /// following criteria:
    ///   1. seqID = 0 (not yet assigned to an isolating run sequence)
    ///   2. its level matches the level we are processing
    ///   3. the first BIDIUNIT is a PDI
    ///
    /// If all those conditions are met, assign that next level run
    /// to this isolating run sequence (set its seqID, and append to
    /// the list).
    ///
    /// Repeat until we hit a level run that doesn't terminate with
    /// an isolate initiator or we hit the end of the list of level
    /// runs.
    ///
    /// That terminates the definition of the isolating run sequence
    /// we are working on. Append it to the list of isolating run
    /// sequences in the UBACONTEXT.
    ///
    /// Then advance to the next level run which has not yet been
    /// assigned to an isolating run sequence and repeat the process.
    ///
    /// Continue until all level runs have been assigned to an
    /// isolating run sequence.
    pub(crate) fn identify_isolating_run_sequences(&mut self) -> Vec<IsolatingRunSequence> {
        let mut seq_id = 0;
        let mut iso_runs = vec![];
        let num_runs = self.runs.len();

        for run_idx in 0..num_runs {
            let save_level;

            {
                let run = &mut self.runs[run_idx];
                if run.seq_id != 0 {
                    continue;
                }
                seq_id += 1;
                iso_runs.push(Self::new_iso_run_seq(run_idx, run));
                run.seq_id = seq_id;

                if !self.char_types[run.end - 1].is_iso_init() {
                    continue;
                }
                save_level = run.level;
            }

            // Look ahead to find the run with the corresponding
            // PopDirectionalIsolate
            for idx in run_idx + 1..num_runs {
                let run = &mut self.runs[idx];
                if run.seq_id == 0
                    && run.level == save_level
                    && run.first_significant_bidi_class(&self.char_types, &self.levels)
                        == Some(BidiClass::PopDirectionalIsolate)
                {
                    // we matched the criteria for adding this run to the sequence.
                    let iso_run = iso_runs.last_mut().unwrap();
                    iso_run.runs.push(idx);
                    iso_run.len += run.len;
                    run.seq_id = seq_id;

                    // Check if the last char in this run is also an
                    // isolate initiator. If not, this sequence is done.
                    if !self.char_types[run.end - 1].is_iso_init() {
                        break;
                    }
                }
            }
        }
        self.calculate_sos_eos(&mut iso_runs);
        self.build_text_chains(&mut iso_runs);
        iso_runs
    }

    /// In order to simplify later rule processing, assemble the indices
    /// of the characters in the isolating runs so that there is just a
    /// single list to iterate
    fn build_text_chains(&mut self, iso_runs: &mut [IsolatingRunSequence]) {
        for iso_run in iso_runs {
            for &run_idx in &iso_run.runs {
                let run = &self.runs[run_idx];
                iso_run.indices.extend(run.start..run.end);
            }
        }
    }

    /// Process the isolating run sequence list, calculating sos and eos values for
    /// each sequence. Each needs to be set to either L or R.
    ///
    /// Strategy: Instead of recalculating all the sos and eos values from
    /// scratch, as specified in X10, we can take a shortcut here, because
    /// we already have sor and eor values assigned to all the level runs.
    /// For any isolating run sequence, simply assign sos to the value of
    /// sor for the *first* run in that sequence, and assign eos to the
    /// value of eor for the *last* run in that sequence. This provides
    /// equivalent values, and is more straightforward to implement and
    /// understand.
    ///
    /// This strategy has to be modified for defective isolating run sequences,
    /// where the sequence ends with an LRI/RLI/FSI.
    /// In those cases the eot needs to be calculated based on
    /// the paragraph embedding level, rather than from the level run.
    /// Note that this only applies when an isolating run sequence
    /// terminating in an LRI/RLI/FSI but with no matching PDI.
    /// An example would be:
    ///
    ///    R  RLI    R
    /// <L-----R> <RR>
    /// <L------[          <== eot would be L, not R
    ///           <RR>
    ///
    fn calculate_sos_eos(&mut self, iso_runs: &mut [IsolatingRunSequence]) {
        for iso_run in iso_runs {
            // First inherit the sos and eos values from the
            // first and last runs in the sequence.
            let first_run_idx = iso_run.runs.first().cloned().expect("at least 1 run");
            let last_run_idx = iso_run.runs.last().cloned().expect("at least 1 run");
            iso_run.sos = self.runs[first_run_idx].sor;
            iso_run.eos = self.runs[last_run_idx].eor;
            // Next adjust for the special case when an isolating
            // run sequence terminates in an unmatched isolate
            // initiator.
            if self.char_types[self.runs[last_run_idx].end - 1].is_iso_init() {
                let higher_level = self.base_level.max(iso_run.level);
                iso_run.eos = higher_level.as_bidi_class();
            }
        }
    }

    fn new_iso_run_seq(run_idx: usize, run: &Run) -> IsolatingRunSequence {
        let len = run.len;
        let level = run.level;
        IsolatingRunSequence {
            runs: vec![run_idx],
            len,
            level,
            sos: BidiClass::OtherNeutral,
            eos: BidiClass::OtherNeutral,
            indices: vec![],
        }
    }
}

#[derive(Debug)]
pub(crate) struct Run {
    /// char indices for start, end of run
    pub(crate) start: usize,
    pub(crate) end: usize,
    /// length of run
    pub(crate) len: usize,
    /// isolating run sequence id
    pub(crate) seq_id: usize,
    /// Embedding level of this run
    pub(crate) level: Level,
    /// Direction of start of run
    pub(crate) sor: BidiClass,
    /// Direction of end of run
    pub(crate) eor: BidiClass,
}

impl Run {
    // The index is used to index both `levels` and `types`; this is bidi
    // isolating-run identification logic, kept as a range loop for clarity.
    #[allow(clippy::needless_range_loop)]
    fn first_significant_bidi_class(
        &self,
        types: &[BidiClass],
        levels: &[Level],
    ) -> Option<BidiClass> {
        for idx in self.start..self.end {
            if !levels[idx].removed_by_x9() {
                return types.get(idx).cloned();
            }
        }
        None
    }
}

#[derive(Debug)]
pub(crate) struct IsolatingRunSequence {
    /// List of the runs in this sequence. The values are indices
    /// into the runs array
    pub(crate) runs: Vec<usize>,
    /// length of the run
    pub(crate) len: usize,
    /// Embedding level of this run
    pub(crate) level: Level,
    /// Direction of start of run
    pub(crate) sos: BidiClass,
    /// Direction of end of run
    pub(crate) eos: BidiClass,
    /// The sequence of indices into the original paragraph,
    /// across the contained set of runs
    pub(crate) indices: Vec<usize>,
}

/// Starting from `start`, extract the first run containing characters
/// all with the same level
fn span_one_run(types: &[BidiClass], levels: &[Level], start: usize) -> (Level, usize) {
    let mut span_level = Level(NO_LEVEL);
    let mut isolate_init_found = false;
    let mut span_len = 0;

    trace!(
        "span_one_run called with types: {:?}, levels: {:?}, start={}",
        types,
        levels,
        start
    );

    for (idx, (bc, level)) in types
        .iter()
        .skip(start)
        .zip(levels.iter().skip(start))
        .enumerate()
    {
        trace!(
            "span_one_run: consider idx={} bc={:?} level={:?}",
            idx,
            bc,
            level
        );
        if !level.removed_by_x9() {
            if bc.is_iso_init() {
                isolate_init_found = true;
            }
            if span_level.removed_by_x9() {
                span_level = *level;
            } else if *level != span_level {
                // End of run
                break;
            }
        }
        span_len = idx;
        if isolate_init_found {
            break;
        }
    }

    (span_level, span_len + 1)
}
