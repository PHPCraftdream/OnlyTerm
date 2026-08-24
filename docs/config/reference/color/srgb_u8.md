# `color.srgba_u8()`

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

Returns a tuple of the internal SRGBA colors expressed
as unsigned 8-bit integers in the range 0-255:

```
> r, g, b, a = onlyterm.color.parse("purple"):srgba_u8()
> print(r, g, b, a)
07:30:20.045 INFO logging > lua: 128 0 128 255
```
