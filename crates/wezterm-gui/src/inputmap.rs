use crate::commands::CommandDef;
use config::keyassignment::{
    ClipboardCopyDestination, ClipboardPasteSource, KeyAssignment, KeyTableEntry, KeyTables,
    MouseEventTrigger, SelectionMode,
};
use config::{ConfigHandle, MouseEventAltScreen, MouseEventTriggerMods};
use std::collections::{BTreeMap, HashMap};
use std::time::Duration;
use wezterm_dynamic::{ToDynamic, Value};
use wezterm_term::input::MouseButton;
use window::{KeyCode, Modifiers, PhysKeyCode, UIKeyCapRendering};

pub struct InputMap {
    pub keys: KeyTables,
    pub mouse: HashMap<(MouseEventTrigger, MouseEventTriggerMods), KeyAssignment>,
    leader: Option<(KeyCode, Modifiers, Duration)>,
}

impl InputMap {
    /// Only the tests below build an input map from the built-in defaults;
    /// every real caller has a `ConfigHandle` already and goes through
    /// `new`.
    #[cfg(test)]
    pub fn default_input_map() -> Self {
        let config = ConfigHandle::default_config();
        Self::new(&config)
    }

    pub fn new(config: &ConfigHandle) -> Self {
        let mut mouse = config.mouse_bindings();

        let mut keys = config.key_bindings();

        let leader = config.leader.as_ref().map(|leader| {
            (
                leader.key.key.resolve(config.key_map_preference).clone(),
                leader.key.mods,
                Duration::from_millis(leader.timeout_milliseconds),
            )
        });

        let ctrl_shift = Modifiers::CTRL | Modifiers::SHIFT;

        macro_rules! m {
            ($([$mod:expr, $code:expr, $action:expr]),* $(,)?) => {
                $(
                mouse.entry(($code, $mod)).or_insert($action);
                )*
            };
        }

        use KeyAssignment::*;

        if !config.disable_default_key_bindings {
            for (mods, code, action) in CommandDef::default_key_assignments(config) {
                // If the user configures {key='p', mods='CTRL|SHIFT'} that gets
                // normalized into {key='P', mods='CTRL'} in Config::key_bindings(),
                // and that value exists in `keys.default` when we reach this point.
                //
                // When we get here with the default assignments for ActivateCommandPalette
                // we are going to register un-normalized entries that don't match
                // the existing normalized entry.
                //
                // Ideally we'd unconditionally normalize_shift
                // here and register the result if it isn't already in the map.
                //
                // Our default set of assignments deliberately and explicitly emits
                // variations on SHIFT as a workaround for an issue with
                // normalization under X11: <https://github.com/wezterm/wezterm/issues/1906>.
                // Until that is resolved, we need to keep emitting both variants.
                //
                // In order for the DisableDefaultAssignment behavior to work with the
                // least surprises, and for these normalization related workarounds
                // to continue? to work, the approach we take here is to lookup the
                // normalized version of what we're about to register, and if we get
                // a match, skip this key.  Otherwise register the non-normalized
                // version from default_key_assignments().
                //
                // See: <https://github.com/wezterm/wezterm/issues/3262>
                let (disable_code, disable_mods) = code.normalize_shift(mods);
                if keys
                    .default
                    .contains_key(&(disable_code.clone(), disable_mods))
                {
                    continue;
                }
                keys.default
                    .entry((code, mods))
                    .or_insert(KeyTableEntry { action });
            }
        }

        if !config.disable_default_mouse_bindings {
            m!(
                [
                    MouseEventTriggerMods {
                        mods: Modifiers::NONE,
                        mouse_reporting: false,
                        alt_screen: MouseEventAltScreen::False,
                    },
                    MouseEventTrigger::Down {
                        streak: 1,
                        button: MouseButton::WheelUp(1),
                    },
                    ScrollByCurrentEventWheelDelta
                ],
                [
                    MouseEventTriggerMods {
                        mods: Modifiers::NONE,
                        mouse_reporting: false,
                        alt_screen: MouseEventAltScreen::False,
                    },
                    MouseEventTrigger::Down {
                        streak: 1,
                        button: MouseButton::WheelDown(1),
                    },
                    ScrollByCurrentEventWheelDelta
                ],
                [
                    MouseEventTriggerMods {
                        mods: Modifiers::NONE,
                        mouse_reporting: false,
                        alt_screen: MouseEventAltScreen::Any,
                    },
                    MouseEventTrigger::Down {
                        streak: 3,
                        button: MouseButton::Left
                    },
                    SelectTextAtMouseCursor(SelectionMode::Line)
                ],
                [
                    MouseEventTriggerMods {
                        mods: Modifiers::NONE,
                        mouse_reporting: false,
                        alt_screen: MouseEventAltScreen::Any,
                    },
                    MouseEventTrigger::Down {
                        streak: 2,
                        button: MouseButton::Left
                    },
                    SelectTextAtMouseCursor(SelectionMode::Word)
                ],
                [
                    MouseEventTriggerMods {
                        mods: Modifiers::NONE,
                        mouse_reporting: false,
                        alt_screen: MouseEventAltScreen::Any,
                    },
                    MouseEventTrigger::Down {
                        streak: 1,
                        button: MouseButton::Left
                    },
                    SelectTextAtMouseCursor(SelectionMode::Cell)
                ],
                [
                    MouseEventTriggerMods {
                        mods: Modifiers::ALT,
                        mouse_reporting: false,
                        alt_screen: MouseEventAltScreen::Any,
                    },
                    MouseEventTrigger::Down {
                        streak: 1,
                        button: MouseButton::Left
                    },
                    SelectTextAtMouseCursor(SelectionMode::Block)
                ],
                [
                    MouseEventTriggerMods {
                        mods: Modifiers::SHIFT,
                        mouse_reporting: false,
                        alt_screen: MouseEventAltScreen::Any,
                    },
                    MouseEventTrigger::Down {
                        streak: 1,
                        button: MouseButton::Left
                    },
                    ExtendSelectionToMouseCursor(SelectionMode::Cell)
                ],
                [
                    MouseEventTriggerMods {
                        mods: Modifiers::SHIFT,
                        mouse_reporting: false,
                        alt_screen: MouseEventAltScreen::Any,
                    },
                    MouseEventTrigger::Up {
                        streak: 1,
                        button: MouseButton::Left
                    },
                    CompleteSelectionOrOpenLinkAtMouseCursor(
                        ClipboardCopyDestination::ClipboardAndPrimarySelection
                    )
                ],
                [
                    MouseEventTriggerMods {
                        mods: Modifiers::NONE,
                        mouse_reporting: false,
                        alt_screen: MouseEventAltScreen::Any,
                    },
                    MouseEventTrigger::Up {
                        streak: 1,
                        button: MouseButton::Left
                    },
                    CompleteSelectionOrOpenLinkAtMouseCursor(
                        ClipboardCopyDestination::ClipboardAndPrimarySelection
                    )
                ],
                [
                    MouseEventTriggerMods {
                        mods: Modifiers::ALT,
                        mouse_reporting: false,
                        alt_screen: MouseEventAltScreen::Any,
                    },
                    MouseEventTrigger::Up {
                        streak: 1,
                        button: MouseButton::Left
                    },
                    CompleteSelection(ClipboardCopyDestination::ClipboardAndPrimarySelection)
                ],
                [
                    MouseEventTriggerMods {
                        mods: Modifiers::NONE,
                        mouse_reporting: false,
                        alt_screen: MouseEventAltScreen::Any,
                    },
                    MouseEventTrigger::Up {
                        streak: 1,
                        button: MouseButton::Right
                    },
                    CopyLinkAtMouseCursor(ClipboardCopyDestination::ClipboardAndPrimarySelection)
                ],
                [
                    MouseEventTriggerMods {
                        mods: Modifiers::ALT | Modifiers::SHIFT,
                        mouse_reporting: false,
                        alt_screen: MouseEventAltScreen::Any,
                    },
                    MouseEventTrigger::Down {
                        streak: 1,
                        button: MouseButton::Left
                    },
                    ExtendSelectionToMouseCursor(SelectionMode::Block)
                ],
                [
                    MouseEventTriggerMods {
                        mods: Modifiers::ALT | Modifiers::SHIFT,
                        mouse_reporting: false,
                        alt_screen: MouseEventAltScreen::Any,
                    },
                    MouseEventTrigger::Up {
                        streak: 1,
                        button: MouseButton::Left
                    },
                    CompleteSelectionOrOpenLinkAtMouseCursor(
                        ClipboardCopyDestination::PrimarySelection
                    )
                ],
                [
                    MouseEventTriggerMods {
                        mods: Modifiers::NONE,
                        mouse_reporting: false,
                        alt_screen: MouseEventAltScreen::Any,
                    },
                    MouseEventTrigger::Up {
                        streak: 2,
                        button: MouseButton::Left
                    },
                    CompleteSelection(ClipboardCopyDestination::ClipboardAndPrimarySelection)
                ],
                [
                    MouseEventTriggerMods {
                        mods: Modifiers::NONE,
                        mouse_reporting: false,
                        alt_screen: MouseEventAltScreen::Any,
                    },
                    MouseEventTrigger::Up {
                        streak: 3,
                        button: MouseButton::Left
                    },
                    CompleteSelection(ClipboardCopyDestination::ClipboardAndPrimarySelection)
                ],
                [
                    MouseEventTriggerMods {
                        mods: Modifiers::NONE,
                        mouse_reporting: false,
                        alt_screen: MouseEventAltScreen::Any,
                    },
                    MouseEventTrigger::Drag {
                        streak: 1,
                        button: MouseButton::Left
                    },
                    ExtendSelectionToMouseCursor(SelectionMode::Cell)
                ],
                [
                    MouseEventTriggerMods {
                        mods: Modifiers::ALT,
                        mouse_reporting: false,
                        alt_screen: MouseEventAltScreen::Any,
                    },
                    MouseEventTrigger::Drag {
                        streak: 1,
                        button: MouseButton::Left
                    },
                    ExtendSelectionToMouseCursor(SelectionMode::Block)
                ],
                [
                    MouseEventTriggerMods {
                        mods: Modifiers::NONE,
                        mouse_reporting: false,
                        alt_screen: MouseEventAltScreen::Any,
                    },
                    MouseEventTrigger::Drag {
                        streak: 2,
                        button: MouseButton::Left
                    },
                    ExtendSelectionToMouseCursor(SelectionMode::Word)
                ],
                [
                    MouseEventTriggerMods {
                        mods: Modifiers::NONE,
                        mouse_reporting: false,
                        alt_screen: MouseEventAltScreen::Any,
                    },
                    MouseEventTrigger::Drag {
                        streak: 3,
                        button: MouseButton::Left
                    },
                    ExtendSelectionToMouseCursor(SelectionMode::Line)
                ],
                [
                    MouseEventTriggerMods {
                        mods: Modifiers::NONE,
                        mouse_reporting: false,
                        alt_screen: MouseEventAltScreen::Any,
                    },
                    MouseEventTrigger::Down {
                        streak: 1,
                        button: MouseButton::Middle
                    },
                    PasteFrom(ClipboardPasteSource::PrimarySelection)
                ],
                [
                    MouseEventTriggerMods {
                        mods: Modifiers::SUPER,
                        mouse_reporting: false,
                        alt_screen: MouseEventAltScreen::Any,
                    },
                    MouseEventTrigger::Drag {
                        streak: 1,
                        button: MouseButton::Left,
                    },
                    StartWindowDrag
                ],
                [
                    MouseEventTriggerMods {
                        mods: ctrl_shift,
                        mouse_reporting: false,
                        alt_screen: MouseEventAltScreen::Any,
                    },
                    MouseEventTrigger::Drag {
                        streak: 1,
                        button: MouseButton::Left,
                    },
                    StartWindowDrag
                ],
            );
        }

        keys.default
            .retain(|_, v| v.action != KeyAssignment::DisableDefaultAssignment);

        mouse.retain(|_, v| *v != KeyAssignment::DisableDefaultAssignment);
        // Expand MouseEventAltScreen::Any to individual True/False entries
        let mut expanded_mouse = vec![];
        for ((code, mods), v) in &mouse {
            if mods.alt_screen == MouseEventAltScreen::Any {
                let mods_true = MouseEventTriggerMods {
                    alt_screen: MouseEventAltScreen::True,
                    ..*mods
                };
                let mods_false = MouseEventTriggerMods {
                    alt_screen: MouseEventAltScreen::False,
                    ..*mods
                };
                expanded_mouse.push((code.clone(), mods_true, v.clone()));
                expanded_mouse.push((code.clone(), mods_false, v.clone()));
            }
        }
        // Eliminate ::Any
        mouse.retain(|(_, mods), _| mods.alt_screen != MouseEventAltScreen::Any);
        for (code, mods, v) in expanded_mouse {
            mouse.insert((code, mods), v);
        }

        keys.by_name
            .entry("copy_mode".to_string())
            .or_insert_with(crate::overlay::copy::copy_key_table);
        keys.by_name
            .entry("search_mode".to_string())
            .or_insert_with(crate::overlay::copy::search_key_table);

        Self {
            keys,
            leader,
            mouse,
        }
    }

