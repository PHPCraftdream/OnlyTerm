# Migrating from a Lua config to a rhai config

!!! danger "Breaking change"

    OnlyTerm's configuration language has changed from **Lua** (via the embedded
    `mlua`/LuaJIT engine) to [**rhai**](https://rhai.rs/) (a pure-Rust scripting
    language with JavaScript/Rust-like syntax). The Lua engine has been removed
    entirely: OnlyTerm no longer parses, evaluates, or understands `.lua` config
    files at runtime.

    If you have an existing `.wezterm.lua`, it will **no longer load** and you
    will see a clear error telling you so. This guide walks you through
    translating it to the new `.wezterm.rhai` format.

This is a *syntax* change, not just a file-rename. rhai is intentionally
Lua-like in spirit (dynamically typed, garbage-collected, expression-oriented)
but it is **not** Lua, and a Lua config will not run as-is. The good news is
that OnlyTerm's *configuration schema* — every option, every event name, every
key-assignment action — is unchanged. Only the language you write it in changed.

## TL;DR — the one-minute migration

1. Rename your config file:

   ```sh
   mv ~/.wezterm.lua ~/.wezterm.rhai
   ```

   (on Windows: `%USERPROFILE%\.wezterm.lua` → `%USERPROFILE%\.wezterm.rhai`).

