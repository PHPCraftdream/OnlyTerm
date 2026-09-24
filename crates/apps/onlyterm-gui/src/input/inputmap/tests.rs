use super::*;
use onlyterm_config::keyassignment::ClipboardCopyDestination;
use onlyterm_dynamic::Value;
use window::PhysKeyCode;

/// Builds an `InputMap` from a config whose `keys:` list mirrors the
/// shape of a real user config: a mix of `phys:`-prefixed and plain
/// bindings, most of them CTRL chords, and *none* of them touching
/// CTRL+C. Note: after the Ctrl+J protocol-aware fix, CTRL+J now
/// has a default binding, so this config no longer tests that it stays
/// free - we keep this config structure for testing that unrelated
/// user bindings don't clobber the defaults they don't explicitly
/// touch.
///
/// The classic way for a config engine to break "everything except the
/// keys I explicitly bound" is for a non-empty user `keys:` list to
/// *replace* the built-in default table rather than extend it (or for
/// a `(key, mods)` normalization collision to silently evict a default
/// entry). This constructs the map through exactly the same path the
/// real GUI uses -- `Config::from_dynamic` -> `Config::key_bindings()`
/// -> `InputMap::new`'s merge of `CommandDef::default_key_assignments`
/// -- so that such a regression would show up here.
fn input_map_with_user_style_keys() -> InputMap {
    use onlyterm_config::Config;
    use onlyterm_dynamic::{FromDynamic, FromDynamicOptions, Object};

    fn s(v: &str) -> Value {
        Value::String(v.to_string())
    }

    fn object(pairs: Vec<(&str, Value)>) -> Value {
        Value::Object(
            pairs
                .into_iter()
                .map(|(k, v)| (s(k), v))
                .collect::<Object>(),
        )
    }

    fn binding(key: &str, mods: &str, action: Value) -> Value {
        object(vec![("key", s(key)), ("mods", s(mods)), ("action", action)])
    }

    fn send_key(key: &str, mods: Option<&str>) -> Value {
        let mut inner = vec![("key", s(key))];
        if let Some(mods) = mods {
            inner.push(("mods", s(mods)));
        }
        object(vec![("SendKey", object(inner))])
    }

    let mut keys = vec![
        binding(
            "phys:v",
            "CTRL",
            object(vec![("PasteFrom", s("Clipboard"))]),
        ),
        binding("Enter", "SHIFT", send_key("Enter", None)),
        binding("Enter", "CTRL", send_key("Enter", None)),
        binding(
            "phys:t",
            "CTRL",
            object(vec![("SpawnTab", s("CurrentPaneDomain"))]),
        ),
        binding("l", "CTRL", send_key("l", Some("CTRL"))),
        binding("L", "CTRL", send_key("l", Some("CTRL"))),
        binding("phys:l", "CTRL", send_key("l", Some("CTRL"))),
        binding(
            "phys:f",
            "CTRL",
            object(vec![(
                "Search",
                object(vec![("CaseInSensitiveString", s(""))]),
            )]),
        ),
        binding(
            "Tab",
            "CTRL",
            object(vec![("ActivateTabRelative", Value::I64(1))]),
        ),
        binding(
            "Tab",
            "CTRL|SHIFT",
            object(vec![("ActivateTabRelative", Value::I64(-1))]),
        ),
        binding("=", "CTRL", s("IncreaseFontSize")),
        binding("-", "CTRL", s("DecreaseFontSize")),
        binding("phys:0", "CTRL", s("ResetFontSize")),
    ];
    for n in 1..=9u8 {
        keys.push(binding(
            &format!("phys:{n}"),
            "CTRL",
            object(vec![("ActivateTab", Value::I64(i64::from(n - 1)))]),
        ));
    }

    let config = Config::from_dynamic(
        &object(vec![("keys", Value::Array(keys.into_iter().collect()))]),
        FromDynamicOptions::default(),
    )
    .expect("user-style config must deserialize")
    .compute_extra_defaults(None);

    InputMap::new(&ConfigHandle::from_config(config))
}

