# window.keyboard_modifiers()

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

{{since('20230712-072601-f4abf8fd')}}

Returns two values; the keyboard modifiers and the key status leds.

Both values are exposed to lua as strings with `|`-delimited values
representing the various modifier keys and keyboard led states:

* Modifiers - is the same form as keyboard assignment modifiers
* Leds - possible flags are `"CAPS_LOCK"` and `"NUM_LOCK"`. Note that macOS
  doesn't have a num lock concept.

This example shows the current modifier and led status in the right status
area:

```lua
local wezterm = require 'wezterm'

local config = wezterm.config_builder()

config.debug_key_events = true

wezterm.on('update-status', function(window, pane)
  local mods, leds = window:keyboard_modifiers()
  window:set_right_status('mods=' .. mods .. ' leds=' .. leds)
end)

return config
```
