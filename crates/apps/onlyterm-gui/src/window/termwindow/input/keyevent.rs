#[path = "keyevent/key_table.rs"]
mod key_table;
pub use key_table::{KeyTableArgs, KeyTableState};

// The double-Ctrl pass-through detector; fed from raw_key_event_impl and
// consumed in key_event_impl below (docs/plans/2026-09-23-double-ctrl-pass-through.md).
#[path = "keyevent/pass_through.rs"]
pub(crate) mod pass_through;
#[path = "keyevent/protocol.rs"]
mod protocol;

use crate::termwindow::InputMap;
use ::window::{
    DeadKeyStatus, KeyCode, KeyEvent, KeyboardLedStatus, Modifiers, PhysKeyCode, RawKeyEvent,
    WindowOps,
};
use anyhow::Context;
use onlyterm_config::keyassignment::{KeyAssignment, KeyTableEntry};
use onlyterm_mux::pane::{Pane, PerformAssignmentResult};
use smol::Timer;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Debug)]
pub enum Key {
    Code(::termwiz::input::KeyCode),
    Composed(String),
    None,
}

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
enum OnlyKeyBindings {
    Yes,
    No,
}

/// True for the three chords task "[0123]" requires to always insert a
/// newline (CTRL+Enter, SHIFT+Enter, CTRL+J), regardless of whether they
/// actually resolve to a key-table entry. Used only to scope an
/// unconditional diagnostic log to these keys instead of every keypress.
fn is_newline_chord(keycode: &KeyCode, mods: Modifiers) -> bool {
    match keycode {
        KeyCode::Char('\r') | KeyCode::Physical(PhysKeyCode::Return) => {
            mods.contains(Modifiers::CTRL) || mods.contains(Modifiers::SHIFT)
        }
        KeyCode::Char('j') | KeyCode::Char('J') => mods.contains(Modifiers::CTRL),
        _ => false,
    }
}

/// True for any CTRL+<character key> chord -- the class the "Ctrl+C and
/// Ctrl+J do nothing under a Cyrillic layout, but work under a Latin one"
/// reports fall into.
///
/// Every stage between the Windows message and the bytes on the wire has
/// been made layout-independent by a separate fix (window.rs's
/// prefer_physical, vkey_to_phys, KeyCode::to_phys,
/// PhysKeyCode::to_win32_key_codes, CommandDef::permute_keys), so the
/// remaining question is no longer "which stage is layout-dependent" but
/// "which stage does this keypress actually reach, and with what identity".
/// Only a log taken on the failing machine, under the failing layout, can
/// answer that -- hence unconditional (not gated on `debug_key_events`,
/// which the user would have to know to turn on).
///
/// Scoped to CTRL, and to non-modifier keys, so the volume stays bounded:
/// plain typing, SHIFT and ALT chords log nothing, and neither does merely
/// holding Ctrl down. RingLog flushes to disk per record (see ringlog.rs),
/// so a looser predicate here is a burst of synchronous disk writes -- the
/// first cut of this diagnostic did include the modifier itself and Ctrl's
/// auto-repeat alone produced ~30 records a second, drowning the chords it
/// was meant to capture.
fn is_ctrl_chord_of_interest(keycode: &KeyCode, mods: Modifiers) -> bool {
    if !mods.contains(Modifiers::CTRL) {
        return false;
    }
    match keycode {
        KeyCode::Char(_) => true,
        KeyCode::Physical(phys) => !phys.to_key_code().is_modifier(),
        _ => false,
    }
}

