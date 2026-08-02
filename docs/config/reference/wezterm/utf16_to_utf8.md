---
title: wezterm.utf16_to_utf8
tags:
 - utility
 - string
---
# `wezterm.utf16_to_utf8(str)`

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

This function is overly specific and exists primarily to workaround
[this wsl.exe issue](https://github.com/microsoft/WSL/issues/4456).

It takes as input a string and attempts to convert it from utf16 to utf8.

```rhai
let result = run_child_process([ "wsl.exe", "-l" ])
let wsl_list = utf16_to_utf8(result[1])
```