    /// Given an action, return the corresponding set of application-wide key assignments that are
    /// mapped to it.
    /// If any key_tables reference a given combination, then that combination
    /// is removed from the list.
    /// This is used to figure out whether an application-wide keyboard shortcut
    /// can be safely configured for this action, without interfering with any
    /// transient key_table mappings.
    #[allow(dead_code)]
    pub fn locate_app_wide_key_assignment(
        &self,
        action: &KeyAssignment,
    ) -> Vec<(KeyCode, Modifiers)> {
        let mut candidates = vec![];

        for ((key, mods), entry) in &self.keys.default {
            if mods.contains(Modifiers::LEADER) {
                continue;
            }
            if entry.action == *action {
                candidates.push((key.clone(), mods.clone()));
            }
        }

        // Now ensure that this combination is not part of a key table
        candidates.retain(|tuple| {
            for table in self.keys.by_name.values() {
                if table.contains_key(tuple) {
                    return false;
                }
            }
            true
        });

        candidates
    }

    pub fn is_leader(&self, key: &KeyCode, mods: Modifiers) -> Option<std::time::Duration> {
        if let Some((leader_key, leader_mods, timeout)) = self.leader.as_ref() {
            if *leader_key == *key && *leader_mods == mods.remove_positional_mods() {
                return Some(timeout.clone());
            }
        }
        None
    }