impl super::TermWindow {
    #[allow(clippy::too_many_arguments)] // key dispatch: params carry the full input context the handler needs
    fn process_key(
        &mut self,
        pane: &Arc<dyn Pane>,
        context: &dyn WindowOps,
        keycode: &KeyCode,
        raw_modifiers: Modifiers,
        leader_active: bool,
        leader_mod: Modifiers,
        bypass_lookup: bool,
        only_key_bindings: OnlyKeyBindings,
        is_down: bool,
        key_event: Option<&KeyEvent>,
    ) -> bool {
        if is_down && !leader_active && !bypass_lookup {
            // Check to see if this key-press is the leader activating
            if let Some(duration) = self.input_map.is_leader(keycode, raw_modifiers) {
                // Yes; record its expiration
                let target = std::time::Instant::now() + duration;
                self.leader_is_down.replace(target);
                self.update_title();
                // schedule an invalidation so that the cursor or status
                // area will be repainted at the right time
                if let Some(window) = self.window.clone() {
                    onlyterm_promise::spawn::spawn(async move {
                        Timer::at(target).await;
                        window.invalidate();
                    })
                    .detach();
                }
                return true;
            }
        }

        if is_down {
            if only_key_bindings == OnlyKeyBindings::No {
                if let Some(modal) = self.get_modal() {
                    if let Key::Code(term_key) = self.win_key_code_to_termwiz_key_code(keycode) {
                        match modal.key_down(term_key, raw_modifiers.remove_positional_mods(), self)
                        {
                            Ok(true) => return true,
                            Ok(false) => {}
                            Err(err) => {
                                log::error!("Error dispatching key to modal: {err:#}");
                                return true;
                            }
                        }
                    }
                }
            }

            let effective_mods = raw_modifiers | leader_mod;
            let looked_up = if bypass_lookup {
                // Pass-through mode: bindings are not consulted -- and
                // lookup_key mutates the key-table stack, so it is skipped
                // outright rather than queried and discarded.
                None
            } else {
                self.lookup_key(pane, keycode, effective_mods, only_key_bindings)
            };

            // Unconditional (not gated on debug_key_events) diagnostic for
            // the three newline-insertion chords specifically: whether this
            // keypress resolves to a key-table entry at all is exactly the
            // fact needed to tell "the binding matched but the app didn't
            // react" apart from "the binding never matched, so the
            // SendEnterOrNewline/SendChar handler never even ran" -- the
            // latter would otherwise look identical to the user as "nothing
            // happened", but has a completely different fix. Reuses the
            // lookup result computed above rather than calling lookup_key
            // again, since it mutates key-table stack state and calling it
            // twice per keypress would double-apply that.
            if is_newline_chord(keycode, effective_mods)
                || is_ctrl_chord_of_interest(keycode, effective_mods)
            {
                log::debug!(
                    "diag: chord {:?} {:?} pass={:?} -> key table lookup {}",
                    keycode,
                    effective_mods,
                    only_key_bindings,
                    match &looked_up {
                        Some((entry, _)) => format!("matched, action={:?}", entry.action),
                        None => "found NO entry (falls through to raw typing)".to_string(),
                    }
                );
            }

            if let Some((entry, table_name)) = looked_up {
                if self.config.debug_key_events {
                    log::info!(
                        "{}{:?} {:?} -> perform {:?}",
                        match table_name {
                            Some(name) => format!("table:{} ", name),
                            None => String::new(),
                        },
                        keycode,
                        raw_modifiers | leader_mod,
                        entry.action,
                    );
                }

                self.key_table_state.did_process_key();
                let handled = match self.perform_key_assignment(pane, &entry.action) {
                    Ok(PerformAssignmentResult::Handled) => true,
                    Err(_) => true,
                    Ok(_) => false,
                };

                if handled {
                    context.invalidate();

                    if leader_active {
                        // A successful leader key-lookup cancels the leader
                        // virtual modifier state
                        self.leader_done();
                    }

                    return true;
                }
            }
        }

        // While the leader modifier is active, only registered
        // keybindings are recognized.
        let only_key_bindings = match (only_key_bindings, leader_active) {
            (OnlyKeyBindings::Yes, _) => OnlyKeyBindings::Yes,
            (_, true) => OnlyKeyBindings::Yes,
            _ => OnlyKeyBindings::No,
        };

        if only_key_bindings == OnlyKeyBindings::No {
            let config = &self.config;

            // This is a bit ugly.
            // Not all of our platforms report LEFT|RIGHT ALT; most report just ALT.
            // For those that do distinguish between them we want to respect the left vs.
            // right settings for the compose behavior.
            // Otherwise, if the event didn't include left vs. right then we want to
            // respect the generic compose behavior.
            let bypass_compose =
                    // Left ALT and they disabled compose
                    (raw_modifiers.contains(Modifiers::LEFT_ALT)
                    && !config.send_composed_key_when_left_alt_is_pressed)
                    // Right ALT and they disabled compose
                    || (raw_modifiers.contains(Modifiers::RIGHT_ALT)
                        && !config.send_composed_key_when_right_alt_is_pressed)
                    // Generic ALT and they disabled generic compose
                    || (!raw_modifiers.contains(Modifiers::RIGHT_ALT)
                        && !raw_modifiers.contains(Modifiers::LEFT_ALT)
                        && raw_modifiers.contains(Modifiers::ALT)
                        && !(config.send_composed_key_when_left_alt_is_pressed
                             || config.send_composed_key_when_right_alt_is_pressed));

            if bypass_compose {
                if let Key::Code(term_key) = self.win_key_code_to_termwiz_key_code(keycode) {
                    let tw_raw_modifiers = raw_modifiers;

                    let mut did_encode = false;
                    if let Some(key_event) = key_event {
                        if let Some(encoded) = self.encode_win32_input(pane, key_event) {
                            if self.config.debug_key_events {
                                log::info!("win32: Encoded input as {:?}", encoded);
                            }
                            pane.writer()
                                .write_all(encoded.as_bytes())
                                .context("sending win32-input-mode encoded data")
                                .ok();
                            did_encode = true;
                        } else if let Some(encoded) = self.encode_kitty_input(pane, key_event) {
                            if self.config.debug_key_events {
                                log::info!("kitty: Encoded input as {:?}", encoded);
                            }
                            pane.writer()
                                .write_all(encoded.as_bytes())
                                .context("sending kitty encoded data")
                                .ok();
                            did_encode = true;
                        }
                    };
                    if !did_encode {
                        if self.config.debug_key_events {
                            log::info!(
                                "{:?} {:?} -> send to pane {:?} {:?}",
                                keycode,
                                raw_modifiers,
                                term_key,
                                tw_raw_modifiers
                            );
                        }

                        did_encode = if is_down {
                            pane.key_down(term_key, tw_raw_modifiers)
                        } else {
                            pane.key_up(term_key, tw_raw_modifiers)
                        }
                        .is_ok();
                    };

                    if did_encode {
                        if is_down
                            && !keycode.is_modifier()
                            && self.pane_state(pane.pane_id()).overlay.is_none()
                        {
                            self.maybe_scroll_to_bottom_for_input(pane);
                        }
                        if is_down
                            && self.config.hide_mouse_cursor_when_typing
                            && !keycode.is_modifier()
                        {
                            context.set_cursor(None);
                        }
                        if !keycode.is_modifier() {
                            context.invalidate();
                        }

                        return true;
                    }
                }
            }
        }

        false
    }

