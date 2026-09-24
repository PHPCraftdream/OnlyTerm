use crate::tabbar::TabBarItem;
use crate::termwindow::{
    GuiWin, MouseCapture, PositionedSplit, ScrollHit, TermWindowNotif, UIItem, UIItemType, TMB,
};
use ::window::{
    MouseButtons as WMB, MouseCursor, MouseEvent, MouseEventKind as WMEK, MousePress,
    WindowDecorations, WindowOps, WindowState,
};
use onlyterm_config::keyassignment::{KeyAssignment, MouseEventTrigger, SpawnTabDomain};
use onlyterm_config::MouseEventAltScreen;
use onlyterm_dynamic::ToDynamic;
use onlyterm_mux::pane::{Pane, WithPaneLines};
use onlyterm_mux::tab::SplitDirection;
use onlyterm_mux::Mux;
use onlyterm_mux_funcs::MuxPane;
use onlyterm_term::input::{MouseButton, MouseEventKind as TMEK};
use onlyterm_term::{ClickPosition, LastMouseClick, StableRowIndex};
use std::convert::TryInto;
use std::ops::Sub;
use std::sync::Arc;
use std::time::{Duration, Instant};
use termwiz::hyperlink::Hyperlink;
use termwiz::surface::Line;

#[path = "mouseevent/dispatch.rs"]
mod dispatch;
#[path = "mouseevent/drag.rs"]
mod drag;
#[path = "mouseevent/pane.rs"]
mod pane;
#[path = "mouseevent/ui_items.rs"]
mod ui_items;

fn mouse_press_to_tmb(press: &MousePress) -> TMB {
    match press {
        MousePress::Left => TMB::Left,
        MousePress::Right => TMB::Right,
        MousePress::Middle => TMB::Middle,
    }
}

/// What should happen when a mouse button is released over a cell, given
/// whether that cell carries an active hyperlink.
///
/// This is the specification for the default mouse bindings configured in
/// `InputMap::new` (see `onlyterm-gui/src/inputmap.rs`), which bind
/// `MouseEventTrigger::Up { button: MouseButton::Left, .. }` to
/// `CompleteSelectionOrOpenLinkAtMouseCursor` (open) and
/// `MouseEventTrigger::Up { button: MouseButton::Right, .. }` to
/// `CopyLinkAtMouseCursor` (copy). `TermWindow::do_open_link_at_mouse_cursor`
/// and `TermWindow::do_copy_link_at_mouse_cursor` implement the "open" and
/// "copy" halves respectively, each gated on `current_highlight.is_some()`
/// exactly as modeled here. Kept as a standalone pure function (rather than
/// inlined) so the button/link/action matrix has a single, unit-testable
/// source of truth. Any button other than left/right, or a click that
/// doesn't land on a hyperlink, has no hyperlink-related effect: a
/// right-click with no link present simply falls through to whatever else
/// is bound to it (e.g. the pane's native mouse reporting), and a
/// middle-click over a link is left alone since it is reserved for
/// primary-selection paste.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
enum HyperlinkClickAction {
    Open,
    Copy,
}

#[allow(dead_code)]
fn hyperlink_click_action(button: MousePress, has_hyperlink: bool) -> Option<HyperlinkClickAction> {
    if !has_hyperlink {
        return None;
    }
    match button {
        MousePress::Left => Some(HyperlinkClickAction::Open),
        MousePress::Right => Some(HyperlinkClickAction::Copy),
        MousePress::Middle => None,
    }
}

/// How long after gaining focus a button-down is still considered to be
/// "the activating click", for the purposes of arming suppression of a
/// same-position synthetic Move (see #2414, #5309). This only needs to
/// bridge a single OS message-pump tick, so it is deliberately much shorter
/// than the 200ms grace period used by `swallow_mouse_click_on_window_focus`
/// (which governs a different, opt-in policy: whether the activating click
/// itself is forwarded to the pane).
const FOCUS_CLICK_MOVE_SUPPRESSION_GRACE: Duration = Duration::from_millis(50);

