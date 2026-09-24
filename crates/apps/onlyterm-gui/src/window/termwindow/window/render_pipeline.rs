use super::prevcursor::PrevCursorPos;
use super::*;
use crate::colorease::ColorEase;
use crate::frontend::front_end;
use crate::inputmap::InputMap;
use crate::overlay::{CopyOverlay, QuickSelectOverlay};
use crate::resize_increment_calculator::ResizeIncrementCalculator;
use crate::tabbar::TabBarState;
use crate::termwindow::background::load_background_image;
use crate::termwindow::keyevent::KeyTableState;
use crate::termwindow::render::paint::AllowImage;
use crate::utilsprites::RenderMetrics;
use anyhow::{anyhow, Context};
use onlyterm_config::{
    configuration, AudibleBell, Dimension, DimensionContext, FrontEndSelection, GeometryOrigin,
};
use onlyterm_font::FontConfiguration;
use onlyterm_gpu_render::{rebuild_backoff_for_attempt, WebGpuState};
use onlyterm_mux::pane::{Pane, PaneId};
use onlyterm_mux::renderable::RenderableDimensions;
use onlyterm_mux::window::WindowId as MuxWindowId;
use onlyterm_mux::{Mux, MuxNotification};
use onlyterm_term::{Alert, StableRowIndex, TerminalSize};
use smol::Timer;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, LinkedList};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Builds the recovery callback handed to the GPU-render crate when a window
/// registers for device-lost/error notifications: routes the reason string
/// into the GUI-thread `TermWindowNotif::Apply` that runs
/// `handle_render_error_recovery` -- exactly what the webgpu context's
/// subscriber machinery used to do inline, before that code moved out of
/// this crate.
fn gpu_recovery_notifier(window: &Window) -> onlyterm_gpu_render::GpuRecoveryNotifier {
    let win = window.clone();
    Box::new(move |reason: &str| {
        let reason = reason.to_string();
        let win2 = win.clone();
        win.notify(TermWindowNotif::Apply(Box::new(move |tw| {
            tw.handle_render_error_recovery(&win2, &reason);
        })));
    })
}

#[path = "render_pipeline/painting.rs"]
mod painting;
#[path = "render_pipeline/pane_events.rs"]
mod pane_events;
#[path = "render_pipeline/render_health.rs"]
mod render_health;
#[path = "render_pipeline/window_events.rs"]
mod window_events;
#[path = "render_pipeline/window_setup.rs"]
mod window_setup;

fn forget_pane_caches(
    pane_state: &mut HashMap<PaneId, PaneState>,
    semantic_zones: &mut HashMap<PaneId, SemanticZoneCache>,
    retained_rows: &mut HashMap<PaneId, render::RetainedPaneRows>,
    pane_id: PaneId,
) -> bool {
    let dropped_state = pane_state.remove(&pane_id).is_some();
    let dropped_zones = semantic_zones.remove(&pane_id).is_some();
    let dropped_rows = retained_rows.remove(&pane_id).is_some();
    dropped_state || dropped_zones || dropped_rows
}

#[cfg(test)]
mod pane_removed_cleanup_tests {
    use super::*;

    fn test_retained_rows() -> render::RetainedPaneRows {
        render::RetainedPaneRows {
            stamp: render::RetainedStamp {
                config_generation: 0,
                shape_generation: 0,
                quad_generation: 0,
                pixel_width: 800,
                pixel_height: 600,
                cell_height: 17,
                left_pixel_x: ordered_float::NotNan::new(0.0).unwrap(),
                top_pixel_y: ordered_float::NotNan::new(0.0).unwrap(),
                num_rows: 3,
                num_cols: 80,
            },
            viewport_top: 0,
            rows: vec![None; 3],
            resume_row: 0,
        }
    }

    /// Regression test for docs/investigations/2026-08-25-render-and-resource-bug-hunt.md
    /// §3.5: closing a pane must drop its entries from pane_state,
    /// semantic_zones, and retained_rows (all three were insert-only and
    /// grew by one entry per pane ever opened). Exercises the exact function
    /// the `MuxNotification::PaneRemoved` dispatch arm calls.
    #[test]
    fn pane_removed_drops_all_three_per_pane_caches_and_only_those() {
        let gone: PaneId = 0x5eed_0001;
        let kept: PaneId = 0x5eed_0002;
        let unknown: PaneId = 0x5eed_0003;

        let mut pane_state = HashMap::new();
        pane_state.insert(gone, PaneState::default());
        pane_state.insert(kept, PaneState::default());
        let mut semantic_zones = HashMap::new();
        semantic_zones.insert(gone, SemanticZoneCache::default());
        semantic_zones.insert(kept, SemanticZoneCache::default());
        let mut retained_rows = HashMap::new();
        retained_rows.insert(gone, test_retained_rows());
        retained_rows.insert(kept, test_retained_rows());

        assert!(
            forget_pane_caches(
                &mut pane_state,
                &mut semantic_zones,
                &mut retained_rows,
                gone
            ),
            "closing a pane that had cached state must report dropping something"
        );

        assert!(
            !pane_state.contains_key(&gone),
            "pane_state must drop the closed pane's entry"
        );
        assert!(
            !semantic_zones.contains_key(&gone),
            "semantic_zones must drop the closed pane's entry"
        );
        assert!(
            !retained_rows.contains_key(&gone),
            "retained_rows must drop the closed pane's entry"
        );

        // Over-deletion guard: the still-live pane keeps every entry.
        assert!(pane_state.contains_key(&kept));
        assert!(semantic_zones.contains_key(&kept));
        assert!(retained_rows.contains_key(&kept));

        // PaneRemoved is broadcast to every window; an id this window never
        // rendered must be a harmless no-op.
        assert!(!forget_pane_caches(
            &mut pane_state,
            &mut semantic_zones,
            &mut retained_rows,
            unknown
        ));
    }
}

#[cfg(test)]
mod render_state_send_bound_tests {
    use onlyterm_gpu_render::WebGpuState;
    use std::sync::Arc;

    fn assert_send<T: Send>() {}

    /// `Arc<WebGpuState>` must stay `Send` for the background-thread drop in
    /// `begin_renderer_rebuild`'s Step 2 to compile at all -- if this stops
    /// compiling, that drop path needs to change to something else that
    /// still keeps the suspect-driver call off the GUI thread, not just
    /// have this assertion deleted.
    ///
    /// The other half of Step 2's reasoning -- `RenderState` must stay
    /// `!Send`, which is why it is `mem::forget`-ten there instead of
    /// following this same background-thread path -- has no equivalent
    /// test: asserting "does not implement Send" would need to fail to
    /// *compile*, which a `#[test]` cannot express without a compile-fail
    /// harness this crate doesn't otherwise depend on. That half of the
    /// invariant is stated in `begin_renderer_rebuild`'s own comments
    /// instead; if `RenderState` ever becomes `Send` (e.g. its internal
    /// `Rc`/`RefCell` replaced with `Arc`/`Mutex`), its `mem::forget` should
    /// become a background drop like this one.
    #[test]
    fn webgpu_state_arc_is_send() {
        assert_send::<Arc<WebGpuState>>();
    }
}
