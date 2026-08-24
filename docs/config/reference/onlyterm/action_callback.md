---
title: onlyterm.action_callback
tags:
 - keys
 - event
---

# `onlyterm.action_callback(callback)`

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

This function is a helper to register a custom event and return an action triggering it.

It is helpful to write custom key bindings directly, without having to declare
the event and use it in a different place.

The implementation is essentially the same as:
```lua
function onlyterm.action_callback(callback)
  local event_id = '...' -- the function generates a unique event id
  onlyterm.on(event_id, callback)
  return onlyterm.action.EmitEvent(event_id)
end
```

See [onlyterm.on](./on.md) and [onlyterm.action](./action.md) for more info on what you can do with these.


## Usage

```lua
local onlyterm = require 'onlyterm'

return {
  keys = {
    {
      mods = 'CTRL|SHIFT',
      key = 'i',
      action = onlyterm.action_callback(function(win, pane)
        onlyterm.log_info 'Hello from callback!'
        onlyterm.log_info(
          'WindowID:',
          win:window_id(),
          'PaneID:',
          pane:pane_id()
        )
      end),
    },
  },
}
```