2. Translate the *shape* of the file from Lua to rhai (see
   [Syntax translation cheat sheet](#syntax-translation-cheat-sheet) below).

3. If a `.lua` file is still present next to where OnlyTerm looks for a `.rhai`
   file but no `.rhai` sibling exists, OnlyTerm prints a dedicated error pointing
   at the exact file to migrate (see *What happens to an old `.wezterm.lua`*
   below).

---

## The shape of a rhai config

A Lua config builds a config table, mutates it, and `return`s it. A rhai config
does the same thing with an **object-map literal** (`#{ ... }`), which is rhai's
analogue of a Lua table used as a record.

### Lua (before)

```lua
-- Pull in the wezterm API
local wezterm = require 'wezterm'

-- This will hold the configuration.
local config = wezterm.config_builder()

-- Apply your config choices.
config.initial_cols = 120
config.initial_rows = 28
config.font_size = 10
config.color_scheme = 'AdventureTime'

-- Finally, return the configuration to wezterm:
return config
```

### rhai (after)

```rhai
// The whole script evaluates to a config object-map.
// There is no `require 'wezterm'` and no `config_builder()`:
// every option is just a key in the returned map.

#{
    initial_cols: 120,
    initial_rows: 28,
    font_size: 10.0,
    color_scheme: "AdventureTime",
}
```

Key differences you can already see:

| Concept | Lua | rhai |
|---|---|---|
| Get the OnlyTerm API | `local wezterm = require 'wezterm'` | nothing — functions are global or in modules like `color::`, `serde::`, `plugin::` |
| Build the config | `local config = wezterm.config_builder()` then mutate it | write an object-map literal `#{ ... }` (or build one with `let mut`) |
| Return it | `return config` | the **last expression** in the file is the value; an explicit `return` also works but is rarely needed |
| Comments | `-- line`, `--[[ block ]]` | `// line`, `/* block */` |
| Strings | `"..."` or `'...'` | `"..."` or `'...'` (same) |

## Syntax translation cheat sheet

### Variables and scope

Lua variables are global by default and use `local` for lexical scope. rhai
variables are **always lexically scoped** and must be declared with `let`
(immutable) or `let mut` (mutable); there are no globals.

```lua
-- Lua
local name = "wez"
local parts = { "a", "b" }   -- a table used as an array
local opts  = { bold = true } -- a table used as a record
```

```rhai
// rhai
let name = "wez";
let mut parts = ["a", "b"];   // an Array
let opts = #{ bold: true };   // an object Map
```

### Tables → Arrays and Maps

In Lua **everything is a table**, which can be either a 1-indexed array or a
key/value map (or both at once). rhai keeps these strictly separate:

* `[1, 2, 3]` is an **Array** (0-indexed), the analogue of a Lua sequence
  table.
* `#{ key: value, ... }` is an object **Map**, the analogue of a Lua record
  table.

OnlyTerm config values map onto these directly:

```lua
-- Lua
config.launch_menu = {
  { args = { "top" } },
  { args = { "bash" } },
}
config.window_padding = { left = 2, right = 2, top = 0, bottom = 0 }
```

```rhai
// rhai
// (assuming `let mut config = #{ };` first, or inline these in the returned map)
#{
    launch_menu: [
        #{ args: ["top"] },
        #{ args: ["bash"] },
    ],
    window_padding: #{ left: 2, right: 2, top: 0, bottom: 0 },
}
```

Note the array indices: Lua arrays are **1-based**, rhai arrays are **0-based**.
`parts[1]` in Lua is `parts[0]` in rhai.

### Functions and closures

```lua
-- Lua
local function greet(name)
  return "hello " .. name
end

local caps = function(s) return string.upper(s) end
```

```rhai
// rhai
fn greet(name) {
    "hello " + name
}

let caps = |s| s.to_upper();
```

* rhai function bodies use the last expression as the return value (no `return`
  needed for the common case), just like the top-level config.
* rhai closures use the `|args| body` pipe syntax.
* Lua's `..` string concatenation becomes `+` in rhai (see below).

### String concatenation

Lua uses `..`; rhai uses `+` (and `+=`).

```lua
-- Lua
local msg = "foo" .. "-" .. tostring(42)
```

```rhai
// rhai
let msg = "foo" + "-" + 42.to_string();
```

### Conditional expressions

```lua
-- Lua
if theme == "dark" then
  bg = "#000"
elseif theme == "light" then
  bg = "#fff"
else
  bg = "#333"
end
```

```rhai
// rhai
let bg = if theme == "dark" {
    "#000"
} else if theme == "light" {
    "#fff"
} else {
    "#333"
};
```

rhai `if`/`else` is an **expression** — it yields a value, so you often don't
need a separate variable + assignment at all.

Note also the inequality operator: Lua's `~=` is rhai's `!=`.

### Loops

```lua
-- Lua
for i, v in ipairs(items) do
  print(i, v)
end

for i = 1, 10 do
  print(i)
end
```

```rhai
// rhai
for (i, v) in items.enumerate() {
    print(i, v);
}

for i in 1..=10 {
    print(i);
}
```

### Method calls and the `:` vs `.` distinction

Lua uses `obj:method(args)` to pass `obj` as an implicit first `self` argument,
and `obj.method(args)` (with a dot) when it does not. rhai has **only** the dot
form: `obj.method(args)`. There is no `:` operator, and no implicit `self`
magic to keep track of.

```lua
-- Lua
local pane = window:active_pane()
local text = pane:get_lines_as_text()
```

```rhai
// rhai
let pane = window.active_pane();
let text = pane.get_lines_as_text();
```

### `nil` vs unit

Lua uses `nil` for "no value". rhai has no `nil`; the closest equivalent is the
**unit** value `()`, written as `()` in scripts. Optional fields are simply
omitted from a map rather than set to `nil`.

## Calling OnlyTerm helpers (`wezterm.*` → globals and modules)

In Lua, helpers lived under the `wezterm` table returned by
`require 'wezterm'` (e.g. `wezterm.on`, `wezterm.color.parse`,
`wezterm.json_encode`, `wezterm.home_dir`). In rhai there is no `wezterm`
prefix: some helpers are **top-level functions** and the rest live in
**modules** accessed with the `module::function(...)` syntax.

| Lua | rhai |
|---|---|
| `wezterm.on("event", fn)` | `on("event", \|args\| { ... })` |
| `wezterm.home_dir` | `home_dir()` |
| `wezterm.hostname` | `hostname()` |
| `wezterm.version` | `version()` |
| `wezterm.config_file` | `config_file()` |
| `wezterm.config_dir` | `config_dir()` |
| `wezterm.target_triple` | `target_triple()` |
| `wezterm.has_action("X")` | `has_action("X")` |
| `wezterm.add_to_config_reload_watch_list(p)` | `add_to_config_reload_watch_list(p)` (one path per call) |
| `wezterm.color.parse("#fff")` | `color::parse("#fff")` |
| `wezterm.json_encode(x)` | `serde::json_encode(x)` |
| `wezterm.json_decode(s)` | `serde::json_decode(s)` |
| `wezterm.plugin.require(path)` | `plugin::require(path)` |

The available modules are `color::`, `serde::`, `procinfo::`, `plugin::` and
`mux::` (the per-function reference pages under [config/lua](lua/config/index.md)
still document the available operations; only the *call syntax* changed).

## Registering event handlers (`wezterm.on` / `wezterm.emit`)

Registering an event handler is one of the most common things a non-trivial
config does. The mechanism is identical in intent — you call `on(name, fn)` with
the event name and a callback — but the callback is a rhai closure and the
registration is a **top-level `on(...)` call** in the config file (its side
effect runs when the config is evaluated).

```lua
-- Lua
local wezterm = require 'wezterm'

wezterm.on('update-right-status', function(window, pane)
  local name = pane:get_foreground_process_name()
  window:set_right_status(wezterm.format({
    { Background = { Color = '#333' } },
    { Text = ' ' .. name .. ' ' },
  }))
end)

return wezterm.config_builder()
```

```rhai
// rhai
on("update-right-status", |window, pane| {
    let name = pane.get_foreground_process_name();
    window.set_right_status(format(#[
        #{ Background: #{ Color: "#333" } },
        #{ Text: " " + name + " " },
    ]));
});