/// Regression test: a user config that binds *some* CTRL chords must not
/// disturb the built-in defaults for the chords it does not mention.
/// CTRL+C must still be `CopySelectionOrInterrupt` (so it interrupts when
/// nothing is selected). After the Ctrl+J protocol-aware fix, CTRL+J now
/// has a default binding (SendChar(Modifiers::CTRL, 'j')), so this test
/// verifies that it is NOT clobbered by unrelated user bindings.
#[test]
fn user_key_overrides_do_not_clobber_ctrl_c_and_ctrl_j_defaults() {
    let input_map = input_map_with_user_style_keys();

    for key in [KeyCode::Physical(PhysKeyCode::C), KeyCode::Char('c')] {
        let entry = input_map
            .lookup_key(&key, Modifiers::CTRL, None)
            .unwrap_or_else(|| {
                panic!(
                    "CTRL+C ({:?}) must keep its default binding when the user \
                         config defines unrelated `keys:` overrides",
                    key
                )
            });
        assert_eq!(entry.action, KeyAssignment::CopySelectionOrInterrupt);
    }

    // After the fix, Ctrl+J has a default binding that should NOT be
    // clobbered by unrelated user bindings
    for key in [KeyCode::Char('j'), KeyCode::Physical(PhysKeyCode::J)] {
        let entry = input_map
            .lookup_key(&key, Modifiers::CTRL, None)
            .unwrap_or_else(|| {
                panic!(
                    "CTRL+J ({:?}) must keep its default protocol-aware binding \
                         when the user config defines unrelated `keys:` overrides",
                    key
                )
            });
        assert_eq!(entry.action, KeyAssignment::SendChar(Modifiers::CTRL, 'j'));
    }

    // Sanity check the other direction: the user's own overrides really
    // did make it into the same table (otherwise the assertions above
    // would pass vacuously against a map that ignored the user config).
    assert_eq!(
        input_map
            .lookup_key(&KeyCode::Physical(PhysKeyCode::V), Modifiers::CTRL, None)
            .expect("user CTRL+V override must be present")
            .action,
        KeyAssignment::PasteFrom(ClipboardPasteSource::Clipboard)
    );
}

fn no_mods() -> MouseEventTriggerMods {
    MouseEventTriggerMods {
        mods: Modifiers::NONE,
        mouse_reporting: false,
        alt_screen: MouseEventAltScreen::False,
    }
}

/// Regression test: a left-click release on a hyperlink must keep
/// opening it (unchanged behavior), while a right-click release on a
/// hyperlink must copy its URL to the clipboard instead of opening it.
#[test]
fn right_click_copies_left_click_opens_hyperlink() {
    let input_map = InputMap::default_input_map();

    let left_click_up = MouseEventTrigger::Up {
        streak: 1,
        button: MouseButton::Left,
    };
    let action = input_map
        .lookup_mouse(left_click_up, no_mods())
        .expect("left-click-up has a default binding");
    assert_eq!(
        action,
        KeyAssignment::CompleteSelectionOrOpenLinkAtMouseCursor(
            ClipboardCopyDestination::ClipboardAndPrimarySelection
        )
    );

    let right_click_up = MouseEventTrigger::Up {
        streak: 1,
        button: MouseButton::Right,
    };
    let action = input_map
        .lookup_mouse(right_click_up, no_mods())
        .expect("right-click-up has a default binding");
    assert_eq!(
        action,
        KeyAssignment::CopyLinkAtMouseCursor(
            ClipboardCopyDestination::ClipboardAndPrimarySelection
        )
    );
}

