---
title: wezterm.emit
tags:
 - event
---

# `wezterm.emit(event_name, args...)`

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

`wezterm.emit` resolves the registered callback(s) for the specified
event name and calls each of them in turn, passing the additional
arguments through to the callback.

If a callback returns `false` then it prevents later callbacks from
being called for this particular call to `wezterm.emit`, and `wezterm.emit`
will return `false` to indicate that no additional/default processing
should take place.

If none of the callbacks returned `false` then `wezterm.emit` will
itself return `true` to indicate that default processing should take
place.

This function has no special knowledge of which events are defined by
wezterm, or what their required arguments might be.

See [wezterm.on](on.md) for more information about event handling.

