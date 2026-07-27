# window.composition_status()

{{since('20220319-142410-0fcdea07')}}

Returns a string holding the current dead key or IME composition text,
or `nil` if the input layer is not in a composition state.

This is the same text that is shown at the cursor position when composing.

This example shows how to show the composition status in the status area.
The cursor color is also changed to `orange` when in this state.

!!! warning "Pending rhai conversion"

    The code example(s) below still use Lua syntax from before OnlyTerm's
    config engine switched to rhai. The *option names, event names and
    object/method shapes* are unchanged -- only the scripting syntax differs.
    See the [migration guide](../../../migration-lua-to-rhai.md) for the Lua-to-rhai
    syntax mapping to translate this example yourself, or watch for a
    follow-up documentation pass that rewrites it directly.

```lua
local wezterm = require 'wezterm'

wezterm.on('update-right-status', function(window, pane)
  local compose = window:composition_status()
  if compose then
    compose = 'COMPOSING: ' .. compose
  end
  window:set_right_status(compose or '')
end)

return {
  colors = {
    compose_cursor = 'orange',
  },
}
```

See also: [window:leader_is_active()](leader_is_active.md).

