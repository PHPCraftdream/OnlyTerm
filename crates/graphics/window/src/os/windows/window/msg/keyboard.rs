use super::super::input::ime::ImmContext;
use super::super::input::keyboard_layout::{KeyboardLayoutInfo, ResolvedDeadKey};
use super::super::*;
use crate::{DeadKeyStatus, Handled, KeyCode, KeyEvent, Modifiers, RawKeyEvent, WindowEvent};
use onlyterm_config::ImePreeditRendering;
use onlyterm_input_types::KeyboardLedStatus;
use winapi::um::sysinfoapi::GetTickCount;

const GCS_RESULTSTR: DWORD = 0x800;
const GCS_COMPSTR: DWORD = 0x8;
const ISC_SHOWUICOMPOSITIONWINDOW: DWORD = 0x80000000;

/// # Safety
/// `hwnd` must be a valid window handle and the args from a real
/// `WM_IME_SETCONTEXT` message.
pub(super) unsafe fn ime_set_context(
    hwnd: HWND,
    msg: UINT,
    wparam: WPARAM,
    lparam: LPARAM,
) -> Option<LRESULT> {
    let use_system_rendering = {
        let inner = rc_from_hwnd(hwnd)?;
        let inner = inner.borrow();
        inner.config.ime_preedit_rendering == ImePreeditRendering::System
    };

    if use_system_rendering {
        return None;
    }

    // Don't show system CompositionWindow because application itself draws it.
    // Note: DefWindowProcW may trigger other window messages, so we must
    // release the borrow before calling it.
    let lparam = lparam & !(ISC_SHOWUICOMPOSITIONWINDOW as LPARAM);
    let result = DefWindowProcW(hwnd, msg, wparam, lparam);
    Some(result)
}

/// # Safety
/// `hwnd` must be a valid window handle and the args from a real
/// `WM_IME_ENDCOMPOSITION` message.
pub(super) unsafe fn ime_end_composition(
    hwnd: HWND,
    _msg: UINT,
    _wparam: WPARAM,
    _lparam: LPARAM,
) -> Option<LRESULT> {
    // IME was cancelled
    let inner = rc_from_hwnd(hwnd)?;
    let mut inner = inner.borrow_mut();

    if inner.config.ime_preedit_rendering == ImePreeditRendering::System {
        return None;
    }

    inner
        .events
        .dispatch(WindowEvent::AdviseDeadKeyStatus(DeadKeyStatus::None));
    Some(1)
}

/// # Safety
/// `hwnd` must be a valid window handle and the args from a real
/// `WM_IME_COMPOSITION` message.
pub(super) unsafe fn ime_composition(
    hwnd: HWND,
    _msg: UINT,
    _wparam: WPARAM,
    lparam: LPARAM,
) -> Option<LRESULT> {
    let inner = rc_from_hwnd(hwnd)?;
    let mut inner = inner.borrow_mut();

    if inner.config.ime_preedit_rendering == ImePreeditRendering::System {
        return None;
    }

    let imc = ImmContext::get(hwnd);

    let lparam = lparam as DWORD;

    if lparam == 0 {
        // IME was cancelled
        inner
            .events
            .dispatch(WindowEvent::AdviseDeadKeyStatus(DeadKeyStatus::None));
        return Some(1);
    }

    if lparam & GCS_RESULTSTR == 0 {
        // No finished result; continue with the default
        // processing
        if let Ok(composing) = imc.get_str(GCS_COMPSTR) {
            inner
                .events
                .dispatch(WindowEvent::AdviseDeadKeyStatus(DeadKeyStatus::Composing(
                    composing,
                )));
        }
        // We will show the composing string ourselves.
        // Suppress the default composition display.
        return Some(1);
    }

    match imc.get_str(GCS_RESULTSTR) {
        Ok(s) if !s.is_empty() => {
            let key = KeyEvent {
                key: KeyCode::Composed(s),
                modifiers: Modifiers::NONE,
                leds: KeyboardLedStatus::empty(),
                repeat_count: 1,
                key_is_down: true,
                raw: None,
                win32_uni_char: None,
            };
            inner
                .events
                .dispatch(WindowEvent::AdviseDeadKeyStatus(DeadKeyStatus::None));
            inner.events.dispatch(WindowEvent::KeyEvent(key));

            return Some(1);
        }
        Ok(_) => {}
        Err(_) => eprintln!("cannot represent IME as unicode string!?"),
    };
    None
}

