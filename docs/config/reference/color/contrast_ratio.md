# `color.contrast_ratio(color)`

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

{{since('20220807-113146-c2fee766')}}

Computes the contrast ratio between the two colors.

```
> wezterm.color.parse("red"):contrast_ratio(wezterm.color.parse("yellow"))
1
> wezterm.color.parse("red"):contrast_ratio(wezterm.color.parse("navy"))
1.8273614734023298
```

The contrast ratio is computed by first converting to HSL, taking the
L components, and diving the lighter one by the darker one.

A contrast ratio of 1 means no contrast.

The maximum possible contrast ratio is 21:

```
> wezterm.color.parse("black"):contrast_ratio(wezterm.color.parse("white"))
21
```

