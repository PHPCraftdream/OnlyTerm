use super::*;
use config::MouseEventAltScreen;
use onlyterm_dynamic::{ToDynamic, Value};
use std::collections::BTreeMap;
use window::{PhysKeyCode, UIKeyCapRendering};

impl InputMap {
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
        KeyCode::Physical(phys) => format!("{phys} (Physical)"),
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
                Some(phys) => format!("phys:{phys}"),
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
        KeyCode::Physical(phys) => format!("phys:{phys}"),
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

/// Render a `onlyterm_dynamic::Value` (as produced by `KeyAssignment::to_dynamic`)
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
