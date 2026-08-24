---
title: onlyterm.running_under_wsl
tags:
 - utility
---
# `onlyterm.running_under_wsl()`

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

This function returns a boolean indicating whether we believe that we are
running in a Windows Services for Linux (WSL) container.  In such an
environment the `onlyterm.target_triple` will indicate that we are running in
Linux but there will be some slight differences in system behavior (such as
filesystem capabilities) that you may wish to probe for in the configuration.

```lua
local onlyterm = require 'onlyterm'
onlyterm.log_error(
  'System '
    .. onlyterm.target_triple
    .. ' '
    .. tostring(onlyterm.running_under_wsl())
)
```