    pub fn has_table(&self, name: &str) -> bool {
        self.keys.by_name.contains_key(name)
    }

    pub fn lookup_key(
        &self,
        key: &KeyCode,
        mods: Modifiers,
        table_name: Option<&str>,
    ) -> Option<KeyTableEntry> {
        let table = match table_name {
            Some(name) => self.keys.by_name.get(name)?,
            None => &self.keys.default,
        };

        table
            .get(&key.normalize_shift(mods.remove_positional_mods()))
            .cloned()
    }

    pub fn lookup_mouse(
        &self,
        event: MouseEventTrigger,
        mut mods: MouseEventTriggerMods,
    ) -> Option<KeyAssignment> {
        mods.mods = mods.mods.remove_positional_mods();
        self.mouse.get(&(event, mods)).cloned()
    }

    pub fn dump_config(&self, key_table: Option<&str>) {
        if key_table.is_none() {
            println!("keys: [");
            show_key_table_as_ktav(&self.keys.default, 4);
            println!("]");
            println!();
        }

        let mut table_names = self.keys.by_name.keys().collect::<Vec<_>>();
        table_names.sort();
        println!("key_tables: {{");
        for name in table_names {
            if let Some(wanted_table) = key_table {
                if name != wanted_table {
                    continue;
                }
            }
            if let Some(table) = self.keys.by_name.get(name) {
                println!("    {name}: [");
                show_key_table_as_ktav(table, 6);
                println!("    ]");
                println!();
            }
        }
        println!("}}");
    }

