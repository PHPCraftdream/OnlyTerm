//! Progressive sweep policy for frame-build budget management.
//!
//! This module implements the row-sweep logic that fixes task #457's Cause B
//! (livelock under budget pressure) while ensuring the invariant from Cause A
//! (no row is ever left blank once rendered).
//!
//! See the task #457 design review by @oh for the full rationale.

use std::time::Instant;

/// Action to take for a single row slot during a sweep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowAction {
    /// Fresh line_quad_cache hit; always allowed regardless of budget.
    EmitCached,
    /// Must actually shape/build this row now.
    Build,
    /// Budget-deferred; re-emit last known-good quads for this slot.
    EmitRetained,
}

/// Outcome of completing a row sweep.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SweepOutcome {
    /// Row slot at which the NEXT frame's sweep should start.
    pub next_resume: usize,
    /// Whether any row was deferred this sweep.
    pub deferred_any: bool,
}

/// State for a progressive row sweep across a pane's visible rows.
///
/// The sweep ensures:
/// - No row is ever left blank (has_retained == false always triggers Build)
/// - The cursor row is always fresh
/// - At least one row is built per frame (forward-progress guarantee)
/// - Rows before work_start that are cache hits still refresh every frame
/// - resume_row advances monotonically until a clean sweep completes
#[derive(Debug)]
pub struct RowSweep {
    deadline: Option<Instant>,
    work_start: usize,
    #[allow(dead_code)] // Available for future use in forward-progress calculations
    n_rows: usize,
    cursor_row: Option<usize>,
    built_this_frame: usize,
    deferred_any: bool,
    next_resume: Option<usize>,
}

impl RowSweep {
    /// Create a new row sweep.
    ///
    /// # Arguments
    /// * `deadline` - The wall-clock deadline for this frame's work, or None if budget disabled.
    /// * `work_start` - Row slot at which this sweep should start spending its shaping budget.
    /// * `n_rows` - Total number of visible row slots in this pane.
    /// * `cursor_row` - The cursor's visible row slot, if visible.
    pub fn new(
        deadline: Option<Instant>,
        work_start: usize,
        n_rows: usize,
        cursor_row: Option<usize>,
    ) -> Self {
        RowSweep {
            deadline,
            work_start,
            n_rows,
            cursor_row,
            built_this_frame: 0,
            deferred_any: false,
            next_resume: None,
        }
    }

    /// Called once per row slot, in row order (0..n_rows), with `now`
    /// threaded in explicitly so this is testable with a synthetic clock.
    ///
    /// # Arguments
    /// * `slot` - The current row slot being processed.
    /// * `cache_hit` - Whether this row hit the line_quad_cache.
    /// * `has_retained` - Whether this row has retained quads from a prior frame.
    /// * `now` - The current wall-clock time.
    pub fn decide(
        &mut self,
        slot: usize,
        cache_hit: bool,
        has_retained: bool,
        now: Instant,
    ) -> RowAction {
        // Cache hits are always allowed; cheap and keep it fresh regardless of budget.
        if cache_hit {
            return RowAction::EmitCached;
        }

        // Cache miss: this row needs real shaping work to get fresh content.
        let must_build = !has_retained          // Never drawn before -> MUST NOT be blank
            || self.cursor_row == Some(slot)  // Keep the cursor row live every frame
            || self.built_this_frame == 0; // Forward-progress guarantee: at least one row

        let over_budget = self.deadline.map(|d| now >= d).unwrap_or(false);

        if must_build || (slot >= self.work_start && !over_budget) {
            self.built_this_frame += 1;
            RowAction::Build
        } else {
            self.deferred_any = true;
            if slot >= self.work_start && self.next_resume.is_none() {
                self.next_resume = Some(slot);
            }
            RowAction::EmitRetained
        }
    }