/// Regression test for layout-independent modifier chords (see task
/// tracked as "Сделать сопоставление стандартных Ctrl-сочетаний
/// независимым от раскладки/языка по умолчанию").
///
/// The default binding for "copy to clipboard" is CTRL+SHIFT+C
/// (registered via the SUPER permutation in `CommandDef::permute_keys`).
/// On a non-Latin keyboard layout (eg: Russian ЙЦУКЕН) the physical "C"
/// key does not produce the Unicode character 'c'/'C', so a real
/// WM_KEYDOWN on Windows would resolve `ToUnicode` to a Cyrillic
/// character instead. The physical-key-first lookup pass performed by
/// `raw_key_event_impl` (see `keyevent.rs`) relies on the default table
/// containing a `KeyCode::Physical(PhysKeyCode::C)` entry alongside the
/// mapped `KeyCode::Char('C')` one; this test asserts that entry exists
/// and resolves to the same action, without having to drive a real
/// WM_KEYDOWN/ToUnicode round trip on an actual Russian keyboard layout.
#[test]
fn ctrl_shift_c_resolves_via_physical_key_regardless_of_layout() {
    let input_map = InputMap::default_input_map();
    let mods = Modifiers::CTRL | Modifiers::SHIFT;

    let mapped = input_map
        .lookup_key(&KeyCode::Char('C'), mods, None)
        .expect("mapped CTRL+SHIFT+C has a default Copy binding");
    assert_eq!(
        mapped.action,
        KeyAssignment::CopyTo(ClipboardCopyDestination::Clipboard)
    );

    // Simulates the physical-key-first lookup pass: even though the
    // active keyboard layout may have produced a completely different
    // Unicode character for this physical position, the position
    // itself (physical "C") must still resolve to the same Copy action.
    let physical = input_map
        .lookup_key(&KeyCode::Physical(PhysKeyCode::C), mods, None)
        .expect("physical CTRL+SHIFT+C must resolve to Copy independent of keyboard layout");
    assert_eq!(physical.action, mapped.action);
}

#[test]
fn ctrl_shift_v_resolves_via_physical_key_regardless_of_layout() {
    let input_map = InputMap::default_input_map();
    let mods = Modifiers::CTRL | Modifiers::SHIFT;

    let mapped = input_map
        .lookup_key(&KeyCode::Char('V'), mods, None)
        .expect("mapped CTRL+SHIFT+V has a default Paste binding");
    assert_eq!(
        mapped.action,
        KeyAssignment::PasteFrom(ClipboardPasteSource::Clipboard)
    );

    let physical = input_map
        .lookup_key(&KeyCode::Physical(PhysKeyCode::V), mods, None)
        .expect("physical CTRL+SHIFT+V must resolve to Paste independent of keyboard layout");
    assert_eq!(physical.action, mapped.action);
}

#[test]
fn ctrl_shift_t_spawns_a_tab_and_bare_ctrl_t_does_not() {
    let input_map = InputMap::default_input_map();
    let spawn_tab =
        KeyAssignment::SpawnTab(onlyterm_config::keyassignment::SpawnTabDomain::CurrentPaneDomain);
    let mods = Modifiers::CTRL | Modifiers::SHIFT;

    let mapped = input_map
        .lookup_key(&KeyCode::Char('T'), mods, None)
        .expect("mapped CTRL+SHIFT+T has a default New Tab binding");
    assert_eq!(mapped.action, spawn_tab);

    let physical = input_map
        .lookup_key(&KeyCode::Physical(PhysKeyCode::T), mods, None)
        .expect("physical CTRL+SHIFT+T must resolve to New Tab independent of keyboard layout");
    assert_eq!(physical.action, spawn_tab);

    // Bare CTRL+T (no SHIFT) must NOT spawn a tab by default -- it's
    // commonly used by the shell/readline running inside the terminal.
    for key in [KeyCode::Char('t'), KeyCode::Physical(PhysKeyCode::T)] {
        if let Some(entry) = input_map.lookup_key(&key, Modifiers::CTRL, None) {
            assert_ne!(
                entry.action, spawn_tab,
                "bare CTRL+T ({:?}) must not be a default New Tab binding",
                key
            );
        }
    }
}

/// Control test: plain, unmodified text entry (no CTRL/SUPER) must be
/// completely unaffected by the physical-fallback synthesis above.
/// Typing a bare Cyrillic character (eg: on a Russian layout, the
/// physical "C" key normally produces 'с') must not accidentally
/// resolve to any key binding at all: CJK, Cyrillic and any other
/// non-Latin text input must keep behaving exactly as before this
/// change, since `permute_keys` only ever synthesizes physical
/// fallbacks for CTRL/SUPER chords.
#[test]
fn bare_non_latin_char_input_is_not_affected_by_physical_fallback() {
    let input_map = InputMap::default_input_map();

    // Bare Cyrillic 'с' (as ToUnicode would produce for the physical
    // "C" key on a Russian ЙЦУКЕН layout) with no modifiers at all.
    assert!(
        input_map
            .lookup_key(&KeyCode::Char('с'), Modifiers::NONE, None)
            .is_none(),
        "bare non-Latin text entry must never be captured by a key binding"
    );

    // Also confirm that plain, unmodified 'c' (Latin) has no default
    // key binding either -- Copy/Paste are only bound with CTRL+SHIFT
    // or SUPER, never with plain unmodified text entry.
    assert!(
        input_map
            .lookup_key(&KeyCode::Char('c'), Modifiers::NONE, None)
            .is_none(),
        "bare 'c' with no modifiers must not be captured by the Copy binding"
    );
}

