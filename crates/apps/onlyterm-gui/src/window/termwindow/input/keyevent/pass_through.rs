//! Pure state machine behind double-Ctrl pass-through: two short, unbroken
//! Ctrl taps arm (on the second tap's release) a one-shot mode where every
//! key-down bypasses binding lookup until a non-modifier key-down or an IME
//! composition spends it -- that press keeps bypassing through autorepeat
//! until its key-up, and a repeat double-tap disarms. Broken taps, mouse
//! input mid-tap, long holds, focus loss and the disabled flag reset to
//! clean. Time is caller-injected; depends only on `::window::PhysKeyCode`.

use ::window::PhysKeyCode;
use std::time::{Duration, Instant};

/// How long a single Ctrl press may last and still count as a tap.
pub(crate) const TAP_MAX_HOLD: Duration = Duration::from_millis(400);
/// How late the second tap's Ctrl-down may follow the first tap's Ctrl-up.
pub(crate) const DOUBLE_TAP_INTERVAL: Duration = Duration::from_millis(400);

/// Per-event verdict for the caller; uninteresting events return the default.
#[derive(PartialEq, Eq, Clone, Copy, Debug, Default)]
pub(crate) struct Outcome {
    /// Skip binding lookup for this key-down (leader, key tables, input map).
    pub bypass: bool,
    /// This event spends the armed mode.
    pub consumes: bool,
    /// `Some(true)` just armed, `Some(false)` just disarmed -- the caller
    /// invalidates the window so the cursor-color indicator repaints.
    pub armed_edge: Option<bool>,
}

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
enum Tap {
    /// No Ctrl held, no interval window open.
    Idle,
    /// A first-tap Ctrl is held, gesture unbroken since `down_at`.
    Held1 { phys: PhysKeyCode, down_at: Instant },
    /// First tap released at `up_at`; a Ctrl-down within DOUBLE_TAP_INTERVAL
    /// starts the second tap.
    Between { up_at: Instant },
    /// Second-tap Ctrl held; arming (or the toggle-off) happens on its
    /// release, not its press.
    Held2 { phys: PhysKeyCode, down_at: Instant },
}

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
enum Mode {
    Disarmed,
    Armed,
    /// The spending key is held: its autorepeats keep bypassing until its
    /// key-up, nothing else bypasses, and `is_armed()` is already false.
    Consuming {
        phys: PhysKeyCode,
    },
}

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub(crate) struct PassThrough {
    enabled: bool,
    /// Gesture tracking keeps running while armed: the toggle-off is itself
    /// a completed double-tap.
    tap: Tap,
    mode: Mode,
}

impl PassThrough {
    pub(crate) fn new(enabled: bool) -> Self {
        Self {
            enabled,
            tap: Tap::Idle,
            mode: Mode::Disarmed,
        }
    }

    /// Mirrors the config flag. Turning it off clears everything, exactly
    /// like focus loss; turning it back on starts from nothing.
    pub(crate) fn set_enabled(&mut self, enabled: bool) -> Outcome {
        let mut outcome = Outcome::default();
        if self.enabled && !enabled {
            outcome.armed_edge = self.disarm();
            self.tap = Tap::Idle;
        }
        self.enabled = enabled;
        outcome
    }

    /// Read by the cursor-color indicator; wired in by a follow-up change of
    /// the same plan, hence the allowance until then.
    #[allow(dead_code)]
    pub(crate) fn is_armed(&self) -> bool {
        matches!(self.mode, Mode::Armed)
    }

    pub(crate) fn key(&mut self, phys: PhysKeyCode, is_down: bool, now: Instant) -> Outcome {
        let mut outcome = Outcome::default();
        if !self.enabled {
            return outcome;
        }
        if is_down {
            self.key_down(phys, now, &mut outcome);
        } else {
            self.key_up(phys, now, &mut outcome);
        }
        outcome
    }