    pub fn show_keys(&self) {
        if let Some((key, mods, duration)) = &self.leader {
            println!("Leader: {key:?} {mods:?} {duration:?}");
        }

        section_header("Default key table");
        show_key_table(&self.keys.default);
        println!();

        let mut table_names = self.keys.by_name.keys().collect::<Vec<_>>();
        table_names.sort();
        for name in table_names {
            if let Some(table) = self.keys.by_name.get(name) {
                section_header(&format!("Key Table: {name}"));
                show_key_table(table);
                println!();
            }
        }

        self.show_mouse();
    }

    fn show_mouse(&self) {
        for (label, alt_screen, mouse_reporting) in [
            ("Mouse", MouseEventAltScreen::False, false),
            ("Mouse: alt_screen", MouseEventAltScreen::True, false),
            ("Mouse: mouse_reporting", MouseEventAltScreen::False, true),
            (
                "Mouse: mouse_reporting + alt_screen",
                MouseEventAltScreen::True,
                true,
            ),
        ] {
            let ordered = self
                .mouse
                .iter()
                .filter(|((_, m), _)| {
                    m.alt_screen == alt_screen && m.mouse_reporting == mouse_reporting
                })
                .collect::<BTreeMap<_, _>>();

            if ordered.is_empty() {
                continue;
            }

            section_header(label);

            let mut trigger_width = 0;
            let mut mod_width = 0;
            for (trigger, mods) in ordered.keys() {
                mod_width = mod_width.max(format!("{:?}", mods.mods).len());
                trigger_width = trigger_width.max(format!("{trigger:?}").len());
            }

            for ((trigger, mods), action) in ordered {
                let mods = if mods.mods == Modifiers::NONE {
                    String::new()
                } else {
                    format!("{:?}", mods.mods)
                };
                let trigger = format!("{trigger:?}");
                println!("\t{mods:mod_width$}   {trigger:trigger_width$}   ->   {action:?}");
            }

            println!();
        }
    }
}

