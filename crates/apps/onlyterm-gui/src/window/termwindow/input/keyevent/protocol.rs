use super::*;
use onlyterm_mux::pane::CachePolicy;
use termwiz::input::KeyboardEncoding;

impl super::super::TermWindow {
    pub(crate) fn encode_win32_input(
        &self,
        pane: &Arc<dyn Pane>,
        key: &KeyEvent,
    ) -> Option<String> {
        if !self.config.allow_win32_input_mode
            || pane.get_keyboard_encoding() != KeyboardEncoding::Win32
        {
            return None;
        }
        // Only a CTRL+<letter> chord can be affected by the substitution, and
        // deciding it costs a walk of the pane's process tree -- so the
        // question is asked only when the answer can matter. The first cut of
        // this called it for every key event that reached passthrough, i.e.
        // on ordinary typing, which is exactly the per-keystroke process-tree
        // work this codebase has already had to remove from the render path
        // once (see `bidi_process_override` in render/pane.rs).
        let ctrl_letter_as_char = matches!(&key.key, KeyCode::Char(c) if c.is_ascii_alphabetic())
            && key.modifiers.contains(Modifiers::CTRL)
            && self.ctrl_letter_as_char_for(pane);
        key.encode_win32_input_mode(ctrl_letter_as_char)
    }

    /// Whether this pane's process tree contains one of the applications
    /// that need a Ctrl+<letter> chord to carry the plain letter rather
    /// than the ASCII control code; see
    /// `Config::win32_input_ctrl_letter_as_char_processes` and
    /// docs/codex-cyrillic-ctrl-chords.md.
    ///
    /// Re-derived per call from a fresh name-only snapshot on Windows, so
    /// starting/leaving Codex changes the encoding on the next chord, without
    /// waiting for the asynchronous title/cwd process cache to catch up. The
    /// cost is only paid for CTRL chords, which arrive at human speed --
    /// this deliberately does not run on ordinary typing.
    fn ctrl_letter_as_char_for(&self, pane: &Arc<dyn Pane>) -> bool {
        self.pane_runs_any_of(
            pane,
            &self.config.win32_input_ctrl_letter_as_char_processes,
            "ctrl_letter_as_char",
        )
    }

    /// Whether SHIFT+Enter should be sent to `pane` as ESC CR rather than as
    /// a faithful modified-Enter record; see
    /// `Config::shift_enter_esc_cr_processes`.
    pub(crate) fn shift_enter_esc_cr_for(&self, pane: &Arc<dyn Pane>) -> bool {
        self.pane_runs_any_of(
            pane,
            &self.config.shift_enter_esc_cr_processes,
            "shift_enter_esc_cr",
        )
    }

    /// Whether any executable in `pane`'s process tree matches `wanted`.
    ///
    /// Matched against the whole tree, not the foreground process: a program
    /// started through a wrapper hides behind whichever link is youngest --
    /// Codex CLI runs as `codex.cmd` -> node -> `codex.exe` and the
    /// foreground call returns `node_repl.exe`, so a foreground-only match
    /// silently never fires.
    ///
    /// Callers must gate this on the chord actually being one that could be
    /// affected. It reads a process snapshot (lightweight, but not free), and
    /// `encode_win32_input` runs for every key that reaches passthrough, not
    /// only for chords.
    fn pane_runs_any_of(&self, pane: &Arc<dyn Pane>, wanted: &[String], what: &str) -> bool {
        if wanted.is_empty() {
            log::debug!(
                "diag: key-compat {} pane={} disabled: empty process list",
                what,
                pane.pane_id(),
            );
            return false;
        }
        let Some(names) = pane.get_process_tree_exe_names(CachePolicy::AllowStale) else {
            log::debug!(
                "diag: key-compat {} pane={} wanted={:?} tree unavailable -> false",
                what,
                pane.pane_id(),
                wanted,
            );
            return false;
        };
        let matched = wanted
            .iter()
            .any(|want| names.iter().any(|have| have.eq_ignore_ascii_case(want)));
        log::debug!(
            "diag: key-compat {} pane={} wanted={:?} tree={:?} -> {}",
            what,
            pane.pane_id(),
            wanted,
            names,
            matched,
        );
        matched
    }

    pub(crate) fn encode_kitty_input(
        &self,
        pane: &Arc<dyn Pane>,
        key: &KeyEvent,
    ) -> Option<String> {
        if !self.config.enable_kitty_keyboard {
            return None;
        }
        if let KeyboardEncoding::Kitty(flags) = pane.get_keyboard_encoding() {
            Some(key.encode_kitty(flags))
        } else {
            None
        }
    }

    /// Encodes `key` through whatever keyboard protocol the pane's app has
    /// negotiated (win32-input-mode or kitty), or `None` if it hasn't
    /// negotiated one that can represent this event. Used for synthetic
    /// key events raised from key *assignments* (eg. `CopySelectionOrInterrupt`,
    /// `SendEnterOrNewline`) that need to respect the app's negotiated
    /// protocol rather than always writing a hardcoded legacy byte.
    pub(crate) fn encode_via_negotiated_protocol(
        &self,
        pane: &Arc<dyn Pane>,
        key: &KeyEvent,
    ) -> Option<String> {
        self.encode_win32_input(pane, key)
            .or_else(|| self.encode_kitty_input(pane, key))
    }

    pub(super) fn lookup_key(
        &mut self,
        pane: &Arc<dyn Pane>,
        keycode: &KeyCode,
        mods: Modifiers,
        only_key_bindings: OnlyKeyBindings,
    ) -> Option<(KeyTableEntry, Option<String>)> {
        if let Some(overlay) = self.pane_state(pane.pane_id()).overlay.as_mut() {
            if let Some((entry, table_name)) = overlay.key_table_state.lookup_key(
                &self.input_map,
                keycode,
                mods,
                only_key_bindings,
            ) {
                return Some((entry, table_name.map(|s| s.to_string())));
            }
        }
        if let Some((entry, table_name)) =
            self.key_table_state
                .lookup_key(&self.input_map, keycode, mods, only_key_bindings)
        {
            return Some((entry, table_name.map(|s| s.to_string())));
        }
        self.input_map
            .lookup_key(keycode, mods, None)
            .map(|entry| (entry, None))
    }
}
