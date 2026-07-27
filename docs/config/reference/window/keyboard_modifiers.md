# window.keyboard_modifiers()

{{since('20230712-072601-f4abf8fd')}}

Returns two values; the keyboard modifiers and the key status leds.

Both values are exposed to lua as strings with `|`-delimited values
representing the various modifier keys and keyboard led states:

* Modifiers - is the same form as keyboard assignment modifiers
* Leds - possible flags are `"CAPS_LOCK"` and `"NUM_LOCK"`. Note that macOS
  doesn't have a num lock concept.

This example shows the current modifier and led status in the right status
area:

!!! warning "Pending rhai conversion"

    The code example(s) below still use Lua syntax from before OnlyTerm's
    config engine switched to rhai. The *option names, event names and
    object/method shapes* are unchanged -- only the scripting syntax differs.
    See the [migration guide](../../../migration-lua-to-rhai.md) for the Lua-to-rhai
    syntax mapping to translate this example yourself, or watch for a
    follow-up documentation pass that rewrites it directly.

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
