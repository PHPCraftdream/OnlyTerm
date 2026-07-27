# `window.current_event()`

{{since('20220807-113146-c2fee766')}}

Returns the current event.
For now only implemented for mouse events.

This example prints the delta scroll value
when you scroll up with your mouse wheel while holding `CTRL`:

!!! warning "Pending rhai conversion"

    The code example(s) below still use Lua syntax from before OnlyTerm's
    config engine switched to rhai. The *option names, event names and
    object/method shapes* are unchanged -- only the scripting syntax differs.
    See the [migration guide](../../../migration-lua-to-rhai.md) for the Lua-to-rhai
    syntax mapping to translate this example yourself, or watch for a
    follow-up documentation pass that rewrites it directly.

```lua
local wezterm = require 'wezterm'

return {
  mouse_bindings = {
    {
      event = { Down = { streak = 1, button = { WheelUp = 1 } } },
      mods = 'CTRL',
      action = wezterm.action_callback(function(window, pane)
        -- note that you want `WheelDown` for a `WheelDown` event
        local delta = window:current_event().Down.button.WheelUp
        wezterm.log_info('delta is: ' .. delta)
      end),
    },
  },
}
```
