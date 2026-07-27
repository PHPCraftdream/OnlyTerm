---
title: wezterm.to_string
tags:
 - utility
---
# `wezterm.to_string(arg)`

{{since('20240127-113634-bbcac864')}}

This function returns a string representation of any rhai value. In particular
this can be used to get a string representation of an array or map.

The intended purpose is as a human readable way to inspect rhai values.  It is not machine
readable; do not attempt to use it as a serialization format as the format is not guaranteed
to remain the same across different versions of wezterm.

This same representation is used in the [debug overlay](../keyassignment/ShowDebugOverlay.md)
when printing the result of an expression from the rhai REPL and for the implicit string
conversions of the parameters passed to [wezterm.log_info](log_info.md).

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

