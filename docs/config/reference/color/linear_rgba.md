# `color.linear_rgba()`

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

Returns a tuple of the colors converted to linear RGBA and
expressed as floating point numbers in the range `0.0-1.0`:

```
> r, g, b, a = onlyterm.color.parse("purple"):linear_rgba()
> print(r, g, b, a)
07:32:17.734 INFO logging > lua: 0.2158605307340622 0 0.2158605307340622 1
```

