---
title: onlyterm.background_child_process
tags:
 - utility
 - open
 - spawn
---

# `onlyterm.background_child_process(args)`

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

This function accepts an argument list; it will attempt to spawn that command
in the background.

May generate an error if the command is not able to be spawned (eg: perhaps
the executable doesn't exist), but not all operating systems/environments
report all types of spawn failures immediately upon spawn.

This function doesn't return any value.

This example shows how you might set up a custom key assignment that opens
the terminal background image in a separate image viewer process:

```lua
local onlyterm = require 'onlyterm'

return {
  window_background_image = '/home/user/Downloads/sunset-american-fork-canyon.jpg',
  keys = {
    {
      mods = 'CTRL|SHIFT',
      key = 'm',
      action = onlyterm.action_callback(function(win, pane)
        onlyterm.background_child_process {
          'xdg-open',
          win:effective_config().window_background_image,
        }
      end),
    },
  },
}
```

See also [run_child_process](run_child_process.md)

