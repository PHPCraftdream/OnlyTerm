---
title: wezterm.open_with
tags:
 - utility
 - open
 - spawn
---

# `wezterm.open_with(path_or_url [, application])`

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

{{since('20220101-133340-7edc5b5a')}}

This function opens the specified `path_or_url` with either the specified
`application` or uses the default application if `application` was not passed
in.

```rhai
// Opens a URL in your default browser
open_with("http://example.com")

// Opens a URL specifically in firefox
open_with("http://example.com", "firefox")
```

