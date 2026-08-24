# `window-focus-changed`

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

{{since('20221119-145034-49b9839f')}}

The `window-focus-changed` event is emitted when the focus state for a window
is changed.

This event is fire-and-forget from the perspective of onlyterm; it fires the
event to advise of the config change, but has no other expectations.

The first event parameter is a [`window` object](../window/index.md) that
represents the gui window.

The second event parameter is a [`pane` object](../pane/index.md) that
represents the active pane in that window.

```lua
local onlyterm = require 'onlyterm'

onlyterm.on('window-focus-changed', function(window, pane)
  onlyterm.log_info(
    'the focus state of ',
    window:window_id(),
    ' changed to ',
    window:is_focused()
  )
end)
```