#{}
```

Notes:

* `on` is a global function — no `wezterm.` prefix.
* The handler arguments (`window`, `pane`, ...) are the same OnlyTerm objects as
  before; you call their methods with a dot (`window.set_right_status(...)`)
  rather than a colon (see *Method calls and the `:` vs `.` distinction* above).
* Returning `false` from a handler suppresses the default action, exactly as the
  Lua version did; any other return value (or no return) lets the default
  action proceed.
* `wezterm.format(...)` becomes the global `format(...)` helper (the
  `#{ Foreground: ... }` / `#{ Text: ... }` records are plain object maps).

There is no script-visible `emit(...)` function: emitting events is something
OnlyTerm itself does internally to drive your handlers. (The `EmitEvent` key
action still works to trigger a named event you registered with `on`.)

## Key bindings and actions

A key binding is a record with `key`, `mods` and `action` fields. In Lua the
action was built with `wezterm.action.SomeAction` or
`wezterm.action.SomeAction{ ... }`. In rhai there is **no `wezterm.action`
helper** — you write the action as the tagged value directly:

* a **simple** action (no arguments) is just its **name as a string**:
  `"Copy"`, `"Paste"`, `"DisableDefaultAssignment"`, ...
* an action **with arguments** is an object map with a single key — the action
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
#{
    keys: [
        #{ key: "c", mods: "CTRL|SHIFT", action: "Copy" },
        #{
            key: "t",
            mods: "CTRL",
            action: #{
                SpawnCommandInNewTab: #{ cwd: "/tmp", args: ["bash"] },
            },
        },
    ],
}
```

The complete list of available actions and their argument shapes is unchanged —
see the [key assignments reference](lua/keyassignment/index.md). To check
whether a name is a valid action from a script, use `has_action("Name")`.

## What happens to an old `.wezterm.lua`

When OnlyTerm looks for a config file in one of the [standard
locations](config/files.md) and finds a `.lua` file there but **no** `.rhai`
sibling, it stops and prints an error like:

```
Found a legacy Lua configuration file at /home/you/.wezterm.lua but Lua configs
are no longer supported: mlua has been removed from wezterm's live
config-loading path. Please rename /home/you/.wezterm.lua to
/home/you/.wezterm.rhai and adapt its syntax to rhai. See the migration guide
for details on translating a wezterm.lua config to wezterm.rhai.
```

OnlyTerm **does not** attempt to parse the `.lua` file — it only checks for its
existence so the error can name the exact file. To resolve it, rename the file
to `.rhai` and translate it using this guide.

If both a `.rhai` and a `.lua` are present, the `.rhai` file wins and the `.lua`
file is silently ignored (you can delete it once your migration is complete).

## Plugins

!!! warning "Git-based plugin installation was already removed"

    Separately from the Lua→rhai change, OnlyTerm no longer embeds a Git
    implementation, so cloning a plugin by URL is no longer possible in *any*
    config language. This section covers what that means now that the config
    language is rhai.

Two things changed about plugins:

1. **`wezterm.plugin.require(url)` no longer clones by URL.** Passing anything
   that looks like a git remote (`https://...`, `ssh://...`, `git://...`,
   `file://...`, or an `scp`-style `host:path`) is now an error. Plugins must be
   obtained by you (e.g. `git clone` them manually, or download a release) and
   then required by their **local directory path**.