/// Regression test for a real bug: the layout-independent physical-key
/// synthesis in `CommandDef::permute_keys` used to also register a bare
/// CTRL (no SHIFT) alias for every SUPER-bound single-letter command,
/// so plain CTRL+C ended up unconditionally bound to "copy to
/// clipboard" and could never reach the pty - breaking Ctrl+C as an
/// interrupt key entirely. CTRL+C must resolve to CopySelectionOrInterrupt
/// (copy-if-selected, otherwise pass a literal Ctrl+C through), never to
/// a plain CopyTo/CopyTextTo.
#[test]
fn ctrl_c_is_copy_or_interrupt_not_unconditional_copy() {
    let input_map = InputMap::default_input_map();

    let entry = input_map
        .lookup_key(&KeyCode::Physical(PhysKeyCode::C), Modifiers::CTRL, None)
        .expect("CTRL+C (physical) must have a default binding");
    assert_eq!(entry.action, KeyAssignment::CopySelectionOrInterrupt);

    let entry = input_map
        .lookup_key(&KeyCode::Char('c'), Modifiers::CTRL, None)
        .expect("CTRL+c must have a default binding");
    assert_eq!(entry.action, KeyAssignment::CopySelectionOrInterrupt);
}

/// Regression test for a real bug: `PasteFrom(Clipboard)`'s default
/// `keys` only listed SUPER+v (macOS Cmd+V) and the OS-level "Paste"
/// gesture, which `permute_keys` only expands into CTRL+SHIFT+v
/// alternates (see the SUPER-branch synthesis above) - never plain,
/// unmodified-by-shift CTRL+v. So on Windows/Linux, plain CTRL+V did
/// nothing at all. Fixed by adding an explicit CTRL+v entry.
#[test]
fn ctrl_v_pastes_from_clipboard() {
    let input_map = InputMap::default_input_map();

    let entry = input_map
        .lookup_key(&KeyCode::Physical(PhysKeyCode::V), Modifiers::CTRL, None)
        .expect("CTRL+V (physical) must have a default binding");
    assert_eq!(
        entry.action,
        KeyAssignment::PasteFrom(ClipboardPasteSource::Clipboard)
    );

    let entry = input_map
        .lookup_key(&KeyCode::Char('v'), Modifiers::CTRL, None)
        .expect("CTRL+v must have a default binding");
    assert_eq!(
        entry.action,
        KeyAssignment::PasteFrom(ClipboardPasteSource::Clipboard)
    );
}

/// Regression tests for reliably sending a newline to the pty via
/// CTRL+Enter, SHIFT+Enter and CTRL+J (see task "Добавить три
/// сочетания клавиш для перевода строки: Ctrl+Enter, Shift+Enter,
/// Ctrl+J").
///
/// CTRL+Enter and SHIFT+Enter have no natural pty-encoding behavior:
/// without an explicit default binding they would fall through to
/// `KeyCode::Enter`'s CSI-u/modified-key encoding path (see
/// `termwiz::input::KeyCode::encode`), which -- absent an app that
/// negotiated CSI-u/kitty keyboard protocol -- degrades to a bare
/// carriage return ('\r'), identical to plain Enter and NOT a line
/// feed. So both are given an explicit default `SendEnterOrNewline`
/// binding, which encodes through whatever protocol the app has
/// negotiated (so eg. Codex CLI, which negotiates kitty keyboard
/// protocol, gets the disambiguated CSI-u form it expects), falling
/// back to a raw '\n' only for apps that haven't negotiated one.
/// CTRL+Enter is deliberately bound to the *CTRL+J* chord rather than to
/// `SendEnterOrNewline(CTRL)`: a faithful modified-Enter is what the
/// chord literally is, but hardly any application acts on one (Codex CLI
/// ignores it, and so does Windows Terminal), whereas CTRL+J is the
/// universally understood "insert a line feed" chord. Both chords
/// therefore insert a newline, which is what a user pressing either of
/// them is asking for.
#[test]
fn ctrl_enter_sends_newline() {
    let input_map = InputMap::default_input_map();

    let entry = input_map
        .lookup_key(&KeyCode::Char('\r'), Modifiers::CTRL, None)
        .expect("CTRL+Enter must have a default binding");
    assert_eq!(entry.action, KeyAssignment::SendChar(Modifiers::CTRL, 'j'));
}