fn section_header(title: &str) {
    let dash = "-".repeat(title.len());
    println!("{title}");
    println!("{dash}");
    println!();
}

pub fn ui_key(key: &KeyCode, ui_key_cap_rendering: UIKeyCapRendering) -> String {
    match key {
        KeyCode::Char('\x1b') | KeyCode::Char('\x7f')
            if ui_key_cap_rendering == UIKeyCapRendering::AppleSymbols =>
        {
            "\u{238b}".to_string()
        }
        KeyCode::Char('\x1b') | KeyCode::Char('\x7f') => "Esc".to_string(),
        KeyCode::Char('\x08') if ui_key_cap_rendering == UIKeyCapRendering::AppleSymbols => {
            "\u{232b}".to_string()
        }
        KeyCode::Char('\x08') => "Del".to_string(),
        KeyCode::Char('\r') if ui_key_cap_rendering == UIKeyCapRendering::AppleSymbols => {
            "\u{21b5}".to_string()
        }
        KeyCode::Char('\r') => "Enter".to_string(),
        KeyCode::Physical(PhysKeyCode::Space) | KeyCode::Char(' ')
            if ui_key_cap_rendering == UIKeyCapRendering::AppleSymbols =>
        {
            "\u{2423}".to_string()
        }
        KeyCode::Char(' ') => "Space".to_string(),
        KeyCode::Char('\t') if ui_key_cap_rendering == UIKeyCapRendering::AppleSymbols => {
            "\u{21e5}".to_string()
        }
        KeyCode::Char('\t') => "Tab".to_string(),
        KeyCode::Char(c) if c.is_ascii_control() => c.escape_debug().to_string(),
        KeyCode::Char(c) => c.to_uppercase().to_string(),

        KeyCode::Physical(PhysKeyCode::PageUp) | KeyCode::PageUp
            if ui_key_cap_rendering == UIKeyCapRendering::AppleSymbols =>
        {
            "\u{21de}".to_string()
        }
        KeyCode::Physical(PhysKeyCode::PageDown) | KeyCode::PageDown
            if ui_key_cap_rendering == UIKeyCapRendering::AppleSymbols =>
        {
            "\u{21df}".to_string()
        }
        KeyCode::Physical(PhysKeyCode::LeftArrow) | KeyCode::LeftArrow => "\u{2190}".to_string(),
        KeyCode::Physical(PhysKeyCode::UpArrow) | KeyCode::UpArrow => "\u{2191}".to_string(),
        KeyCode::Physical(PhysKeyCode::RightArrow) | KeyCode::RightArrow => "\u{2192}".to_string(),
        KeyCode::Physical(PhysKeyCode::DownArrow) | KeyCode::DownArrow => "\u{2193}".to_string(),
        KeyCode::Function(n) => format!("F{n}"),
        KeyCode::Numpad(n) => format!("Numpad{n}"),
        KeyCode::Physical(phys) => phys.to_string(),
        _ => format!("{key:?}"),
    }
}