    pub fn raw_key_event_impl(&mut self, key: RawKeyEvent, context: &dyn WindowOps) {
        // Feed the pass-through detector first: it must see every raw event,
        // including ones this function would otherwise handle, and a
        // bypassed event must not reach the leader check below (its
        // is_leader lookup mutates leader state). The outcome is stashed for
        // key_event_impl, which receives the cooked KeyEvent for this same
        // press as a separate WindowEvent.
        let pass_through_outcome = match key.phys_code {
            Some(phys) => {
                let outcome = self.pass_through.key(phys, key.key_is_down, Instant::now());
                self.pending_pass_through = Some((
                    phys,
                    key.key_is_down,
                    pass_through::Outcome {
                        bypass: outcome.bypass,
                        consumes: outcome.consumes,
                        // Edges are observed here rather than in
                        // key_event_impl: a key-up can be fully handled below
                        // (the encoders encode key-ups too), and then no
                        // KeyEvent ever follows for it.
                        armed_edge: None,
                    },
                ));
                outcome
            }
            None => pass_through::Outcome::default(),
        };

        if let Some(armed) = pass_through_outcome.armed_edge {
            // Arming cancels a leader that is active at that moment, or the
            // next key -- the one meant to pass through -- would be swallowed
            // by the leader branch of key_event_impl.
            if armed && self.leader_is_active() {
                self.leader_done();
            }
            log::debug!(
                "diag: pass-through {} (double ctrl tap)",
                if armed { "armed" } else { "disarmed" }
            );
            // The fancy tab bar caches its built element (paint_tab_bar only
            // rebuilds it when invalidated): refresh the accent rim now.
            self.invalidate_fancy_tab_bar();
            context.invalidate();
        }

        if pass_through_outcome.bypass {
            // Keep the modifier/LED snapshot current even though the binding
            // paths below are skipped.
            let modifier_and_leds = (key.modifiers, key.leds);
            if self.current_modifier_and_leds != modifier_and_leds {
                self.current_modifier_and_leds = modifier_and_leds;
            }
            // Deliberately not set_handled: the cooked KeyEvent for this
            // press must still arrive, and it is what sends the key to the
            // pane with bindings bypassed.
            return;
        }

        // The leader key is a kind of modal modifier key.
        // It is allowed to be active for up to the leader timeout duration,
        // after which it auto-deactivates.
        let (leader_active, leader_mod) = if self.leader_is_active_mut() {
            // Currently active
            (true, Modifiers::LEADER)
        } else {
            (false, Modifiers::NONE)
        };

        if self.config.debug_key_events {
            log::info!(
                "key_event {:?} {}",
                key,
                if leader_active { "LEADER" } else { "" }
            );
        } else {
            log::trace!(
                "key_event {:?} {}",
                key,
                if leader_active { "LEADER" } else { "" }
            );
        }

        let modifier_and_leds = (key.modifiers, key.leds);
        if self.current_modifier_and_leds != modifier_and_leds {
            self.current_modifier_and_leds = modifier_and_leds;
        }

        let pane = match self.get_active_pane_or_overlay() {
            Some(pane) => pane,
            None => return,
        };

        // Unconditional diagnostic for CTRL chords: this is the earliest
        // point at which the keypress exists inside OnlyTerm, so it shows
        // the identity Windows actually handed us -- before any of our own
        // normalization. If a Cyrillic-layout Ctrl+J arrives here with the
        // wrong phys_code or vk, every later stage is working from bad
        // input and there is no point looking at them. See
        // `is_ctrl_chord_of_interest`.
        if is_ctrl_chord_of_interest(&key.key, key.modifiers) {
            log::debug!(
                "diag: raw key event key={:?} phys_code={:?} vk={:#04x} sc={:#04x} \
                 mods={:?} down={} pane_encoding={:?}",
                key.key,
                key.phys_code,
                key.raw_code,
                key.scan_code,
                key.modifiers,
                key.key_is_down,
                pane.get_keyboard_encoding(),
            );
        }

        // First, try to match raw physical key
        let phys_key = match &key.key {
            phys @ KeyCode::Physical(_) => Some(phys.clone()),
            _ => key.phys_code.map(KeyCode::Physical),
        };

        if let Some(phys_key) = &phys_key {
            if self.process_key(
                &pane,
                context,
                phys_key,
                key.modifiers,
                leader_active,
                leader_mod,
                false,
                OnlyKeyBindings::Yes,
                key.key_is_down,
                None,
            ) {
                key.set_handled();
                return;
            }
        }

        // Then try the raw code
        let raw_key = match &key.key {
            raw @ KeyCode::RawCode(_) => raw.clone(),
            _ => KeyCode::RawCode(key.raw_code),
        };
        if self.process_key(
            &pane,
            context,
            &raw_key,
            key.modifiers,
            leader_active,
            leader_mod,
            false,
            OnlyKeyBindings::Yes,
            key.key_is_down,
            None,
        ) {
            key.set_handled();
            return;
        }

        if phys_key.as_ref() == Some(&key.key) || raw_key == key.key {
            // We already matched against whatever key.key is, so no need
            // to do it again below
            return;
        }

        if self.process_key(
            &pane,
            context,
            &key.key,
            key.modifiers,
            leader_active,
            leader_mod,
            false,
            OnlyKeyBindings::Yes,
            key.key_is_down,
            None,
        ) {
            key.set_handled();
        }
    }

