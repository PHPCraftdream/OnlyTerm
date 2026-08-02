# Migrating to a ktav config

!!! danger "Breaking change"

    OnlyTerm's configuration language is now **ktav**, a static, engine-free
    `key: value` data format. There is **no scripting engine** anymore: no
    Lua, no [rhai](https://rhai.rs/), no expressions, no function calls, and
    no `on(...)` event hooks. Config loading is now a direct
    `parse -> Config` step with nothing evaluated in between.

    If you have an existing `.wezterm.lua` or `.wezterm.rhai` (renamed to
    `onlyterm.rhai`/`.onlyterm.rhai` in the last release) config, it will
    **no longer load**, and OnlyTerm will print an error naming the exact
    legacy file it found and telling you to migrate it. This guide walks you
    through translating either one to `onlyterm.ktav`.

This was a deliberate simplification, not a stopgap. An audit of the rhai
event-hook/callback machinery found that most of it was already unreachable
dead code — an earlier, incomplete Lua-to-rhai migration had silently
dropped the wiring for most callbacks — so rather than keep carrying a
scripting engine forward mainly to evaluate what amounted to static data,
OnlyTerm now loads config as data, directly. See the
[changelog](changelog.md) for the full rationale and the list of removed
event hooks. A separate, explicit external-hooks API may be added later if
scripted customization turns out to be genuinely needed; this document only
covers today's static config format.

## What ktav is (and isn't)

ktav is a small static data format: it can express everything a JSON or TOML
document can (nested objects, arrays, strings, numbers, booleans, null), with
a terser, quote-optional, comma-optional syntax. It has:

* `key: value` pairs, one per line (or separated by whitespace/newlines).
* `{ ... }` for nested objects.
* `[ ... ]` for arrays.
* Bare, unquoted strings: **write string values without quotes.** ktav does
  not strip quote characters, so `family: "JetBrains Mono"` does not give you
  the string `JetBrains Mono` — it gives you the literal seven-character
  string `"JetBrains Mono"`, quote marks included. Write `family: JetBrains
  Mono` instead. See [Strings](#strings) below for the very small number of
  cases where this actually matters.
* `##` comments: a line whose first non-whitespace characters are `##` is
  ignored. Nothing else is a comment — not `#` alone, and not `//`. A line
  starting with a single `#` or with `//` is parsed as ordinary content
  (usually a bare top-level array item), not skipped, and will either produce
  a confusing error or silently change the shape of your document.

It does **not** have variables, `let`, functions, closures, `if`/`else`
expressions, loops, string concatenation, module imports, or any notion of
calling into the host program (`wezterm.*`, `color::*`, `plugin::*`, and so
on are all gone — there is nothing left to call). A ktav config file is
*read*, not *run*.

A minimal config file, `onlyterm.ktav`:

```
font_size: 14
term: screen-256color
color_scheme: Builtin Solarized Dark
keys: [
    {
        key: t
        mods: CTRL|SHIFT
        action: ToggleFullScreen
    }
]
```

An empty (and rather useless) config file is just an empty document.

## Where the file goes

OnlyTerm looks for `.onlyterm.ktav` in your home directory, or `onlyterm.ktav`
in one of the standard config directories (see
[Configuration Files](config/files.md) for the full search order and how to
override it with `--config-file`/`ONLYTERM_CONFIG_FILE`). This replaces the
previous `.wezterm.lua`/`.wezterm.rhai` (and, briefly, `onlyterm.rhai`)
search.

If OnlyTerm finds a legacy `onlyterm.rhai`, `.onlyterm.rhai`, `onlyterm.lua`
or `.onlyterm.lua` file but no `.ktav` sibling, it stops and prints an error
naming that exact file rather than silently falling back to defaults —
OnlyTerm never attempts to parse the legacy file, it only checks for its
existence so the error can point you at the right one to migrate.

## Syntax translation cheat sheet

### Object maps and records

Both Lua tables-as-records and rhai `#{ ... }` object maps become ktav
`{ ... }` objects. Keys don't need quotes; commas between entries are
optional (a newline is enough).

```lua
-- Lua
config.window_padding = { left = 2, right = 2, top = 0, bottom = 0 }
```

```rhai
// rhai
config.window_padding = #{ left: 2, right: 2, top: 0, bottom: 0 };
```

```
## ktav
window_padding: {
    left: 2
    right: 2
    top: 0
    bottom: 0
}
```

### Arrays / sequences

Lua sequence tables (`{ 1, 2, 3 }`) and rhai arrays (`[1, 2, 3]`) both become
ktav `[ ... ]` arrays. As with objects, commas are optional.

```lua
-- Lua
config.launch_menu = {
  { args = { 'top' } },
  { args = { 'bash' } },
}
```

```rhai
// rhai
config.launch_menu = [
  #{ args: ["top"] },
  #{ args: ["bash"] },
];
```

```
## ktav
launch_menu: [
    { args: [top] }
    { args: [bash] }
]
```

Note that array indices don't matter for ktav (there is no indexing
expression at all — it's just a literal list).

### Strings

Lua/rhai string literals (`"..."` or `'...'`) become ktav strings — but
**drop the quotes**. ktav does not treat `"` as a string delimiter at all: it
has no quoting syntax, so any quote characters you write become part of the
value. `color_scheme: "AdventureTime"` does not parse to the string
`AdventureTime`; it parses to the 15-character string `"AdventureTime"`,
quote marks included. For a `String`-typed field like `font` this fails
*silently* — you just get the wrong value with no error, and OnlyTerm falls
back to whatever default applies. For an enum-typed field (most keyassignment
and enum config options) it fails loudly at config-load time, since the
quoted value doesn't match any known variant name.

```lua
color_scheme = 'AdventureTime'
```

```rhai
color_scheme: "AdventureTime",
```

```
## ktav — write the bareword value with no quotes at all:
color_scheme: AdventureTime
```

This applies to every plain string value: file paths, font family names,
program arguments, and so on. There is no escaping mechanism for a value
that would need to contain a literal `#` or `:` as its very first character
— ktav has no quoting syntax to fall back on for that case, so if you ever
need a string value that is genuinely ambiguous with ktav's own syntax
(this essentially never happens for wezterm config values), there currently
isn't a way to express it. Ordinary values that merely *contain* a `#`,
`:`, or `//` in the middle (a URL like `http://example.com:8080`, a hex
color like `#af8700`) are unaffected; the ambiguity only exists for `##`
right at the very start of a line, and there's no config field where a
literal string value would need to start that way.

### Numbers and booleans

Unchanged in spirit: bare `14`, `10.0`, `true`, `false` all parse the same
way in ktav as they did as Lua/rhai literals. There's no distinction between
an "integer literal" and a "float literal" beyond whether a decimal point is
present, same as before.

### What has no ktav equivalent at all

Because ktav has no expressions or function calls, some things you may have
had in a Lua or rhai config simply **cannot be expressed** anymore. There is
no direct ktav replacement for:

* Any use of `require`/`wezterm.plugin.require`, `config_builder()`, or any
  other function call — write the value directly as it would have been
  *returned*, not the call that produced it. For example,
  `wezterm.font_with_fallback("Operator Mono")` becomes a literal
  `TextStyle` object: `font: { font: [{ family: Operator Mono }] }`.
* `wezterm.on(event, ...)`/`wezterm.emit(...)` and every event hook
  (`format-tab-title`, `format-window-title`, `update-status`,
  `window-config-reloaded`, `bell`, `user-var-changed`, `gui-startup`,
  `gui-attached`, and the rest) — these have been removed with no scripting
  replacement. The corresponding *default* (built-in, non-customized)
  behavior for each is unchanged; see each event's reference page and the
  [changelog](changelog.md) for the specifics of what was removed.
* Any conditional logic (`if`/`else`, ternaries) that picked a config value
  based on the platform, hostname, environment variable, or anything else
  computed at config-load time. A ktav config is a single, static document —
  if you need different configs per machine, maintain separate `.ktav` files
  and point `ONLYTERM_CONFIG_FILE`/`--config-file` at the right one (e.g.
  from a shell profile), or use `--config key=value` overrides on the
  command line for one-off overrides.
* `ExecDomain`'s `fixup`/`label` callbacks — an `ExecDomain` could
  previously wrap/rewrite the command being spawned via a callback; that
  capability is gone along with the rest of the scripting surface.
* Plugins (`plugin::require(path)`) — loading a plugin required evaluating
  its `plugin/init.rhai` (or, before that, `plugin/init.lua`) entry point
  with the scripting engine; with no engine left, plugins as a concept are
  gone. Vendor any plugin functionality you relied on directly into your own
  config.

If your old config relied on any of the above for something load-bearing
(not just cosmetic), there currently isn't a way to reproduce it. An external
hooks API may be added later — see the changelog entry for this migration
for the current thinking.

## Key bindings and actions

A key binding is still a record with `key`, `mods` and `action` fields.
Actions translate the same way objects do in general:

* a **simple** action (no arguments) is just its name as a bare string:
  `Copy`, `Paste`, `DisableDefaultAssignment`, ...
* an action **with arguments** is an object with a single key — the action
  name — whose value carries the arguments.

```lua
-- Lua
config.keys = {
  { key = 'c', mods = 'CTRL|SHIFT', action = wezterm.action.Copy },
  {
    key = 't',
    mods = 'CTRL',
    action = wezterm.action.SpawnCommandInNewTab {
      cwd = '/tmp',
      args = { 'bash' },
    },
  },
}
```

```rhai
// rhai
config.keys = [
    #{ key: "c", mods: "CTRL|SHIFT", action: "Copy" },
    #{
        key: "t",
        mods: "CTRL",
        action: #{ SpawnCommandInNewTab: #{ cwd: "/tmp", args: ["bash"] } },
    },
];
```

```
## ktav
keys: [
    { key: c, mods: CTRL|SHIFT, action: Copy }
    {
        key: t
        mods: CTRL
        action: { SpawnCommandInNewTab: { cwd: /tmp, args: [bash] } }
    }
]
```

The complete list of available actions and their argument shapes is
otherwise unchanged from the rhai era — see the
[key assignments reference](config/reference/keyassignment/index.md).

## A worked example: porting a realistic config

### Before (rhai)

```rhai
config.color_scheme = "Catppuccin Mocha";
config.font = font_with_fallback("JetBrains Mono");
config.font_size = 11.0;
config.window_background_opacity = 0.9;

config.keys = [
    #{ key: "C", mods: "CTRL|SHIFT", action: "Copy" },
    #{
        key: "Enter",
        mods: "SUPER",
        action: #{ SplitHorizontal: #{ domain: "CurrentPaneDomain" } },
    },
];
```

### After (`onlyterm.ktav`)

```
color_scheme: Catppuccin Mocha
font: { font: [{ family: JetBrains Mono }] }
font_size: 11.0
window_background_opacity: 0.9

keys: [
    { key: C, mods: CTRL|SHIFT, action: Copy }
    {
        key: Enter
        mods: SUPER
        action: { SplitHorizontal: { domain: CurrentPaneDomain } }
    }
]
```

Things to notice in the port:

* No file-level wrapper is needed at all — no `#{ ... }` object literal, no
  `return`, nothing to evaluate. The whole file *is* the config object.
* `font_with_fallback(...)`/`wezterm.font(...)` have no ktav equivalent;
  write the value directly as the `TextStyle` object they used to
  construct.
* There's no event hook translation to show here, because event hooks no
  longer exist; a config that relied on one (e.g. a custom
  `format-window-title` handler) has no ktav equivalent for that part —
  only the option/key-binding parts of such a config can be migrated.

## What happens to an old `.wezterm.lua`/`onlyterm.rhai`

When OnlyTerm looks for a config file in one of the
[standard locations](config/files.md) and finds a legacy `.lua`/`.rhai` file
there but **no** `.ktav` sibling, it stops and prints an error naming the
exact legacy file, e.g.:

```
Found a legacy scripted configuration file at /home/you/.onlyterm.rhai but
scripted configs (rhai/Lua) are no longer supported: the config-scripting
engine has been removed from wezterm's live config-loading path in favor of
the static `ktav` format. Please migrate /home/you/.onlyterm.rhai to the
ktav format and save it as /home/you/.onlyterm.ktav. See the migration guide
for details.
```

OnlyTerm **does not** attempt to parse the legacy file — it only checks for
its existence so the error can name the exact file. To resolve it, translate
its *options and key bindings* (using the cheat sheet above) into a new
`.ktav` file with the same base name, and drop anything that depended on
scripting (event hooks, plugins, conditional logic) since there is currently
no replacement for those.

## Plugins

Plugins required a scripting engine to evaluate their `plugin/init.lua` (or
`plugin/init.rhai`) entry point. With no scripting engine left, plugins as a
mechanism are gone entirely — there is no `plugin::require(path)` and no
`plugin/init.ktav` equivalent, since a plugin's whole purpose was to run
code that computed part of your config. If you relied on a plugin,
translate the config values it used to produce into static ktav data by
hand in your own config file.

See [Plugins](config/plugins.md) for more on the current (much reduced)
state of plugin support.