#[test]
fn shift_enter_sends_newline() {
    let input_map = InputMap::default_input_map();

    let entry = input_map
        .lookup_key(&KeyCode::Char('\r'), Modifiers::SHIFT, None)
        .expect("SHIFT+Enter must have a default binding");
    assert_eq!(
        entry.action,
        KeyAssignment::SendEnterOrNewline(Modifiers::SHIFT)
    );
}

/// Regression test: CTRL+Enter must also resolve via its physical-key
/// fallback, consistent with the layout-independent CTRL-chord handling
/// added in `CommandDef::permute_keys` (see task "Сделать сопоставление
/// стандартных Ctrl-сочетаний независимым от раскладки/языка по
/// умолчанию"). Enter is not a letter key, so it is not affected by
/// non-Latin layouts in practice, but the physical fallback entry
/// should still be present and resolve to the same action.
#[test]
fn ctrl_enter_resolves_via_physical_key_too() {
    let input_map = InputMap::default_input_map();
    let mapped = input_map
        .lookup_key(&KeyCode::Char('\r'), Modifiers::CTRL, None)
        .expect("mapped CTRL+Enter has a default newline binding");

    let physical = input_map
        .lookup_key(
            &KeyCode::Physical(PhysKeyCode::Return),
            Modifiers::CTRL,
            None,
        )
        .expect("physical CTRL+Enter must resolve to the same newline binding");
    assert_eq!(physical.action, mapped.action);
}

/// Regression test: CTRL+J now has a protocol-aware default binding
/// that sends Ctrl+j through whatever keyboard protocol the app has
/// negotiated (win32-input-mode or kitty), falling back to a raw 0x0A
/// byte if no protocol was negotiated. This fixes the issue where
/// apps like Codex CLI (which negotiates win32-input-mode) expect all
/// keystrokes to arrive via the negotiated protocol and may not
/// correctly process a stray raw 0x0A byte appearing mid-stream.
///
/// Before this fix, Ctrl+J fell through to the terminal's standard
/// ASCII control-code encoding (ctrl_mapping('j') -> 0x0A) unconditionally,
/// which worked for legacy apps but broke protocol-aware apps.
#[test]
fn ctrl_j_has_protocol_aware_default_binding() {
    let input_map = InputMap::default_input_map();

    let entry = input_map
        .lookup_key(&KeyCode::Char('j'), Modifiers::CTRL, None)
        .expect("CTRL+J must have a default protocol-aware binding");
    assert_eq!(
        entry.action,
        KeyAssignment::SendChar(Modifiers::CTRL, 'j'),
        "CTRL+J must be bound to SendChar(Modifiers::CTRL, 'j')"
    );

    // The physical J key should also resolve to the same action
    // (layout-independent physical-key fallback, synthesized in CommandDef::permute_keys)
    let physical = input_map
        .lookup_key(&KeyCode::Physical(PhysKeyCode::J), Modifiers::CTRL, None)
        .expect("physical CTRL+J must resolve to the same protocol-aware binding");
    assert_eq!(
        physical.action,
        KeyAssignment::SendChar(Modifiers::CTRL, 'j'),
        "physical CTRL+J must resolve to the same SendChar(Modifiers::CTRL, 'j') binding"
    );
}