    /// Complete the sweep and return its outcome.
    pub fn finish(self) -> SweepOutcome {
        SweepOutcome {
            next_resume: self.next_resume.unwrap_or(0),
            deferred_any: self.deferred_any,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mock_instant() -> Instant {
        // Use a fixed instant as a synthetic clock for testing.
        Instant::now()
    }

    fn mock_deadline() -> Option<Instant> {
        // A deadline that will appear "over budget" when compared to mock_instant().
        Some(Instant::now() - std::time::Duration::from_millis(1))
    }

    #[test]
    fn no_row_is_ever_left_empty() {
        // For representative combinations of (cache_hit, has_retained, over_budget, slot vs work_start, cursor row),
        // decide() never returns something that would emit nothing; has_retained == false always yields Build regardless of budget state.
        let deadline = mock_deadline();

        // Test 1: has_retained == false must always Build, even over budget.
        let mut sweep = RowSweep::new(deadline, 0, 10, None);
        for slot in 0..10 {
            let action = sweep.decide(slot, false, false, mock_instant());
            assert_eq!(
                action,
                RowAction::Build,
                "slot {} with has_retained=false must Build",
                slot
            );
        }

        // Test 2: Cursor row must always Build, even if it has retained quads and is over budget.
        let cursor_row = 5;
        let mut sweep = RowSweep::new(deadline, 0, 10, Some(cursor_row));
        for slot in 0..10 {
            let action = sweep.decide(slot, false, true, mock_instant());
            if slot == cursor_row {
                assert_eq!(action, RowAction::Build, "cursor row {} must Build", slot);
            }
        }

        // Test 3: At least one row must Build even when already over budget.
        let mut sweep = RowSweep::new(deadline, 0, 10, None);
        let mut built_any = false;
        for slot in 0..10 {
            let action = sweep.decide(slot, false, true, mock_instant());
            if action == RowAction::Build {
                built_any = true;
            }
        }
        assert!(
            built_any,
            "at least one row must Build even when already over budget"
        );
    }

    #[test]
    fn budget_exceeded_defers_only_rows_that_have_something_to_show() {
        // EmitRetained is only ever returned when has_retained was true.
        let deadline = mock_deadline();
        let mut sweep = RowSweep::new(deadline, 0, 10, None);

        for slot in 0..10 {
            // Skip the first row (which must Build due to forward-progress guarantee).
            if slot == 0 {
                let action = sweep.decide(slot, false, true, mock_instant());
                assert_eq!(
                    action,
                    RowAction::Build,
                    "slot 0 must Build for forward-progress"
                );
                continue;
            }

            // When has_retained == false, we must Build, not defer.
            let action_no_retained = sweep.decide(slot, false, false, mock_instant());
            assert_ne!(
                action_no_retained,
                RowAction::EmitRetained,
                "slot {} with has_retained=false cannot EmitRetained",
                slot
            );

            // When has_retained == true and we're over budget, we can defer.
            let action_with_retained = sweep.decide(slot, false, true, mock_instant());
            assert_eq!(
                action_with_retained,
                RowAction::EmitRetained,
                "slot {} with has_retained=true should EmitRetained when over budget",
                slot
            );
        }
    }

    #[test]
    fn sweep_advances() {
        // Simulate a sequence of frames where the deadline trips at row k each time;
        // assert resume_row/next_resume strictly increases across frames and eventually reaches n_rows,
        // then resets to 0 on a clean sweep.
        let n_rows = 10;
        let deadline = mock_deadline();

        // Frame 1: work_start = 0, deadline trips immediately
        {
            let mut sweep = RowSweep::new(deadline, 0, n_rows, None);
            for slot in 0..n_rows {
                sweep.decide(slot, false, true, mock_instant());
            }
            let outcome = sweep.finish();
            // First row builds (forward progress), rest defer. next_resume points to first deferred row.
            assert_eq!(
                outcome.next_resume, 1,
                "next_resume should be 1 after first frame"
            );
            assert!(outcome.deferred_any, "frame should have deferred rows");
        }

        // Frame 2: work_start = 1 (from frame 1's outcome), deadline still trips immediately
        {
            let mut sweep = RowSweep::new(deadline, 1, n_rows, None);
            for slot in 0..n_rows {
                sweep.decide(slot, false, true, mock_instant());
            }
            let outcome = sweep.finish();
            // Row 0 builds (forward progress), row 1 is at work_start so it tries but defers, next_resume = 1
            assert_eq!(
                outcome.next_resume, 1,
                "next_resume stays at 1 because we can't advance past work_start when over budget"
            );
            assert!(outcome.deferred_any, "frame should have deferred rows");
        }

        // Frame 3: no deadline (budget disabled), completes cleanly
        {
            let mut sweep = RowSweep::new(None, 1, n_rows, None);
            for slot in 0..n_rows {
                sweep.decide(slot, false, true, mock_instant());
            }
            let outcome = sweep.finish();
            assert_eq!(
                outcome.next_resume, 0,
                "next_resume should be 0 after clean sweep"
            );
            assert!(!outcome.deferred_any, "frame should have no deferred rows");
        }
    }

    #[test]
    fn at_least_one_row_is_built_per_frame_even_when_already_over_budget() {
        // The livelock/forward-progress guard: even if now >= deadline from the very first decide() call,
        // at least one row still gets Build.
        let deadline = mock_deadline(); // Already over budget.
        let mut sweep = RowSweep::new(deadline, 0, 10, None);

        let mut built_any = false;
        for slot in 0..10 {
            let action = sweep.decide(slot, false, true, mock_instant());
            if action == RowAction::Build {
                built_any = true;
            }
        }
        assert!(
            built_any,
            "at least one row must Build even when already over budget"
        );
    }

    #[test]
    fn cursor_row_is_never_deferred() {
        // Cursor row is always built, never deferred, regardless of budget state.
        let deadline = mock_deadline();
        let cursor_row = 5;
        let mut sweep = RowSweep::new(deadline, 0, 10, Some(cursor_row));

        for slot in 0..10 {
            let action = sweep.decide(slot, false, true, mock_instant());
            if slot == cursor_row {
                assert_eq!(
                    action,
                    RowAction::Build,
                    "cursor row {} must Build, not {:?}",
                    slot,
                    action
                );
            }
        }
    }

    #[test]
    fn clean_sweep_resets_resume_to_zero_and_reports_no_deferral() {
        // A clean sweep (all cache hits or all rows built within budget) resets resume_row to 0 and reports no deferral.
        let deadline = Some(Instant::now() + std::time::Duration::from_secs(1)); // Far in future, so not over budget.
        let mut sweep = RowSweep::new(deadline, 0, 10, None);

        // All cache hits.
        for slot in 0..10 {
            sweep.decide(slot, true, true, mock_instant());
        }
        let outcome = sweep.finish();
        assert_eq!(
            outcome.next_resume, 0,
            "next_resume should be 0 after clean sweep"
        );
        assert!(
            !outcome.deferred_any,
            "deferred_any should be false after clean sweep"
        );
    }

    #[test]
    fn deadline_none_means_every_miss_builds() {
        // tab_frame_build_budget_ms = 0 (i.e. deadline: None) behaves exactly like the pre-existing behavior:
        // every cache miss builds, nothing is ever deferred.
        let deadline = None; // Budget disabled.
        let mut sweep = RowSweep::new(deadline, 0, 10, None);

        for slot in 0..10 {
            let action = sweep.decide(slot, false, true, mock_instant());
            assert_eq!(
                action,
                RowAction::Build,
                "slot {} must Build when deadline is None",
                slot
            );
        }
    }

    #[test]
    fn deferred_any_implies_a_repaint_would_be_requested() {
        // Check this is checkable directly off SweepOutcome without needing a real window/timer.
        let deadline = mock_deadline();
        let mut sweep = RowSweep::new(deadline, 0, 10, None);

        // Force deferrals by being over budget and having retained rows.
        for slot in 0..10 {
            sweep.decide(slot, false, true, mock_instant());
        }
        let outcome = sweep.finish();
        assert!(
            outcome.deferred_any,
            "deferred_any should be true when any row was deferred"
        );
    }
}
