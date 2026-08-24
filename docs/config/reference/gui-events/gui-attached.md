# `gui-attached`

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

{{since('20230320-124340-559cb7b0')}}

This event is triggered when the GUI is starting up after attaching the
selected domain.  For example, when you use `onlyterm connect DOMAIN` or
`onlyterm start --domain DOMAIN` to start the GUI, the `gui-attached` event will
be triggered and passed the [MuxDomain](../MuxDomain/index.md) object
associated with `DOMAIN`.  In cases where you don't specify the domain, the
default domain will be passed instead.

This event fires after the [gui-startup](gui-startup.md) event.

Note that the `gui-startup` event does not fire when invoking `onlyterm connect
DOMAIN` or `onlyterm start --domain DOMAIN --attach`.

You can use this opportunity to take whatever action suits your purpose; some
users like to maximize all of their windows on startup, and this event would
allow you do that:

```lua
local onlyterm = require 'onlyterm'
local mux = onlyterm.mux

onlyterm.on('gui-attached', function(domain)
  -- maximize all displayed windows on startup
  local workspace = mux.get_active_workspace()
  for _, window in ipairs(mux.all_windows()) do
    if window:get_workspace() == workspace then
      window:gui_window():maximize()
    end
  end
end)

local config = onlyterm.config_builder()

return config
```

See also: [gui-startup](gui-startup.md).
