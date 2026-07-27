# `ActivateTab`

Activate the tab specified by the argument value. eg: `0` activates the
leftmost tab, while `1` activates the second tab from the left, and so on.

{{since('20200620-160318-e00b076c')}}

`ActivateTab` now accepts negative numbers; these wrap around from the start
of the tabs to the end, so `-1` references the right-most tab, `-2` the tab
to its left and so on.


!!! warning "Pending rhai conversion"

    The code example(s) below still use Lua syntax from before OnlyTerm's
    config engine switched to rhai. The *option names, event names and
    object/method shapes* are unchanged -- only the scripting syntax differs.
    See the [migration guide](../../../migration-lua-to-rhai.md) for the Lua-to-rhai
    syntax mapping to translate this example yourself, or watch for a
    follow-up documentation pass that rewrites it directly.

```lua
local wezterm = require 'wezterm'
local act = wezterm.action
local config = {}

config.keys = {}
for i = 1, 8 do
  -- CTRL+ALT + number to activate that tab
  table.insert(config.keys, {
    key = tostring(i),
    mods = 'CTRL|ALT',
    action = act.ActivateTab(i - 1),
  })
  -- F1 through F8 to activate that tab
  table.insert(config.keys, {
    key = 'F' .. tostring(i),
    action = act.ActivateTab(i - 1),
  })
end

return config
```