/// Regression test for the bug that made CTRL+J do *nothing at all* in
/// Codex CLI: `SendChar`'s handler used to build its synthetic key event
/// with `raw_code: 0, scan_code: 0`, and those two zeros go straight into
/// the `Vk`/`Sc` fields of the win32-input-mode sequence. ConPTY and
/// crossterm recover the character by calling `ToUnicode(vk, sc, ..)`,
/// which fails outright for `vk == 0`, so the keypress was dropped before
/// the application ever saw it. CTRL+C worked throughout only because its
/// handler hardcoded the real VK_C/scan pair.
///
/// This drives the *production* constructor rather than a hand-built
/// look-alike event: the previous version of this test hardcoded the
/// correct VK/scan values itself, so it passed against the very bug it
/// was meant to catch.
#[cfg(windows)]
#[test]
fn send_char_j_encodes_correctly_via_win32_input_mode() {
    let encoded =
        crate::termwindow::actions::synthetic_key_down(KeyCode::Char('j'), Modifiers::CTRL)
            .encode_win32_input_mode(false)
            .expect("Ctrl+j should encode via win32-input-mode");

    // ESC [ Vk ; Sc ; Uc ; Kd ; Cs ; Rc _ -- VK_J (0x4a = 74), the J key's
    // scan code (0x24 = 36), the Ctrl+J control code (0x0a = 10), key
    // down, LEFT_CTRL_PRESSED (0x08), repeat count 1. This is byte for
    // byte what Windows Terminal sends for a real CTRL+J keypress.
    assert_eq!(encoded, "\u{1b}[74;36;10;1;8;1_");
}

/// CTRL+Enter is bound to the CTRL+J chord (see `ctrl_enter_sends_newline`),
/// so it must put the exact same bytes on the wire.
#[cfg(windows)]
#[test]
fn ctrl_enter_encodes_as_the_ctrl_j_chord() {
    let input_map = InputMap::default_input_map();
    let entry = input_map
        .lookup_key(&KeyCode::Char('\r'), Modifiers::CTRL, None)
        .expect("CTRL+Enter must have a default binding");

    let KeyAssignment::SendChar(mods, c) = entry.action else {
        panic!(
            "CTRL+Enter must be bound to SendChar, got {:?}",
            entry.action
        );
    };

    let encoded = crate::termwindow::actions::synthetic_key_down(KeyCode::Char(c), mods)
        .encode_win32_input_mode(false)
        .expect("CTRL+Enter's bound chord should encode via win32-input-mode");
    assert_eq!(encoded, "\u{1b}[74;36;10;1;8;1_");
}

/// SHIFT+Enter has no equivalent problem to CTRL+Enter's -- applications
/// (Codex CLI included) do act on a faithful modified-Enter here -- so it
/// keeps its `SendEnterOrNewline` binding, and must encode as a real
/// Enter keypress with SHIFT held.
#[cfg(windows)]
#[test]
fn shift_enter_encodes_as_a_real_modified_enter() {
    let encoded =
        crate::termwindow::actions::synthetic_key_down(KeyCode::Char('\r'), Modifiers::SHIFT)
            .encode_win32_input_mode(false)
            .expect("Shift+Enter should encode via win32-input-mode");

    // VK_RETURN (0x0d = 13), Enter's scan code (0x1c = 28), CR (0x0d = 13),
    // key down, SHIFT_PRESSED (0x10 = 16), repeat count 1.
    assert_eq!(encoded, "\u{1b}[13;28;13;1;16;1_");
}