pub fn human_key(key: &KeyCode) -> String {
    match key {
        KeyCode::Char('\x1b') => "Escape".to_string(),
        KeyCode::Char('\x7f') => "Escape".to_string(),
        KeyCode::Char('\x08') => "Backspace".to_string(),
        KeyCode::Char('\r') => "Enter".to_string(),
        KeyCode::Char(' ') => "Space".to_string(),
        KeyCode::Char('\t') => "Tab".to_string(),
        KeyCode::Char(c) if c.is_ascii_control() => c.escape_debug().to_string(),
        KeyCode::Char(c) => c.to_string(),
        KeyCode::Function(n) => format!("F{n}"),
        KeyCode::Numpad(n) => format!("Numpad{n}"),
        KeyCode::Physical(phys) => format!("{} (Physical)", phys.to_string()),
        _ => format!("{key:?}"),
    }
}

/// Render a `KeyCode` as the ktav string value for a `key:` field.
///
/// ktav bareword values are written unquoted, so a value that would
/// otherwise look like a bare number (a single digit, e.g. the `1` key)
/// must be qualified with the `phys:` prefix to force it to parse as the
/// physical-position key string rather than a numeric literal. See
/// `docs/config/keys.md` for the `phys:`/`mapped:`/`raw:` prefixes.
fn ktav_key_code(key: &KeyCode) -> String {
    match key {
        KeyCode::Char('\x1b') => "Escape".to_string(),
        KeyCode::Char('\x7f') => "Escape".to_string(),
        KeyCode::Char('\x08') => "Backspace".to_string(),
        KeyCode::Char('\r') => "Enter".to_string(),
        KeyCode::Char(' ') => "Space".to_string(),
        KeyCode::Char('\t') => "Tab".to_string(),
        KeyCode::Char(c) if c.is_ascii_control() => c.escape_debug().to_string(),
        KeyCode::Char(c) if c.is_ascii_digit() => {
            // A bare digit would parse as a ktav number, not a string, and
            // fail to load as a `key:` value -- force it to the `phys:`
            // physical-position form, which is unambiguously a string.
            // Fall back to `mapped:` in the (currently unreachable for
            // ASCII digits) case there's no physical-position equivalent.
            match key.to_phys() {
                Some(phys) => format!("phys:{}", phys.to_string()),
                None => format!("mapped:{c}"),
            }
        }
        // Characters that carry structural meaning inside a ktav inline
        // compound have to be escaped or the `key:` value swallows the rest
        // of the line: `key: [` opens an array, `key: {` an object, `key: }`
        // closes the enclosing one early, and `key: ,` reads as an empty
        // pair segment. A backslash is ktav's escape character, so it needs
        // escaping too. This isn't cosmetic -- a single unescaped bracket
        // makes the whole emitted document fail to parse, which defeats the
        // point of a dump that's meant to be pasted straight into a config.
        KeyCode::Char(c @ ('[' | ']' | '{' | '}' | ',' | '\\')) => format!("\\{c}"),
        KeyCode::Char(c) => c.to_string(),
        KeyCode::Function(n) => format!("F{n}"),
        KeyCode::Numpad(n) => format!("Numpad{n}"),
        KeyCode::Physical(phys) => format!("phys:{}", phys.to_string()),
        _ => format!("{key:?}"),
    }
}