    pub fn current_modifier_and_led_state(&self) -> (Modifiers, KeyboardLedStatus) {
        self.current_modifier_and_leds
    }

    pub fn leader_is_active(&self) -> bool {
        match self.leader_is_down.as_ref() {
            Some(expiry) if *expiry > std::time::Instant::now() => {
                self.update_next_frame_time(Some(*expiry));
                true
            }
            Some(_) => false,
            None => false,
        }
    }

    pub fn leader_is_active_mut(&mut self) -> bool {
        match self.leader_is_down.as_ref() {
            Some(expiry) if *expiry > std::time::Instant::now() => {
                self.update_next_frame_time(Some(*expiry));
                true
            }
            Some(_) => {
                self.leader_done();
                false
            }
            None => false,
        }
    }

    pub fn current_key_table_name(&mut self) -> Option<String> {
        let mut name = None;

        if let Some(pane) = self.get_active_pane_or_overlay() {
            if let Some(overlay) = self.pane_state(pane.pane_id()).overlay.as_mut() {
                name = overlay
                    .key_table_state
                    .current_table()
                    .map(|s| s.to_string());

                if let Some(expiry) = overlay.key_table_state.current_expiration() {
                    self.update_next_frame_time(Some(expiry));
                }
            }
        }
        if name.is_none() {
            name = self.key_table_state.current_table().map(|s| s.to_string());
        }
        if let Some(expiry) = self.key_table_state.current_expiration() {
            self.update_next_frame_time(Some(expiry));
        }
        name
    }