    /// Any mouse button press or wheel tick. Breaks the tap in progress; an
    /// armed mode is deliberately untouched -- the doc clears it only via the
    /// spending key's key-up, a second double-tap, focus loss or the flag.
    pub(crate) fn mouse_input(&mut self) -> Outcome {
        let outcome = Outcome::default();
        if !self.enabled {
            return outcome;
        }
        if matches!(self.tap, Tap::Held1 { .. } | Tap::Held2 { .. }) {
            self.tap = Tap::Idle;
        }
        outcome
    }

    pub(crate) fn focus_lost(&mut self) -> Outcome {
        let mut outcome = Outcome::default();
        if !self.enabled {
            return outcome;
        }
        self.tap = Tap::Idle;
        outcome.armed_edge = self.disarm();
        outcome
    }

    /// Composed text from the IME (an event with no raw key part): spends the
    /// armed mode like a plain non-modifier key, and breaks a tap in progress
    /// -- the key-downs that produced the composition never reached us, so
    /// the gesture is already tainted.
    pub(crate) fn ime_composed(&mut self) -> Outcome {
        let mut outcome = Outcome::default();
        if !self.enabled {
            return outcome;
        }
        if matches!(self.tap, Tap::Held1 { .. } | Tap::Held2 { .. }) {
            self.tap = Tap::Idle;
        }
        if matches!(self.mode, Mode::Armed) {
            outcome.consumes = true;
            outcome.armed_edge = self.disarm();
        }
        outcome
    }

    fn key_down(&mut self, phys: PhysKeyCode, now: Instant, outcome: &mut Outcome) {
        match self.mode {
            Mode::Armed => {
                // Modifiers bypass too (bare-modifier bindings exist), but
                // only a non-modifier spends the mode -- otherwise the Ctrl
                // starting a Ctrl+Shift+C chord would burn it before the
                // chord could pass through.
                outcome.bypass = true;
                if !is_modifier(phys) {
                    self.mode = Mode::Consuming { phys };
                    outcome.consumes = true;
                    outcome.armed_edge = Some(false);
                }
            }
            Mode::Consuming { phys: spending } if spending == phys => {
                // One physical press is spent whole: autorepeats of the very
                // key that consumed the mode keep bypassing until its key-up.
                outcome.bypass = true;
            }
            Mode::Disarmed | Mode::Consuming { .. } => {}
        }

        match self.tap {
            Tap::Idle => {
                if is_ctrl(phys) {
                    self.tap = Tap::Held1 { phys, down_at: now };
                }
            }
            Tap::Held1 { phys: held, .. } => {
                if phys != held {
                    // A different Ctrl, another modifier, plain typing --
                    // anything breaks the gesture. The breaking press itself
                    // is not tracked: only a Ctrl-down from a clean slate
                    // starts a tap, so the AltGr fake-Ctrl and a Ctrl+click
                    // cannot re-seed one.
                    self.tap = Tap::Idle;
                }
                // else: autorepeat of the held Ctrl -- neither a new press nor
                // a break (Windows exposes no was_down bit, so the repeat is
                // counted here).
            }
            Tap::Between { up_at } => {
                if is_ctrl(phys) {
                    if now.saturating_duration_since(up_at) <= DOUBLE_TAP_INTERVAL {
                        self.tap = Tap::Held2 { phys, down_at: now };
                    } else {
                        // Window closed: this is a fresh first tap.
                        self.tap = Tap::Held1 { phys, down_at: now };
                    }
                } else {
                    self.tap = Tap::Idle;
                }
            }
            Tap::Held2 { phys: held, .. } => {
                if phys != held {
                    // The second tap was broken before its release, so it
                    // never arms -- nor toggles an armed mode off.
                    self.tap = Tap::Idle;
                }
            }
        }
    }

