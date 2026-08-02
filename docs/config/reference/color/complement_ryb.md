# `color.complement_ryb()`

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

Returns the complement of the color using the [RYB color
model](https://en.wikipedia.org/wiki/RYB_color_model), which more closely
matches how artists think of mixing colors.

The complement is computed by converting to HSL, converting the
hue angle to the equivalent RYB angle, rotating by 180 degrees and
and then converting back to RGBA.

See also: [color:complement()](complement.md).
