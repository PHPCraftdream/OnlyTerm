---
title: onlyterm.version
tags:
 - utility
 - version
---
# `onlyterm.version`

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

This constant is set to the `onlyterm` version string that is also reported
by running `onlyterm -V`.  This can potentially be used to adjust configuration
according to the installed version.

The version string looks like `20200406-151651-5b700e4`.  You can compare the
strings lexicographically if you wish to test whether a given version is newer
than another; the first component is the date on which the release was made,
the second component is the time and the final component is a git hash.

```lua
local onlyterm = require 'onlyterm'
onlyterm.log_error('Version ' .. onlyterm.version)
```