    fn key_up(&mut self, phys: PhysKeyCode, now: Instant, outcome: &mut Outcome) {
        // The spending press is over: its bypass privilege dies with it.
        if let Mode::Consuming { phys: spending } = self.mode {
            if spending == phys {
                self.mode = Mode::Disarmed;
            }
        }

        match self.tap {
            Tap::Held1 {
                phys: held,
                down_at,
            } if phys == held => {
                self.tap = if now.saturating_duration_since(down_at) <= TAP_MAX_HOLD {
                    Tap::Between { up_at: now }
                } else {
                    // Held too long: not a tap, and no interval window opens.
                    Tap::Idle
                };
            }
            Tap::Held2 {
                phys: held,
                down_at,
            } if phys == held => {
                self.tap = Tap::Idle;
                if now.saturating_duration_since(down_at) <= TAP_MAX_HOLD {
                    let was_armed = self.is_armed();
                    self.mode = if was_armed {
                        Mode::Disarmed
                    } else {
                        Mode::Armed
                    };
                    outcome.armed_edge = Some(!was_armed);
                }
                // else: second tap held too long -- gesture dies, mode
                // untouched.
            }
            _ => {}
        }
    }

    /// Drops the mode; returns a disarm edge only if `is_armed()` actually
    /// went true->false (a stale `Consuming` leftover was never armed).
    fn disarm(&mut self) -> Option<bool> {
        let was_armed = matches!(self.mode, Mode::Armed);
        self.mode = Mode::Disarmed;
        if was_armed {
            Some(false)
        } else {
            None
        }
    }
}

fn is_ctrl(phys: PhysKeyCode) -> bool {
    matches!(phys, PhysKeyCode::LeftControl | PhysKeyCode::RightControl)
}

