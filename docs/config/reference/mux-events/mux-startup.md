# `mux-startup`

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

{{since('20220624-141144-bd1b7c5d')}}

The `mux-startup` event is emitted once when the mux server is starting up.
It is triggered before any default program is started.

If the `mux-startup` event causes any panes to be created then those will
take precedence over the default program configuration and no additional
default program will be spawned.

This event is useful for starting a set of programs in a standard
configuration to save you the effort of doing it manually each time:

```lua
local onlyterm = require 'onlyterm'
local mux = onlyterm.mux

-- this is called by the mux server when it starts up.
-- It makes a window split top/bottom
onlyterm.on('mux-startup', function()
  local tab, pane, window = mux.spawn_window {}
  pane:split { direction = 'Top' }
end)

return {
  unix_domains = {
    { name = 'unix' },
  },
}
```

See also:
* [onlyterm.mux](../onlyterm.mux/index.md)
