---
title: onlyterm.run_child_process
tags:
 - utility
 - open
 - spawn
---
# `onlyterm.run_child_process(args)`

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

{{since('20200503-171512-b13ef15f')}}

This function accepts an argument list; it will attempt to spawn that command
and will return a 3-element array consisting of the boolean success of the
invocation, the stdout data and the stderr data.

```rhai
let result = run_child_process([ "ls", "-l" ])
let success = result[0]
let stdout = result[1]
let stderr = result[2]
```

See also [background_child_process](background_child_process.md)