/// The binding path's own notion of a modifier (keyevent.rs and the lookup
/// both use `KeyCode::is_modifier`); what spends the mode must agree with
/// what the mode bypasses, so the same predicate decides, via
/// `PhysKeyCode::to_key_code`.
fn is_modifier(phys: PhysKeyCode) -> bool {
    phys.to_key_code().is_modifier()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    const LCTRL: PhysKeyCode = PhysKeyCode::LeftControl;
    const RCTRL: PhysKeyCode = PhysKeyCode::RightControl;
    const RALT: PhysKeyCode = PhysKeyCode::RightAlt;
    const SHIFT: PhysKeyCode = PhysKeyCode::LeftShift;
    const C: PhysKeyCode = PhysKeyCode::C;
    const D: PhysKeyCode = PhysKeyCode::D;
    const X: PhysKeyCode = PhysKeyCode::X;

    fn pt() -> PassThrough {
        PassThrough::new(true)
    }

    fn down(sm: &mut PassThrough, t0: Instant, phys: PhysKeyCode, ms: u64) -> Outcome {
        sm.key(phys, true, t0 + Duration::from_millis(ms))
    }

    fn up(sm: &mut PassThrough, t0: Instant, phys: PhysKeyCode, ms: u64) -> Outcome {
        sm.key(phys, false, t0 + Duration::from_millis(ms))
    }

    /// A clean 50ms tap of `phys` starting at `ms`.
    fn tap(sm: &mut PassThrough, t0: Instant, phys: PhysKeyCode, ms: u64) -> (Outcome, Outcome) {
        (down(sm, t0, phys, ms), up(sm, t0, phys, ms + 50))
    }

    /// A clean double-tap: down/up, down/up, all within the limits.
    fn double_tap(
        sm: &mut PassThrough,
        t0: Instant,
        ms: u64,
        first: PhysKeyCode,
        second: PhysKeyCode,
    ) -> Outcome {
        down(sm, t0, first, ms);
        up(sm, t0, first, ms + 50);
        down(sm, t0, second, ms + 120);
        up(sm, t0, second, ms + 170)
    }

    fn assert_plain(o: Outcome) {
        assert!(!o.bypass, "unexpected bypass: {:?}", o);
        assert!(!o.consumes, "unexpected consume: {:?}", o);
        assert!(o.armed_edge.is_none(), "unexpected edge: {:?}", o);
    }

    /// AltGr layouts: Windows precedes RAlt with a fake LCtrl down; the RAlt
    /// must break that press, so typing @/€ never arms anything.
    #[test]
    fn altgr_fake_lctrl_interrupted_by_ralt_is_not_a_tap() {
        let t0 = Instant::now();
        let mut sm = pt();
        assert_plain(down(&mut sm, t0, LCTRL, 0));
        assert_plain(down(&mut sm, t0, RALT, 30));
        assert_plain(up(&mut sm, t0, LCTRL, 60));
        assert_plain(up(&mut sm, t0, RALT, 90));
        assert!(!sm.is_armed());
        // Nothing partial survived: a genuine double-tap right after arms.
        let last = double_tap(&mut sm, t0, 200, LCTRL, RCTRL);
        assert_eq!(last.armed_edge, Some(true));
        assert!(sm.is_armed());
    }

    /// Synthetic repeats of a held Ctrl (Windows exposes no was_down bit) are
    /// neither a second tap nor a break of the first one.
    #[test]
    fn held_ctrl_autorepeat_is_not_a_second_tap() {
        let t0 = Instant::now();
        let mut sm = pt();
        down(&mut sm, t0, LCTRL, 0);
        assert_plain(down(&mut sm, t0, LCTRL, 100));
        assert_plain(down(&mut sm, t0, LCTRL, 200));
        assert_plain(up(&mut sm, t0, LCTRL, 250)); // hold 250ms: first tap done
        assert!(!sm.is_armed());
        // The repeats neither armed anything nor spoiled the first tap:
        // a proper second tap now arms.
        let (d2, u2) = tap(&mut sm, t0, RCTRL, 300);
        assert_plain(d2);
        assert_eq!(u2.armed_edge, Some(true));
        assert!(sm.is_armed());
    }

    #[test]
    fn long_hold_is_not_a_tap() {
        let t0 = Instant::now();
        let mut sm = pt();
        down(&mut sm, t0, LCTRL, 0);
        assert_plain(up(&mut sm, t0, LCTRL, 401)); // 1ms over TAP_MAX_HOLD
        assert!(!sm.is_armed());
        // The dead hold must not even open the interval window: were it
        // counted as a first tap, this single quick tap would arm.
        down(&mut sm, t0, LCTRL, 600);
        assert_plain(up(&mut sm, t0, LCTRL, 650));
        assert!(!sm.is_armed());
    }

    #[test]
    fn ctrl_click_twice_does_not_arm() {
        let t0 = Instant::now();
        let mut sm = pt();
        for start in [0u64, 300] {
            down(&mut sm, t0, LCTRL, start);
            assert_plain(sm.mouse_input());
            assert_plain(up(&mut sm, t0, LCTRL, start + 50));
        }
        assert!(!sm.is_armed());
    }

    /// Tap, then the Ctrl+C chord: the chord's Ctrl-down is broken by C
    /// before its release, so it is not a second tap -- and since the mode
    /// never armed, lookup is not bypassed anywhere in the chord.
    #[test]
    fn tap_then_ctrl_c_chord_does_not_arm() {
        let t0 = Instant::now();
        let mut sm = pt();
        tap(&mut sm, t0, LCTRL, 0); // first tap done, window open
        let ctrl = down(&mut sm, t0, LCTRL, 120); // chord's Ctrl: tap candidate
        assert_plain(ctrl);
        assert_plain(down(&mut sm, t0, C, 150));
        assert_plain(up(&mut sm, t0, LCTRL, 180));
        assert_plain(up(&mut sm, t0, C, 200));
        assert!(!sm.is_armed());
    }

    #[test]
    fn armed_modifiers_bypass_without_consuming_c_consumes() {
        let t0 = Instant::now();
        let mut sm = pt();
        let arm = double_tap(&mut sm, t0, 0, LCTRL, RCTRL);
        assert_eq!(arm.armed_edge, Some(true));
        assert!(sm.is_armed());

        let ctrl = down(&mut sm, t0, LCTRL, 300);
        assert!(ctrl.bypass);
        assert!(!ctrl.consumes && ctrl.armed_edge.is_none());
        let shift = down(&mut sm, t0, SHIFT, 330);
        assert!(shift.bypass);
        assert!(!shift.consumes && shift.armed_edge.is_none());
        let c = down(&mut sm, t0, C, 360);
        assert!(c.bypass && c.consumes);
        assert_eq!(c.armed_edge, Some(false));
        assert!(!sm.is_armed()); // spent on the C-down, not on a key-up

        assert_plain(up(&mut sm, t0, LCTRL, 400));
        assert_plain(up(&mut sm, t0, SHIFT, 420));
        assert_plain(up(&mut sm, t0, C, 450));
        assert!(!sm.is_armed());

        // The next unrelated key-down goes through the normal path again.
        assert_plain(down(&mut sm, t0, X, 600));
        assert!(!sm.is_armed());
    }

    #[test]
    fn consuming_key_autorepeat_bypasses_other_keys_do_not() {
        let t0 = Instant::now();
        let mut sm = pt();
        double_tap(&mut sm, t0, 0, LCTRL, RCTRL);
        let c1 = down(&mut sm, t0, C, 200);
        assert!(c1.bypass && c1.consumes);
        let c2 = down(&mut sm, t0, C, 260); // synthetic repeat of the held key
        assert!(c2.bypass);
        assert!(!c2.consumes && c2.armed_edge.is_none());
        assert_plain(down(&mut sm, t0, D, 300)); // mode already spent
        assert_plain(up(&mut sm, t0, C, 350)); // ends the press; nothing more
        assert_plain(down(&mut sm, t0, X, 500));
        assert!(!sm.is_armed());
    }

    #[test]
    fn double_tap_while_armed_disarms() {
        let t0 = Instant::now();
        let mut sm = pt();
        double_tap(&mut sm, t0, 0, LCTRL, RCTRL);
        assert!(sm.is_armed());
        // While armed, even the toggle-off gesture's own Ctrl-downs bypass.
        let d = down(&mut sm, t0, RCTRL, 700);
        assert!(d.bypass);
        assert!(!d.consumes && d.armed_edge.is_none());
        assert_plain(up(&mut sm, t0, RCTRL, 750));
        let d2 = down(&mut sm, t0, RCTRL, 820); // within DOUBLE_TAP_INTERVAL
        assert!(d2.bypass);
        let off = up(&mut sm, t0, RCTRL, 870);
        assert_eq!(off.armed_edge, Some(false));
        assert!(!sm.is_armed());
        // No timeout: the mode stays off until a key or a fresh double-tap.
        assert_plain(down(&mut sm, t0, X, 2000));
    }

    #[test]
    fn focus_lost_resets_tap_and_armed_state() {
        let t0 = Instant::now();
        let probe = |sm: &mut PassThrough| -> Vec<Outcome> {
            vec![
                down(sm, t0, LCTRL, 100),
                up(sm, t0, LCTRL, 150),
                down(sm, t0, RCTRL, 300),
                up(sm, t0, RCTRL, 350), // genuine double-tap: arms
            ]
        };

        // Focus lost with a tap half-complete: the stale key-up must not
        // complete it, and what follows must behave like a fresh instance.
        let mut sm = pt();
        down(&mut sm, t0, LCTRL, 0);
        assert_plain(sm.focus_lost());
        assert_plain(up(&mut sm, t0, LCTRL, 50));
        let mut fresh = pt();
        assert_plain(up(&mut fresh, t0, LCTRL, 50));
        assert_eq!(probe(&mut sm), probe(&mut fresh));
        assert!(sm.is_armed());

        // Focus lost while armed: disarmed with an edge, then like fresh.
        let mut sm = pt();
        double_tap(&mut sm, t0, 0, LCTRL, RCTRL);
        assert!(sm.is_armed());
        let lost = sm.focus_lost();
        assert_eq!(lost.armed_edge, Some(false));
        assert!(!sm.is_armed());
        let mut fresh = pt();
        assert_eq!(probe(&mut sm), probe(&mut fresh));
    }

    #[test]
    fn disabled_flag_clears_everything_and_rearm_needs_fresh_double_tap() {
        let t0 = Instant::now();
        let probe = |sm: &mut PassThrough| -> Vec<Outcome> {
            vec![
                up(sm, t0, LCTRL, 20), // stale key-up: nothing
                down(sm, t0, LCTRL, 100),
                up(sm, t0, LCTRL, 150), // one tap: no arm
                down(sm, t0, LCTRL, 300),
                up(sm, t0, LCTRL, 350), // second tap: arms
            ]
        };

        // Disabled mid-tap.
        let mut sm = pt();
        down(&mut sm, t0, LCTRL, 0);
        assert_plain(sm.set_enabled(false));
        assert!(!sm.is_armed());
        // While disabled, input is ignored outright.
        assert_plain(double_tap(&mut sm, t0, 0, LCTRL, RCTRL));
        assert!(!sm.is_armed());
        sm.set_enabled(true);
        let mut fresh = pt();
        assert_eq!(probe(&mut sm), probe(&mut fresh));
        assert!(sm.is_armed());

        // Disabled while armed: same clearing, with a disarm edge.
        let mut sm = pt();
        double_tap(&mut sm, t0, 0, LCTRL, RCTRL);
        let off = sm.set_enabled(false);
        assert_eq!(off.armed_edge, Some(false));
        sm.set_enabled(true);
        let mut fresh = pt();
        assert_eq!(probe(&mut sm), probe(&mut fresh));
        assert!(sm.is_armed());
    }

    #[test]
    fn interval_and_hold_limits_are_inclusive() {
        let t0 = Instant::now();
        // Hold of exactly TAP_MAX_HOLD is a tap; the second Ctrl-down exactly
        // DOUBLE_TAP_INTERVAL after the first up arms on its release.
        let mut sm = pt();
        down(&mut sm, t0, LCTRL, 0);
        assert_plain(up(&mut sm, t0, LCTRL, 400));
        assert_plain(down(&mut sm, t0, LCTRL, 800)); // 800 - 400 == 400
        let u2 = up(&mut sm, t0, LCTRL, 850);
        assert_eq!(u2.armed_edge, Some(true));
        assert!(sm.is_armed());

        // 1ms over the hold limit: no tap, no window, no arm.
        let mut sm = pt();
        down(&mut sm, t0, LCTRL, 0);
        up(&mut sm, t0, LCTRL, 401);
        down(&mut sm, t0, LCTRL, 700);
        assert_plain(up(&mut sm, t0, LCTRL, 750));
        assert!(!sm.is_armed());

        // 1ms over the interval: the second down is a fresh first tap.
        let mut sm = pt();
        down(&mut sm, t0, LCTRL, 0);
        up(&mut sm, t0, LCTRL, 50);
        down(&mut sm, t0, LCTRL, 451); // 50 + 400 + 1
        up(&mut sm, t0, LCTRL, 501);
        assert!(!sm.is_armed());
    }

    /// A mouse click between the two taps does not close the interval window:
    /// the doc constrains the window by time alone. (A click DURING a tap is
    /// a break, covered by ctrl_click_twice_does_not_arm.)
    #[test]
    fn mouse_between_taps_leaves_interval_window_open() {
        let t0 = Instant::now();
        let mut sm = pt();
        tap(&mut sm, t0, LCTRL, 0);
        assert_plain(sm.mouse_input());
        assert_plain(down(&mut sm, t0, RCTRL, 300));
        let u = up(&mut sm, t0, RCTRL, 350);
        assert_eq!(u.armed_edge, Some(true));
    }

    #[test]
    fn ime_composition_spends_armed_mode_and_breaks_tap() {
        let t0 = Instant::now();
        // Armed: composed text counts as a plain key and consumes.
        let mut sm = pt();
        double_tap(&mut sm, t0, 0, LCTRL, RCTRL);
        let ime = sm.ime_composed();
        assert!(ime.consumes);
        assert_eq!(ime.armed_edge, Some(false));
        assert!(!sm.is_armed());
        assert_plain(down(&mut sm, t0, X, 200));

        // Half-complete tap: the composition taints it, the stale key-up
        // must not complete anything.
        let mut sm = pt();
        down(&mut sm, t0, LCTRL, 0);
        assert_plain(sm.ime_composed());
        assert_plain(up(&mut sm, t0, LCTRL, 100));
        assert!(!sm.is_armed());
    }
}
