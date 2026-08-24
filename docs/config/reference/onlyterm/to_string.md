---
title: onlyterm.to_string
tags:
 - utility
---
# `onlyterm.to_string(arg)`

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

{{since('20240127-113634-bbcac864')}}

This function returns a string representation of any rhai value. In particular
this can be used to get a string representation of an array or map.

The intended purpose is as a human readable way to inspect rhai values.  It is not machine
readable; do not attempt to use it as a serialization format as the format is not guaranteed
to remain the same across different versions of onlyterm.

This same representation is used in the [debug overlay](../keyassignment/ShowDebugOverlay.md)
when printing the result of an expression from the rhai REPL and for the implicit string
conversions of the parameters passed to [onlyterm.log_info](log_info.md).

```rhai
assert(to_string([1, 2]) == "[
    1,
    2,
]")
assert(to_string(#{ a: 1, b: 2 }) == "{
    \"a\": 1,
    \"b\": 2,
}")
```

