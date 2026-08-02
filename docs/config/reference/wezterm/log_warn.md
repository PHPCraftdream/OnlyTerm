---
title: wezterm.log_warn
tags:
 - utility
 - log
 - debug
---
# `wezterm.log_warn(arg, ..)`

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

{{since('20210314-114017-04b7cedd')}}

This function logs the provided message string through wezterm's logging layer
at 'WARN' level, which can be displayed via [ShowDebugOverlay](../keyassignment/ShowDebugOverlay.md) action.  If you started wezterm from a terminal that text will print
to the stdout of that terminal.  If running as a daemon for the multiplexer
server then it will be logged to the daemon output path.

```rhai
log_warn("Hello!")
```

{{since('20210814-124438-54e29167')}}

Now accepts multiple arguments, and those arguments can be of any type.


See also [log_info](log_info.md) and [log_error](log_error.md).