    pub fn composition_status(&self) -> &DeadKeyStatus {
        &self.dead_key_status
    }

    fn leader_done(&mut self) {
        self.leader_is_down.take();
        self.update_title();
        if let Some(window) = &self.window {
            window.invalidate();
        }
    }

    pub fn key_event_impl(&mut self, window_key: KeyEvent, context: &dyn WindowOps) {
        let pane = match self.get_active_pane_or_overlay() {
            Some(pane) => pane,
            None => {
                // A `--choose-tab` window has no pane until the user presses
                // Run, and returning here left its dialog unable to receive a
                // single keystroke -- the modal dispatch further down is never
                // reached. A modal owns the keyboard regardless of what is
                // behind it, so hand the key over before the pane is required.
                //
                // Deliberately scoped to the no-pane case: whenever a pane
                // does exist, the path below is unchanged, leader handling and
                // all.
                if let Some(modal) = self.get_modal() {
                    if window_key.key_is_down {
                        if let Key::Code(key) =
                            self.win_key_code_to_termwiz_key_code(&window_key.key)
                        {
                            modal.key_down(key, window_key.modifiers, self).ok();
                        }
                    }
                }
                return;
            }
        };

        // The leader key is a kind of modal modifier key.
        // It is allowed to be active for up to the leader timeout duration,
        // after which it auto-deactivates.
        let (leader_active, leader_mod) = if self.leader_is_active_mut() {
            // Currently active
            (true, Modifiers::LEADER)
        } else {
            (false, Modifiers::NONE)
        };

        if self.config.debug_key_events {
            log::info!(
                "key_event {:?} {}",
                window_key,
                if leader_active { "LEADER" } else { "" }
            );
        } else {
            log::trace!(
                "key_event {:?} {}",
                window_key,
                if leader_active { "LEADER" } else { "" }
            );
        }

        // The pass-through outcome for this press was computed exactly once
        // in raw_key_event_impl; feeding the detector again here would count
        // the same press twice. Events with no raw part are IME results:
        // report them as "another key", which consumes an armed mode.
        let pass_through_outcome = if window_key.raw.is_none() {
            self.pending_pass_through = None;
            let outcome = self.pass_through.ime_composed();
            if let Some(armed) = outcome.armed_edge {
                log::debug!(
                    "diag: pass-through {} (composed IME text)",
                    if armed { "armed" } else { "disarmed" }
                );
                // Same as the raw-stage edge: refresh the cached accent rim.
                self.invalidate_fancy_tab_bar();
                context.invalidate();
            }
            outcome
        } else {
            match self.pending_pass_through.take().filter(|(phys, down, _)| {
                window_key.raw.as_ref().and_then(|raw| raw.phys_code) == Some(*phys)
                    && *down == window_key.key_is_down
            }) {
                Some((_, _, outcome)) => outcome,
                // No matching raw outcome: the raw event was handled by a
                // binding, or carried no phys code. Nothing bypasses.
                None => pass_through::Outcome::default(),
            }
        };
        if pass_through_outcome.consumes {
            log::debug!(
                "diag: pass-through consumed key={:?} key_is_down={}",
                window_key.key,
                window_key.key_is_down
            );
        }

        let modifiers = window_key.modifiers;

        if self.process_key(
            &pane,
            context,
            &window_key.key,
            window_key.modifiers,
            leader_active,
            leader_mod,
            pass_through_outcome.bypass,
            OnlyKeyBindings::No,
            window_key.key_is_down,
            Some(&window_key),
        ) {
            return;
        }

        // If we get here, then none of the keys matched
        // any key table rules. Therefore, we should pop all `until_unknown`
        // entries from the stack.
        if window_key.key_is_down {
            self.key_table_state.pop_until_unknown();
        }

        let key = self.win_key_code_to_termwiz_key_code(&window_key.key);

        match key {
            Key::Code(key) => {
                if window_key.key_is_down && !key.is_modifier() {
                    if leader_active {
                        // Leader was pressed and this non-modifier keypress isn't
                        // a registered key binding; swallow this event and cancel
                        // the leader modifier.
                        self.leader_done();
                        return;
                    }
                    self.key_table_state.did_process_key();
                }

                if let Some(modal) = self.get_modal() {
                    if window_key.key_is_down {
                        modal.key_down(key, modifiers, self).ok();
                    }
                    return;
                }

                log::trace!(
                    "diag: key_event_impl key={:?} mods={:?} enable_kitty_keyboard={} \
                     pane_keyboard_encoding={:?}",
                    key,
                    modifiers,
                    self.config.enable_kitty_keyboard,
                    pane.get_keyboard_encoding(),
                );

                // Unconditional for CTRL chords: this is where a chord that
                // matched no key binding turns into actual bytes, and which
                // of the three encoders runs is decided by what the app in
                // the pane negotiated. That is the one thing that differs
                // between two apps in the same window under the same layout
                // -- eg. Claude Code working while Codex CLI does not -- so
                // the chosen path and the exact bytes are what a bug report
                // needs. See `is_ctrl_chord_of_interest`.
                let diag_ctrl = is_ctrl_chord_of_interest(&window_key.key, modifiers);

                let res = if let Some(encoded) = self.encode_win32_input(&pane, &window_key) {
                    log::trace!("diag: chose win32-input-mode path, encoded={:?}", encoded);
                    if diag_ctrl {
                        log::debug!(
                            "diag: passthrough via win32-input-mode, key={:?} mods={:?} \
                             win32_uni_char={:?} encoded={:?}",
                            window_key.key,
                            modifiers,
                            window_key.win32_uni_char,
                            encoded,
                        );
                    }
                    if self.config.debug_key_events {
                        log::info!("win32: Encoded input as {:?}", encoded);
                    }
                    pane.writer()
                        .write_all(encoded.as_bytes())
                        .context("sending win32-input-mode encoded data")
                } else if let Some(encoded) = self.encode_kitty_input(&pane, &window_key) {
                    log::trace!("diag: chose kitty path, encoded={:?}", encoded);
                    if diag_ctrl {
                        log::debug!(
                            "diag: passthrough via kitty, key={:?} mods={:?} encoded={:?}",
                            window_key.key,
                            modifiers,
                            encoded,
                        );
                    }
                    if self.config.debug_key_events {
                        log::info!("kitty: Encoded input as {:?}", encoded);
                    }
                    pane.writer()
                        .write_all(encoded.as_bytes())
                        .context("sending kitty encoded data")
                } else {
                    log::trace!("diag: chose legacy pane.key_down/key_up path");
                    if diag_ctrl {
                        log::debug!(
                            "diag: passthrough via legacy pane.key_{}, key={:?} mods={:?} \
                             (pane negotiated {:?}, allow_win32_input_mode={}, \
                             enable_kitty_keyboard={})",
                            if window_key.key_is_down { "down" } else { "up" },
                            key,
                            modifiers,
                            pane.get_keyboard_encoding(),
                            self.config.allow_win32_input_mode,
                            self.config.enable_kitty_keyboard,
                        );
                    }
                    if self.config.debug_key_events {
                        log::info!(
                            "send to pane {} key={:?} mods={:?}",
                            if window_key.key_is_down { "DOWN" } else { "UP" },
                            key,
                            modifiers
                        );
                    }

                    if window_key.key_is_down {
                        pane.key_down(key, modifiers)
                    } else {
                        pane.key_up(key, modifiers)
                    }
                };

                if res.is_ok() {
                    if window_key.key_is_down
                        && !key.is_modifier()
                        && self.pane_state(pane.pane_id()).overlay.is_none()
                    {
                        self.maybe_scroll_to_bottom_for_input(&pane);
                    }
                    if window_key.key_is_down
                        && self.config.hide_mouse_cursor_when_typing
                        && !key.is_modifier()
                    {
                        context.set_cursor(None);
                    }
                    if !key.is_modifier() {
                        context.invalidate();
                    }
                }
            }
            Key::Composed(s) => {
                if !window_key.key_is_down {
                    return;
                }
                if leader_active {
                    // Leader was pressed and this non-modifier keypress isn't
                    // a registered key binding; swallow this event and cancel
                    // the leader modifier.
                    self.leader_done();
                    return;
                }
                self.key_table_state.did_process_key();
                if self.config.debug_key_events {
                    log::info!("send to pane string={:?}", s);
                }
                pane.writer().write_all(s.as_bytes()).ok();
                self.maybe_scroll_to_bottom_for_input(&pane);
                context.invalidate();
            }
            Key::None => {}
        }
    }

