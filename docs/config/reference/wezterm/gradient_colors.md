---
title: wezterm.gradient_colors
tags:
 - color
---

# `wezterm.gradient_colors(gradient, num_colors)`

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

{{since('20210814-124438-54e29167')}}

Given a gradient spec and a number of colors, returns a table
holding that many colors spaced evenly across the range of
the gradient.

This is useful for example to generate colors for tabs, or
to do something fancy like interpolate colors across a gradient
based on the time of the day.

`gradient` is any gradient allowed by the
[window_background_gradient](../config/window_background_gradient.md) option.

This example is what you'd see if you opened up the [debug overlay](../keyassignment/ShowDebugOverlay.md) to try this out in the repl:

```
> wezterm.gradient_colors({preset="Rainbow"}, 4)
["#6e40aa", "#ff8c38", "#5dea8d", "#6e40aa"]
```

{{since('20220807-113146-c2fee766')}}

This function has moved to
[wezterm.color.gradient](../wezterm.color/gradient.md) and that name
should be used instead of this name.

In addition, the returned colors are now [Color
objects](../color/index.md).
