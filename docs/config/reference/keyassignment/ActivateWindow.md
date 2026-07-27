# `ActivateWindow(n)`

{{since('20230320-124340-559cb7b0')}}

Activates the *nth* GUI window, zero-based.

Performing this action is equivalent to executing this lua code fragment:

!!! warning "Pending rhai conversion"

    The code example(s) below still use Lua syntax from before OnlyTerm's
    config engine switched to rhai. The *option names, event names and
    object/method shapes* are unchanged -- only the scripting syntax differs.
    See the [migration guide](../../../migration-lua-to-rhai.md) for the Lua-to-rhai
    syntax mapping to translate this example yourself, or watch for a
    follow-up documentation pass that rewrites it directly.

```lua
wezterm.gui.gui_windows()[n + 1]:focus()
```

Here's an example of setting up hotkeys to activate specific windows:

```lua
local wezterm = require 'wezterm'
local act = wezterm.action
local config = {}

config.keys = {}
for i = 1, 8 do
  -- CMD+ALT + number to activate that window
  table.insert(config.keys, {
    key = tostring(i),
    mods = 'CMD|ALT',
    action = act.ActivateWindow(i - 1),
  })
end

return config
```


See also 
[ActivateWindowRelative](ActivateWindowRelative.md),
[ActivateWindowRelativeNoWrap](ActivateWindowRelativeNoWrap.md).