/// Builds an `InputMap` from a minimal user-style config containing an
/// `Alt+v` binding, plus a `Ctrl|Alt+q` binding that stands in for an
/// AltGr-composed chord (Windows reports AltGr as CTRL|ALT together).
///
/// This exists to test `InputMap::new`'s `synthesize_physical_fallbacks`
/// pass, which is what actually needed fixing: `CommandDef::permute_keys`
/// only ever runs over *built-in default* bindings and only ever
/// synthesizes a physical-key twin for CTRL/SUPER chords (never ALT), so
/// a *user-configured* `Alt+v` binding -- which goes straight through
/// `Config::key_bindings` with no fallback synthesis of any kind -- could
/// never resolve via the physical-key-first lookup pass on a non-Latin
/// layout, regardless of that separate `permute_keys` gap.
fn input_map_with_alt_v_user_binding() -> InputMap {
    use onlyterm_config::Config;
    use onlyterm_dynamic::{FromDynamic, FromDynamicOptions, Object};

    fn s(v: &str) -> Value {
        Value::String(v.to_string())
    }

    fn object(pairs: Vec<(&str, Value)>) -> Value {
        Value::Object(
            pairs
                .into_iter()
                .map(|(k, v)| (s(k), v))
                .collect::<Object>(),
        )
    }

    fn binding(key: &str, mods: &str, action: Value) -> Value {
        object(vec![("key", s(key)), ("mods", s(mods)), ("action", action)])
    }

    let keys = vec![
        binding("v", "ALT", s("ActivateCopyMode")),
        // Stands in for an AltGr-composed chord: Windows reports AltGr
        // as CTRL and ALT held together. Must not get a bare-ALT
        // physical twin synthesized for it (trap #1 in the fix).
        binding("q", "CTRL|ALT", s("ActivateCopyMode")),
    ];

    let config = Config::from_dynamic(
        &object(vec![("keys", Value::Array(keys.into_iter().collect()))]),
        FromDynamicOptions::default(),
    )
    .expect("alt-v config must deserialize")
    .compute_extra_defaults(None);

    InputMap::new(&ConfigHandle::from_config(config))
}

/// Regression test: a user-configured `Alt+v` binding (not a built-in
/// default -- see `input_map_with_alt_v_user_binding`'s doc comment for
/// why that distinction matters) must resolve via the physical V key,
/// exactly as a real WM_KEYDOWN on a Russian ЙЦУКЕН layout would deliver
/// it: `ToUnicode` produces 'м' for that physical position, not 'v', so
/// `raw_key_event_impl`'s physical-key-first lookup pass is what actually
/// has to find this binding in practice.
#[test]
fn alt_v_user_binding_resolves_via_physical_key_regardless_of_layout() {
    let input_map = input_map_with_alt_v_user_binding();

    let mapped = input_map
        .lookup_key(&KeyCode::Char('v'), Modifiers::ALT, None)
        .expect("user-configured Alt+v must resolve via the mapped character");
    assert_eq!(mapped.action, KeyAssignment::ActivateCopyMode);

    let physical = input_map
        .lookup_key(&KeyCode::Physical(PhysKeyCode::V), Modifiers::ALT, None)
        .expect("Alt+v must resolve via the physical V key independent of keyboard layout");
    assert_eq!(physical.action, mapped.action);
}

/// Negative case: the Alt+v physical-key fallback must not leak into
/// plain, unmodified typing of the physical V key (eg. Cyrillic 'м'),
/// which is exactly the failure mode `bare_non_latin_char_input_is_not_affected_by_physical_fallback`
/// already guards for CTRL/SUPER chords -- this is the same guarantee,
/// for the newly-covered ALT case.
#[test]
fn bare_physical_v_without_modifiers_is_unaffected_by_alt_fallback() {
    let input_map = input_map_with_alt_v_user_binding();

    assert!(
        input_map
            .lookup_key(&KeyCode::Physical(PhysKeyCode::V), Modifiers::NONE, None)
            .is_none(),
        "the Alt+v physical-key fallback must not leak into plain, \
             unmodified typing of the physical V key"
    );
}

/// Negative case for trap #1 in the physical-fallback synthesis: a
/// CTRL|ALT binding (standing in for an AltGr-composed chord) must not
/// get a bare-ALT physical twin. If it did, a real AltGr-composed
/// character on a European layout could misfire this keybinding instead
/// of inserting the composed character.
#[test]
fn ctrl_alt_altgr_like_binding_does_not_get_a_bare_alt_physical_twin() {
    let input_map = input_map_with_alt_v_user_binding();

    assert!(
        input_map
            .lookup_key(&KeyCode::Physical(PhysKeyCode::Q), Modifiers::ALT, None)
            .is_none(),
        "a CTRL|ALT (AltGr-shaped) binding must not synthesize a bare-ALT physical twin"
    );

    // The CTRL|ALT binding itself must still resolve normally via its
    // mapped character -- this fix must not disturb that.
    let mapped = input_map
        .lookup_key(&KeyCode::Char('q'), Modifiers::CTRL | Modifiers::ALT, None)
        .expect("the CTRL|ALT binding itself must still resolve via its mapped character");
    assert_eq!(mapped.action, KeyAssignment::ActivateCopyMode);
}
