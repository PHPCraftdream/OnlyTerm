---
title: wezterm.sleep_ms
tags:
 - utility
 - time
---
# `wezterm.sleep_ms(milliseconds)`

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

{{since('20201031-154415-9614e117')}}

`wezterm.sleep_ms` suspends execution of the script for the specified
number of milliseconds.  After that time period has elapsed, the script
continues running at the next statement.

