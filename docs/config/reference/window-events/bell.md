# `bell`

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

{{since('20211204-082213-a66c61ee9')}}

The `bell` event is emitted when the ASCII BEL sequence is emitted to
a pane in the window.

Defining an event handler doesn't alter onlyterm's handling of the bell;
the event supplements it and allows you to take additional action over
the configured behavior.

The first event parameter is a [`window` object](../window/index.md) that
represents the gui window.

The second event parameter is a [`pane` object](../pane/index.md) that
represents the pane in which the bell was rung, which may not be active
pane--it could be in an unfocused pane or tab..

```lua
local onlyterm = require 'onlyterm'

onlyterm.on('bell', function(window, pane)
  onlyterm.log_info('the bell was rung in pane ' .. pane:pane_id() .. '!')
end)

return {}
```

See also [audible_bell](../config/audible_bell.md) and [visual_bell](../config/visual_bell.md).
