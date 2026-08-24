# `window.set_config_overrides(overrides)`

!!! danger "Removed: no scripting engine"

    This page documents part of the rhai (and, before that, Lua) **scripting
    API**, which has been removed entirely. OnlyTerm's configuration format
    is now [ktav](../../../migration-to-ktav.md), a static `key: value` data
    format with no expressions, function calls, or callbacks of any kind --
    there is nothing left in OnlyTerm that could call this function, invoke
    this method, or construct this object. The description and examples
    below are kept for historical reference (e.g. if you're migrating a very
    old config and trying to understand what it used to do), but none of it
    is callable today. See the [changelog](../../../changelog.md#continuousnightly)
    for the full rationale.

{{since('20210314-114017-04b7cedd')}}

Changes the set of configuration overrides for the window.
The config file is re-evaluated and any CLI overrides are
applied, followed by the keys and values from the `overrides`
parameter.

This can be used to override configuration on a per-window basis;
this is only useful for options that apply to the GUI window, such
as rendering the GUI.

Each call to `window:set_config_overrides` will emit the
[window-config-reloaded](../window-events/window-config-reloaded.md) event for
the window.  If you are calling this method from inside the handler
for `window-config-reloaded` you should take care to only call `window:set_config_overrides`
if the actual override values have changed to avoid a loop.

In this example, a key assignment (`CTRL-SHIFT-E`) is used to toggle the use of
ligatures in the current window:

```lua
local onlyterm = require 'onlyterm'

onlyterm.on('toggle-ligature', function(window, pane)
  local overrides = window:get_config_overrides() or {}
  if not overrides.harfbuzz_features then
    -- If we haven't overridden it yet, then override with ligatures disabled
    overrides.harfbuzz_features = { 'calt=0', 'clig=0', 'liga=0' }
  else
    -- else we did already, and we should disable out override now
    overrides.harfbuzz_features = nil
  end
  window:set_config_overrides(overrides)
end)

return {
  keys = {
    {
      key = 'E',
      mods = 'CTRL',
      action = onlyterm.action.EmitEvent 'toggle-ligature',
    },
  },
}
```

In this example, a key assignment (`CTRL-SHIFT-B`) is used to toggle opacity
for the window:

```lua
local onlyterm = require 'onlyterm'

onlyterm.on('toggle-opacity', function(window, pane)
  local overrides = window:get_config_overrides() or {}
  if not overrides.window_background_opacity then
    overrides.window_background_opacity = 0.5
  else
    overrides.window_background_opacity = nil
  end
  window:set_config_overrides(overrides)
end)

return {
  keys = {
    {
      key = 'B',
      mods = 'CTRL',
      action = onlyterm.action.EmitEvent 'toggle-opacity',
    },
  },
}
```

