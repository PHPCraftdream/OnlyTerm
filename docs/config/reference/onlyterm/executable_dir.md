---
title: onlyterm.executable_dir
tags:
 - filesystem
 - utility
---

# `onlyterm.executable_dir`

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

This constant is set to the directory containing the `onlyterm`
executable file.

```lua
local onlyterm = require 'onlyterm'
onlyterm.log_error('Exe dir ' .. onlyterm.executable_dir)
```