/// Re-render a `DeferredKeyCode`'s dynamic string form (as produced by
/// `KeyCode::to_string()`: `"mapped:<char>"`, `"phys:<name>"`, `"raw:<n>"`,
/// or a bare key name) as a safe ktav `key:` value, going through the same
/// control-character-aware formatting as the outer key of a binding. This
/// matters because `KeyCode::to_string()` spells e.g. Enter as the literal
/// three-byte string `"mapped:\r"` -- a raw carriage return with no ktav
/// escape available -- which would otherwise corrupt the emitted document.
fn rewrite_dynamic_key_string(s: &str) -> String {
    let key = if let Some(c) = s.strip_prefix("mapped:").and_then(|rest| {
        let mut chars = rest.chars();
        let c = chars.next()?;
        if chars.next().is_none() {
            Some(c)
        } else {
            None
        }
    }) {
        KeyCode::Char(c)
    } else if let Some(phys) = s
        .strip_prefix("phys:")
        .and_then(|rest| <PhysKeyCode as std::convert::TryFrom<&str>>::try_from(rest).ok())
    {
        KeyCode::Physical(phys)
    } else {
        // Bare key name (e.g. "Enter", "F1") or a form we don't specially
        // recognize here -- ktav_key_code re-derives the same spelling for
        // named keys, and anything else round-trips through unchanged.
        return escape_ktav_bareword(s);
    };
    ktav_key_code(&key)
}

/// Render a `wezterm_dynamic::Value` (as produced by `KeyAssignment::to_dynamic`)
/// as a ktav literal. `is_top` is set for the outermost `action` value: a
/// simple (argument-less) action is just its bare name (`Copy`), while a
/// parameterized action is a single-key object whose key is the action name
/// (`{ SpawnCommandInNewTab: { cwd: /tmp } }`), matching the shape documented
/// in `docs/migration-to-ktav.md#key-bindings-and-actions`.
fn ktavify(value: Value, is_top: bool) -> String {
    match value {
        Value::String(s) if is_top => s,
        // ktav has no quoting syntax: bareword strings are written as-is.
        // A literal backslash starts an escape sequence in ktav, so any
        // path-like value must use forward slashes to round-trip safely.
        Value::String(s) => escape_ktav_bareword(&s),
        Value::Bool(true) => "true".to_string(),
        Value::Bool(false) => "false".to_string(),
        Value::Null => "null".to_string(),
        Value::U64(u) => u.to_string(),
        Value::F64(u) => u.to_string(),
        Value::I64(u) => u.to_string(),
        Value::Array(a) => {
            let items: Vec<String> = a.into_iter().map(|v| ktavify(v, false)).collect();
            format!("[{}]", items.join(", "))
        }
        Value::Object(o) if is_top => {
            let (k, v) = o.into_iter().next().unwrap();
            let k = match k {
                Value::String(s) => s,
                _ => unreachable!(),
            };
            format!("{{ {k}: {} }}", ktavify(v, false))
        }
        Value::Object(o) => {
            let mut fields = vec![];
            for (k, v) in o {
                let k = match k {
                    Value::String(s) => s,
                    _ => unreachable!(),
                };
                match v {
                    Value::Null => continue,
                    Value::Object(o) if o.is_empty() => continue,
                    // A nested `key: <DeferredKeyCode>` field (e.g. inside
                    // `SendKey`/`ActivateKeyTable`) is serialized via
                    // `KeyCode::to_string()`, which spells a control
                    // character key (Enter, Escape, ...) as e.g.
                    // `"mapped:\r"` -- a raw control byte with no ktav
                    // escape available. Re-render it the same safe way as
                    // the outer `key:` field instead of passing the raw
                    // string through.
                    Value::String(s) if k == "key" => {
                        fields.push(format!("{k}: {}", rewrite_dynamic_key_string(&s)))
                    }
                    _ => fields.push(format!("{k}: {}", ktavify(v, false))),
                }
            }
            format!("{{ {} }}", fields.join(", "))
        }
    }
}