2. **Plugins are now rhai, not Lua.** A plugin directory must contain a
   `plugin/init.rhai` entry point. A directory that only has the old
   `plugin/init.lua` will fail to load with an error explaining that Lua plugins
   are not compatible with the rhai engine and must be republished with a
   `plugin/init.rhai` entry point.

Requiring a local rhai plugin:

```rhai
// rhai
let a_plugin = plugin::require("/home/you/projects/myPlugin");

let mut config = #{};
a_plugin.apply_to_config(config);

config
```

`plugin::list()` and `plugin::update_all()` still exist as callable names (so
old configs don't crash with "no such function"), but they now report an error
explaining that git-based plugin management has been removed — they no longer
enumerate or update anything. To update a plugin, update the files in its local
directory yourself and reload your config.

See [Plugins](config/plugins.md) for the full plugin-authoring guide.

## A worked example: porting a realistic config

### Before (`.wezterm.lua`)

```lua
local wezterm = require 'wezterm'
local config = wezterm.config_builder()

config.color_scheme = 'Catppuccin Mocha'
config.font = wezterm.font('JetBrains Mono')
config.font_size = 11.0
config.window_background_opacity = 0.9

config.keys = {
  { key = 'C', mods = 'CTRL|SHIFT', action = wezterm.action.Copy },
  {
    key = 'Enter',
    mods = 'SUPER',
    action = wezterm.action.SplitHorizontal { domain = 'CurrentPaneDomain' },
  },
}

wezterm.on('format-window-title', function(tab)
  local pane = tab:active_pane()
  return pane.title
end)

return config
```

### After (`.wezterm.rhai`)

```rhai
on("format-window-title", |tab| {
    let pane = tab.active_pane();
    pane.title
});

#{
    color_scheme: "Catppuccin Mocha",
    // No `wezterm.font(...)` helper exists in rhai; write the TextStyle
    // value directly as an object map (its `font` field is an array of
    // FontAttributes, the first of which carries the family name).
    font: #{ font: [#{ family: "JetBrains Mono" }] },
    font_size: 11.0,
    window_background_opacity: 0.9,

    keys: [
        #{ key: "C", mods: "CTRL|SHIFT", action: "Copy" },
        #{
            key: "Enter",
            mods: "SUPER",
            action: #{
                SplitHorizontal: #{ domain: "CurrentPaneDomain" },
            },
        },
    ],
}
```

Things to notice in the port:

* `require 'wezterm'` / `config_builder()` are gone — the config is just the
  returned map.
* `wezterm.font(...)` has no rhai equivalent; write the value as an object map
  (`#{ font: [#{ family: "..." }] }`). The same applies to other Lua convenience
  constructors such as `wezterm.font_with_fallback(...)`.
* `wezterm.action.Copy` → the string `"Copy"`; `wezterm.action.SplitHorizontal{...}`
  → `#{ SplitHorizontal: #{ ... } }`.
* The `on(...)` handler uses a closure `|tab| { ... }`, the method calls use
  dots (`tab.active_pane()`), and the handler returns its last expression
  (`pane.title`) instead of an explicit `return`.
* Lua's `..` concatenation would become `+` (not needed in this example).

## Quick reference: config-file discovery

OnlyTerm looks for a `wezterm.rhai` (or `.wezterm.rhai`) file in the same
locations and order it previously looked for `.lua` files. See
[Configuration Files](config/files.md) for the full search order. The
`WEZTERM_CONFIG_FILE` environment variable, the `--config-file` CLI argument,
and thumb-drive mode (config next to `wezterm.exe` on Windows) all still work —
just point them at a `.rhai` file.
