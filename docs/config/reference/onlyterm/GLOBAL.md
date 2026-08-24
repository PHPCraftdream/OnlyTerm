---
title: onlyterm.GLOBAL
tags:
 - utility
---

# `onlyterm.GLOBAL`

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

Provides global, in-process, in-memory, data storage for json-like variables
that persists across config reloads.

onlyterm's lua files may be re-loaded and re-evaluated multiple times in
different contexts or in different threads. If you'd like to keep track
of state that lasts for the lifetime of your onlyterm process then you
cannot simply use global variables in the lua script.

`onlyterm.GLOBAL` is a special userdata value that acts like a table.
Writing to keys will copy the data that you assign into a global in-memory
table and allow it to be read back later.

Reads and writes from/to `onlyterm.GLOBAL` are thread-safe but don't currently
provide synchronization primitives for managing read-modify-write operations.

The following example shows the number of times that the config has been
loaded in the right status bar. Watch it increase when you press the
[ReloadConfiguration](../keyassignment/ReloadConfiguration.md) key assignment
(`CTRL-SHIFT-R`):

```lua
local onlyterm = require 'onlyterm'

-- Count how many times the lua config has been loaded
onlyterm.GLOBAL.parse_count = (onlyterm.GLOBAL.parse_count or 0) + 1

onlyterm.on('update-right-status', function(window, pane)
  window:set_right_status('Reloads=' .. tostring(onlyterm.GLOBAL.parse_count))
end)

return {}
```

Note that the reload counter goes up by more than 1 when you reload: that is
because the config is evaluated multiple times in different threads as part of
safely validating and setting up the configuration.

You may store values with the following types:

* string
* number
* table
* boolean

Attempting to assign other types will raise an error.

## Note about referencing table values

Accessing `onlyterm.GLOBAL` returns a copy of the data that you'd read, making
it cumbersome to perform read/modify/write updates; for example this:

```lua
onlyterm.GLOBAL.tab_titles['T0'] = 'Test'
```

will not result in `onlyterm.GLOBAL.tab_titles.T0` being set, and you need
to explicitly break it apart into:

```lua
local tab_titles = onlyterm.GLOBAL.tab_titles
tab_titles['T0'] = 'Test'
onlyterm.GLOBAL.tab_titles = tab_titles
```

{{since('20230320-124340-559cb7b0')}}

You no longer need to split apart read/modify/write and the simple assignment
now works as you would expect:

```lua
onlyterm.GLOBAL.tab_titles['T0'] = 'Test'
```