/// Escape a string for use as a ktav bareword value: forward-slash any
/// backslashes (ktav treats `\` as the start of an escape sequence, so a
/// literal Windows path would otherwise silently corrupt or fail to parse).
fn escape_ktav_bareword(s: &str) -> String {
    s.replace('\\', "/")
}

fn ktav_key(key: &KeyCode, mods: Modifiers, action: &KeyAssignment) -> String {
    let dyn_action = action.to_dynamic();
    let action = ktavify(dyn_action, true);
    let key = ktav_key_code(key);

    let mods = format!("{mods:?}").replace(" ", "");

    format!("{{ key: {key}, mods: {mods}, action: {action} }}")
}

fn show_key_table(table: &config::keyassignment::KeyTable) {
    let ordered = table.iter().collect::<BTreeMap<_, _>>();

    let mut key_width = 0;
    let mut mod_width = 0;
    for (key, mods) in ordered.keys() {
        mod_width = mod_width.max(format!("{mods:?}").len());
        key_width = key_width.max(human_key(key).len());
    }

    for ((key, mods), entry) in ordered {
        let action = &entry.action;
        let mods = if *mods == Modifiers::NONE {
            String::new()
        } else {
            format!("{mods:?}")
        };
        let key = human_key(key);
        println!("\t{mods:mod_width$}   {key:key_width$}   ->   {action:?}");
    }
}

fn show_key_table_as_ktav(table: &config::keyassignment::KeyTable, indent: usize) {
    let ordered = table.iter().collect::<BTreeMap<_, _>>();

    let pad = " ".repeat(indent);
    for ((key, mods), entry) in ordered {
        let action = &entry.action;
        println!("{pad}{}", ktav_key(key, *mods, action));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use config::keyassignment::ClipboardCopyDestination;

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
    #[test]
    fn ctrl_enter_sends_newline() {
        let input_map = InputMap::default_input_map();

        let entry = input_map
            .lookup_key(&KeyCode::Char('\r'), Modifiers::CTRL, None)
            .expect("CTRL+Enter must have a default binding");
        assert_eq!(
            entry.action,
            KeyAssignment::SendEnterOrNewline(Modifiers::CTRL)
        );
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

    /// Regression test: CTRL+J must NOT be captured by any default key
    /// binding (eg: tab/pane navigation or anything else). Since it is
    /// free, the raw key event falls through to the terminal's standard
    /// ASCII control-code encoding, where CTRL+J naturally encodes to
    /// 0x0A (line feed) via `ctrl_mapping('j')`. That is exactly the
    /// desired behavior, so no new default binding is required for it --
    /// only this assertion that nothing has claimed the chord.
    #[test]
    fn ctrl_j_has_no_default_binding_and_passes_through_as_raw_byte() {
        let input_map = InputMap::default_input_map();

        assert!(
            input_map
                .lookup_key(&KeyCode::Char('j'), Modifiers::CTRL, None)
                .is_none(),
            "CTRL+J must be free of any default KeyAssignment so that it \
             passes through to the pty as a raw 0x0A byte"
        );
        assert!(
            input_map
                .lookup_key(&KeyCode::Char('J'), Modifiers::CTRL, None)
                .is_none(),
            "CTRL+J (uppercase variant) must also be free of any default KeyAssignment"
        );
    }
}