/// Decide whether a button-down event should arm suppression of the next
/// same-position Move, given how long ago (if at all) the window most
/// recently gained focus.
fn should_arm_focus_click_move_suppression(focused_elapsed: Option<Duration>) -> bool {
    matches!(focused_elapsed, Some(elapsed) if elapsed <= FOCUS_CLICK_MOVE_SUPPRESSION_GRACE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arms_when_press_is_within_focus_grace_period() {
        assert!(should_arm_focus_click_move_suppression(Some(
            Duration::from_millis(0)
        )));
        assert!(should_arm_focus_click_move_suppression(Some(
            FOCUS_CLICK_MOVE_SUPPRESSION_GRACE
        )));
    }

    #[test]
    fn does_not_arm_once_grace_period_has_elapsed() {
        assert!(!should_arm_focus_click_move_suppression(Some(
            FOCUS_CLICK_MOVE_SUPPRESSION_GRACE + Duration::from_millis(1)
        )));
        assert!(!should_arm_focus_click_move_suppression(Some(
            Duration::from_secs(5)
        )));
    }

    #[test]
    fn does_not_arm_when_window_was_never_focused() {
        // `self.focused` is `None` whenever the window doesn't currently
        // have focus (e.g. it was never focused, or focus was just lost);
        // there is no activating click to protect in that case.
        assert!(!should_arm_focus_click_move_suppression(None));
    }

    /// Regression test for #2414 / #5309: a same-position Move that arrives
    /// immediately after the click that (re)focused the window must be
    /// recognized as suppressible, while a Move at a different position
    /// (real motion) must not be, and neither should be a Move that arrives
    /// well after the focus-granting click.
    #[test]
    fn suppresses_only_the_exact_zero_motion_move_after_focus_click() {
        let click_coords: (isize, isize) = (1, 1);

        // Simulates arming: a Press landed inside the grace period.
        let armed = if should_arm_focus_click_move_suppression(Some(Duration::from_millis(0))) {
            Some(click_coords)
        } else {
            None
        };
        assert_eq!(armed, Some(click_coords));

        // A same-position Move should match the armed coordinates (and thus
        // be suppressed by the caller).
        let same_position_move = click_coords;
        assert_eq!(armed, Some(same_position_move));

        // A Move at different coordinates is real motion and must not match.
        let real_motion_move: (isize, isize) = (1, 2);
        assert_ne!(armed, Some(real_motion_move));

        // If the Press arrived outside the grace period, nothing is armed,
        // so even a same-position Move afterwards is left untouched.
        let not_armed = if should_arm_focus_click_move_suppression(Some(Duration::from_millis(200)))
        {
            Some(click_coords)
        } else {
            None
        };
        assert_eq!(not_armed, None);
    }

    /// Regression test: right-clicking a hyperlink must copy its URL to the
    /// clipboard rather than open it, while left-click keeps opening the
    /// link as it always has.
    #[test]
    fn right_click_on_hyperlink_copies_left_click_opens() {
        assert_eq!(
            hyperlink_click_action(MousePress::Right, true),
            Some(HyperlinkClickAction::Copy)
        );
        assert_eq!(
            hyperlink_click_action(MousePress::Left, true),
            Some(HyperlinkClickAction::Open)
        );
    }

    #[test]
    fn click_without_a_hyperlink_does_nothing() {
        assert_eq!(hyperlink_click_action(MousePress::Left, false), None);
        assert_eq!(hyperlink_click_action(MousePress::Right, false), None);
        assert_eq!(hyperlink_click_action(MousePress::Middle, false), None);
    }

    #[test]
    fn middle_click_on_hyperlink_has_no_hyperlink_effect() {
        // Middle-click is reserved for primary-selection paste; it must not
        // be repurposed for hyperlink handling even when hovering a link.
        assert_eq!(hyperlink_click_action(MousePress::Middle, true), None);
    }
}
