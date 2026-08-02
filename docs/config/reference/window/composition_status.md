# window.composition_status()

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

{{since('20220319-142410-0fcdea07')}}

Returns a string holding the current dead key or IME composition text,
or `nil` if the input layer is not in a composition state.

This is the same text that is shown at the cursor position when composing.

This example shows how to show the composition status in the status area.
The cursor color is also changed to `orange` when in this state.

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