    pub fn win_key_code_to_termwiz_key_code(&self, key: &::window::KeyCode) -> Key {
        use ::termwiz::input::KeyCode as KC;
        use ::window::KeyCode as WK;

        let code = match key {
            // TODO: consider eliminating these codes from termwiz::input::KeyCode
            WK::Char('\r') => KC::Enter,
            WK::Char('\t') => KC::Tab,
            WK::Char('\u{08}') => {
                if self.config.swap_backspace_and_delete {
                    KC::Delete
                } else {
                    KC::Backspace
                }
            }
            WK::Char('\u{7f}') => {
                if self.config.swap_backspace_and_delete {
                    KC::Backspace
                } else {
                    KC::Delete
                }
            }
            WK::Char('\u{1b}') => KC::Escape,
            WK::RawCode(_) => return Key::None,
            WK::Physical(phys) => {
                return self.win_key_code_to_termwiz_key_code(&phys.to_key_code())
            }

            WK::Char(c) => KC::Char(*c),
            WK::Composed(ref s) => {
                let mut chars = s.chars();
                if let Some(first_char) = chars.next() {
                    if chars.next().is_none() {
                        // Was just a single char after all
                        return self.win_key_code_to_termwiz_key_code(&WK::Char(first_char));
                    }
                }
                return Key::Composed(s.to_owned());
            }
            WK::Function(f) => KC::Function(*f),
            WK::LeftArrow => KC::LeftArrow,
            WK::RightArrow => KC::RightArrow,
            WK::UpArrow => KC::UpArrow,
            WK::DownArrow => KC::DownArrow,
            WK::Home => KC::Home,
            WK::End => KC::End,
            WK::PageUp => KC::PageUp,
            WK::PageDown => KC::PageDown,
            WK::Insert => KC::Insert,
            WK::Hyper => KC::Hyper,
            WK::Super => KC::Super,
            WK::Meta => KC::Meta,
            WK::Cancel => KC::Cancel,
            WK::Clear => KC::Clear,
            WK::Shift => KC::Shift,
            WK::LeftShift => KC::LeftShift,
            WK::RightShift => KC::RightShift,
            WK::Control => KC::Control,
            WK::LeftControl => KC::LeftControl,
            WK::RightControl => KC::RightControl,
            WK::Alt => KC::Alt,
            WK::LeftAlt => KC::LeftAlt,
            WK::RightAlt => KC::RightAlt,
            WK::Pause => KC::Pause,
            WK::CapsLock => KC::CapsLock,
            WK::VoidSymbol => return Key::None,
            WK::Select => KC::Select,
            WK::Print => KC::Print,
            WK::Execute => KC::Execute,
            WK::PrintScreen => KC::PrintScreen,
            WK::Help => KC::Help,
            WK::LeftWindows => KC::LeftWindows,
            WK::RightWindows => KC::RightWindows,
            WK::Sleep => KC::Sleep,
            WK::Multiply => KC::Multiply,
            WK::Applications => KC::Applications,
            WK::Add => KC::Add,
            WK::Numpad(0) => KC::Numpad0,
            WK::Numpad(1) => KC::Numpad1,
            WK::Numpad(2) => KC::Numpad2,
            WK::Numpad(3) => KC::Numpad3,
            WK::Numpad(4) => KC::Numpad4,
            WK::Numpad(5) => KC::Numpad5,
            WK::Numpad(6) => KC::Numpad6,
            WK::Numpad(7) => KC::Numpad7,
            WK::Numpad(8) => KC::Numpad8,
            WK::Numpad(9) => KC::Numpad9,
            WK::Numpad(_) => return Key::None,
            WK::Separator => KC::Separator,
            WK::Subtract => KC::Subtract,
            WK::Decimal => KC::Decimal,
            WK::Divide => KC::Divide,
            WK::NumLock => KC::NumLock,
            WK::ScrollLock => KC::ScrollLock,
            WK::Copy => KC::Copy,
            WK::Cut => KC::Cut,
            WK::Paste => KC::Paste,
            WK::BrowserBack => KC::BrowserBack,
            WK::BrowserForward => KC::BrowserForward,
            WK::BrowserRefresh => KC::BrowserRefresh,
            WK::BrowserStop => KC::BrowserStop,
            WK::BrowserSearch => KC::BrowserSearch,
            WK::BrowserFavorites => KC::BrowserFavorites,
            WK::BrowserHome => KC::BrowserHome,
            WK::VolumeMute => KC::VolumeMute,
            WK::VolumeDown => KC::VolumeDown,
            WK::VolumeUp => KC::VolumeUp,
            WK::MediaNextTrack => KC::MediaNextTrack,
            WK::MediaPrevTrack => KC::MediaPrevTrack,
            WK::MediaStop => KC::MediaStop,
            WK::MediaPlayPause => KC::MediaPlayPause,
            WK::ApplicationLeftArrow => KC::ApplicationLeftArrow,
            WK::ApplicationRightArrow => KC::ApplicationRightArrow,
            WK::ApplicationUpArrow => KC::ApplicationUpArrow,
            WK::ApplicationDownArrow => KC::ApplicationDownArrow,
            WK::KeyPadHome => KC::KeyPadHome,
            WK::KeyPadEnd => KC::KeyPadEnd,
            WK::KeyPadBegin => KC::KeyPadBegin,
            WK::KeyPadPageUp => KC::KeyPadPageUp,
            WK::KeyPadPageDown => KC::KeyPadPageDown,
        };
        Key::Code(code)
    }
}