/// Generate a MSG and call TranslateMessage upon it
/// # Safety
/// `hwnd` must be a valid window handle and the args from a real key message.
unsafe fn translate_message(hwnd: HWND, msg: UINT, wparam: WPARAM, lparam: LPARAM) {
    TranslateMessage(&MSG {
        hwnd,
        message: msg,
        wParam: wparam,
        lParam: lparam,
        pt: POINT { x: 0, y: 0 },
        time: GetTickCount(),
    });
}

/// # Safety
/// `hwnd` must be a valid window handle and the args from a real key message.
pub(super) unsafe fn key(hwnd: HWND, msg: UINT, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
    let inner = rc_from_hwnd(hwnd)?;
    let mut inner = inner.borrow_mut();
    let repeat = (lparam & 0xffff) as u16;
    let scan_code = ((lparam >> 16) & 0xff) as u8;
    let releasing = (lparam & (1 << 31)) != 0;
    let ime_active = wparam == VK_PROCESSKEY as WPARAM;
    let phys_code = super::super::input::keycodes::vkey_to_phys(wparam);

    let alt_pressed = (lparam & (1 << 29)) != 0;
    let is_extended = (lparam & (1 << 24)) != 0;
    let was_down = (lparam & (1 << 30)) != 0;
    let label = match msg {
        WM_CHAR => "WM_CHAR",
        WM_IME_CHAR => "WM_IME_CHAR",
        WM_KEYDOWN => "WM_KEYDOWN",
        WM_KEYUP => "WM_KEYUP",
        WM_SYSKEYUP => "WM_SYSKEYUP",
        WM_SYSKEYDOWN => "WM_SYSKEYDOWN",
        WM_SYSCHAR => "WM_SYSCHAR",
        WM_DEADCHAR => "WM_DEADCHAR",
        _ => "WAT",
    };
    log::trace!(
        "{} c=`{}` repeat={} scan={} is_extended={} alt_pressed={} was_down={} \
             releasing={} IME={} dead_pending={:?}",
        label,
        wparam,
        repeat,
        scan_code,
        is_extended,
        alt_pressed,
        was_down,
        releasing,
        ime_active,
        inner.dead_pending,
    );

    if ime_active {
        // If the IME is active, allow Windows to perform default processing
        // to drive it forwards.  It will generate a call to `ime_composition`
        // or `ime_endcomposition` when it completes.

        if msg == WM_KEYDOWN {
            // Release the borrow before calling translate_message:
            // TranslateMessage can trigger other window messages (like WM_SIZE)
            // via CtfImeCreateInputContext, which would otherwise cause a
            // RefCell borrow conflict while inner is still borrowed.
            drop(inner);
            // Explicitly allow the built-in translation to occur for the IME
            translate_message(hwnd, msg, wparam, lparam);
            return Some(0);
        }

        return None;
    }

    if msg == WM_DEADCHAR {
        // Ignore WM_DEADCHAR; we only care about the resultant WM_CHAR
        return Some(0);
    }

    let keys = {
        let mut keys = [0u8; 256];
        GetKeyboardState(keys.as_mut_ptr());
        keys
    };

    let mut modifiers = Modifiers::default();
    if keys[VK_SHIFT as usize] & 0x80 != 0 {
        modifiers |= Modifiers::SHIFT;
    }
    if keys[VK_LSHIFT as usize] & 0x80 != 0 {
        modifiers |= Modifiers::LEFT_SHIFT;
    }
    if keys[VK_RSHIFT as usize] & 0x80 != 0 {
        modifiers |= Modifiers::RIGHT_SHIFT;
    }
    if keys[VK_LCONTROL as usize] & 0x80 != 0 {
        modifiers |= Modifiers::LEFT_CTRL;
    }
    if keys[VK_RCONTROL as usize] & 0x80 != 0 {
        modifiers |= Modifiers::RIGHT_CTRL;
    }
    modifiers.set(Modifiers::ENHANCED_KEY, is_extended);

    if inner.keyboard_info.has_alt_gr()
        && (keys[VK_RMENU as usize] & 0x80 != 0)
        && (keys[VK_CONTROL as usize] & 0x80 != 0)
    {
        // AltGr is pressed; while AltGr is on the RHS of the keyboard
        // is not the same thing as right-alt.
        // Windows sets RMENU and CONTROL to indicate AltGr and we
        // have to keep these in the key state in order for ToUnicode
        // to map the key correctly.
        // We set RIGHT_ALT as a hint to ourselves that AltGr is in
        // use (we use regular ALT otherwise) so that our dead key
        // resolution can do the right thing.
        modifiers |= Modifiers::RIGHT_ALT;
    } else if inner.keyboard_info.has_alt_gr()
        && inner.config.treat_left_ctrlalt_as_altgr
        && (keys[VK_MENU as usize] & 0x80 != 0)
        && (keys[VK_CONTROL as usize] & 0x80 != 0)
    {
        // When running inside a VNC session, VNC emulates the AltGr keypresses
        // by sending plain VK_MENU (rather than VK_RMENU) + VK_CONTROL.
        // For compatibility with that the option `treat_left_ctrlalt_as_altgr` allows
        // to treat MENU+CONTROL as equivalent to RMENU+CONTROL (AltGr) even though it is
        // technically a lossy transformation.
        //
        // We only do that when the keyboard layout has AltGr and the option is enabled,
        // so that we don't screw things up by default or for other keyboard layouts.
        // See issue #392 & #472 for some more context.
        modifiers |= Modifiers::RIGHT_ALT;
    } else {
        if keys[VK_CONTROL as usize] & 0x80 != 0 {
            modifiers |= Modifiers::CTRL;
        }
        if keys[VK_MENU as usize] & 0x80 != 0 {
            modifiers |= Modifiers::ALT;
        }
    }
    if keys[VK_LWIN as usize] & 0x80 != 0 || keys[VK_RWIN as usize] & 0x80 != 0 {
        modifiers |= Modifiers::SUPER;
    }

    let mut leds = KeyboardLedStatus::empty();
    if keys[VK_CAPITAL as usize] & 1 != 0 {
        leds |= KeyboardLedStatus::CAPS_LOCK;
    }
    if keys[VK_NUMLOCK as usize] & 1 != 0 {
        leds |= KeyboardLedStatus::NUM_LOCK;
    }

    let handled_raw = Handled::new();
    let raw_key_event = RawKeyEvent {
        key: match phys_code {
            Some(phys) => KeyCode::Physical(phys),
            None => KeyCode::RawCode(wparam as _),
        },
        phys_code,
        raw_code: wparam as _,
        scan_code: scan_code as _,
        leds,
        modifiers,
        repeat_count: 1,
        key_is_down: !releasing,
        handled: handled_raw.clone(),
    };

    let (key, win32_uni_char) = if msg == WM_IME_CHAR || msg == WM_CHAR {
        // If we were sent a character by the IME, some other apps,
        // or by ourselves via TranslateMessage, then take that
        // value as-is.
        (
            Some(KeyCode::Char(std::char::from_u32_unchecked(wparam as u32))),
            None,
        )
    } else {
        // Otherwise we're dealing with a raw key message.
        // ToUnicode has frustrating statefulness so we take care to
        // call it only when we think it will give consistent results.

        inner
            .events
            .dispatch(WindowEvent::RawKeyEvent(raw_key_event.clone()));
        if handled_raw.is_handled() {
            // Cancel any pending dead key
            if inner.dead_pending.take().is_some() {
                inner
                    .events
                    .dispatch(WindowEvent::AdviseDeadKeyStatus(DeadKeyStatus::None));
            }
            log::trace!("raw key was handled; not processing further");
            return Some(0);
        }

        let is_modifier_only = phys_code.map(|p| p.is_modifier()).unwrap_or(false);
        if is_modifier_only {
            // If this event is only modifiers then don't ask the system
            // for further resolution, as we don't want ToUnicode to
            // perturb its inscrutable global state.
            // Modifier-only keypresses are reported as NUL when using win32 input mode.
            (phys_code.map(|p| p.to_key_code()), Some('\x00'))
        } else {
            // If we think this might be a dead key, process it for ourselves.
            // Our KeyboardLayoutInfo struct probed the layout for the key
            // combinations that start a dead key sequence, as well as those
            // that are valid end states for dead keys, so we can resolve
            // these for ourselves in a couple of quick hash lookups.
            let vk = wparam as u32;

            if releasing && inner.dead_pending.is_some() {
                // Don't care about key-up events while processing dead keys
                return Some(0);
            }

            // If we previously had the start of a dead key...
            let dead = if let Some(leader) = inner.dead_pending.take() {
                inner
                    .events
                    .dispatch(WindowEvent::AdviseDeadKeyStatus(DeadKeyStatus::None));
                // look to see how the current event resolves it
                match inner
                    .keyboard_info
                    .resolve_dead_key(leader, (modifiers, vk))
                {
                    // Valid combination produces a single character
                    ResolvedDeadKey::Combined(c) => Some(KeyCode::Char(c)),
                    ResolvedDeadKey::InvalidCombination(c) => {
                        // An invalid combination results in the deferred
                        // keypress triggering the original key first,
                        // and then we process the current key.

                        // Emit an event for the leader of the failed
                        // dead key combination
                        let key = KeyEvent {
                            key: KeyCode::Char(c),
                            modifiers,
                            leds,
                            repeat_count: 1,
                            key_is_down: !releasing,
                            win32_uni_char: Some(c),
                            raw: Some(RawKeyEvent {
                                scan_code: 0,
                                ..raw_key_event.clone()
                            }),
                        }
                        .normalize_shift()
                        .resurface_positional_modifier_key()
                        .normalize_ctrl();

                        inner.events.dispatch(WindowEvent::KeyEvent(key.clone()));

                        // And then we'll perform normal processing on the
                        // current key press
                        if let Some(new_dead_char) =
                            inner.keyboard_info.is_dead_key_leader(modifiers, vk)
                        {
                            if new_dead_char != c {
                                // Happens to be the start of its own new,
                                // different, dead key sequence
                                inner.dead_pending.replace((modifiers, vk));
                                return Some(0);
                            }

                            // They pressed the same dead key twice,
                            // emit the underlying char again and call
                            // it done.
                            // <https://github.com/wezterm/wezterm/issues/1729>
                            inner.events.dispatch(WindowEvent::KeyEvent(key.clone()));
                            return Some(0);
                        }

                        // We don't know; allow normal ToUnicode processing
                        None
                    }

                    // We thought we had a dead key last time around,
                    // but this time it didn't resolve.  Most likely
                    // because the keyboard layout changed in the middle
                    // of the keypress.
                    // We're effectively swallowing the original dead
                    // key event here, but we could potentially re-process
                    // the original and current one here if needed.
                    // Seems like a real edge case.
                    ResolvedDeadKey::InvalidDeadKey => None,
                }
            } else if let Some(c) = inner.keyboard_info.is_dead_key_leader(modifiers, vk) {
                if releasing {
                    // Don't care about key-up events while processing dead keys
                    return Some(0);
                }

                // They pressed a dead key.
                // If they want dead key processing, then record that and
                // wait for a subsequent keypress.
                if inner.config.use_dead_keys {
                    inner.dead_pending.replace((modifiers, vk));
                    inner.events.dispatch(WindowEvent::AdviseDeadKeyStatus(
                        DeadKeyStatus::Composing(c.to_string()),
                    ));
                    return Some(0);
                }
                // They don't want dead keys; just return the base character
                Some(KeyCode::Char(c))
            } else {
                // Not a dead key as far as we know
                None
            };

            if dead.is_some() {
                (dead, None)
            } else {
                // We get here for the various UP (but not DOWN as we shortcircuit
                // those above) messages.
                // We perform conversion to unicode for ourselves,
                // rather than calling TranslateMessage to do it for us,
                // so that we have tighter control over the key processing.
                let mut out = [0u16; 16];

                let win32_uni_char = {
                    let res = ToUnicode(
                        wparam as u32,
                        scan_code as u32,
                        keys.as_ptr(),
                        out.as_mut_ptr(),
                        out.len() as i32,
                        0,
                    );

                    match res {
                        1 => Some(std::char::from_u32_unchecked(out[0] as u32)),
                        0 => Some('\x00'),
                        _ => None,
                    }
                };

                let mut keys = keys;
                // If control is pressed, clear that out and remember it in our
                // own set of modifiers.
                // We used to also remove shift from this set, but it impacts
                // handling of eg: ctrl+shift+' (which is equivalent to ctrl+" in a US English
                // layout.
                // The shift normalization is now handled by the normalize_shift() method.
                if modifiers.contains(Modifiers::CTRL) {
                    keys[VK_CONTROL as usize] = 0;
                    keys[VK_LCONTROL as usize] = 0;
                    keys[VK_RCONTROL as usize] = 0;
                }

                let res = ToUnicode(
                    wparam as u32,
                    scan_code as u32,
                    keys.as_ptr(),
                    out.as_mut_ptr(),
                    out.len() as i32,
                    0,
                );

                // CTRL and (non-AltGr) ALT combinations are keybinding
                // modifiers, not text input: `modifiers` only ever
                // contains CTRL/ALT here when AltGr was ruled out above
                // (AltGr sets RIGHT_ALT instead), so this can't misfire
                // on eg. a German/French AltGr+letter that legitimately
                // produces a different character. `ToUnicode` still
                // layout-translates the physical key even with a
                // modifier held (eg. physical "V" maps to Cyrillic 'м'
                // under a Russian layout), which then fails to match a
                // keybinding registered against the US-QWERTY letter (eg.
                // `ALT|CTRL+V`). Prefer the layout-independent physical
                // key identity for these so bindings resolve the same
                // way regardless of the active keyboard layout, matching
                // how every other modifier-based terminal shortcut is
                // expected to behave.
                let prefer_physical_for_binding = (modifiers.contains(Modifiers::CTRL)
                    || modifiers.contains(Modifiers::ALT))
                    && phys_code.map(|p| p.to_key_code()).is_some();

                let key = if prefer_physical_for_binding {
                    phys_code.map(|p| p.to_key_code())
                } else {
                    match res {
                        1 => Some(KeyCode::Char(std::char::from_u32_unchecked(out[0] as u32))),
                        // No mapping, so use our raw info
                        0 => {
                            log::trace!(
                                "ToUnicode had no mapping for {:?} wparam={}",
                                phys_code,
                                wparam
                            );
                            phys_code.map(|p| p.to_key_code())
                        }
                        _ => {
                            // dead key: if our dead key mapping in KeyboardLayoutInfo was
                            // correct, we shouldn't be able to get here as we should have
                            // landed in the dead key case above.
                            // If somehow we do get here, we don't have a valid mapping
                            // as -1 indicates the start of a dead key sequence,
                            // and any other n > 1 indicates an ambiguous expansion.
                            // Either way, indicate that we don't have a valid result.
                            log::error!(
                                "unexpected dead key expansion: \
                                 modifiers={:?} vk={:?} res={} releasing={} {:?}",
                                modifiers,
                                vk,
                                res,
                                releasing,
                                out
                            );
                            KeyboardLayoutInfo::clear_key_state();
                            None
                        }
                    }
                };

                (key, win32_uni_char)
            }
        }
    };

    if let Some(key) = key {
        // FIXME: verify this behavior: Urgh, special case for ctrl and non-latin layouts.
        // In order to avoid a situation like #678, if CTRL is the only
        // modifier and we've got composed text, then discard the composed
        // text.
        let key = KeyEvent {
            key,
            modifiers,
            leds,
            repeat_count: repeat,
            key_is_down: !releasing,
            win32_uni_char,
            raw: Some(raw_key_event),
        }
        .normalize_shift();

        // Special case for ALT-space to show the system menu, and
        // ALT-F4 to close the window.
        if key.modifiers == Modifiers::ALT
            && (key.key == KeyCode::Char(' ') || key.key == KeyCode::Function(4))
        {
            translate_message(hwnd, msg, wparam, lparam);
            return None;
        }

        log::trace!(
            "diag: win32 key() dispatching msg={} key={:?} modifiers={:?} key_is_down={}",
            label,
            key.key,
            key.modifiers,
            key.key_is_down,
        );

        inner.events.dispatch(WindowEvent::KeyEvent(key));
        return Some(0);
    }
    None
}
