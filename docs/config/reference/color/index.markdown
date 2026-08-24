# `Color` object

!!! danger "Removed: no scripting engine"

    This page documents part of the rhai (and, before that, Lua) **scripting
    API**, which has been removed entirely. OnlyTerm's configuration format
    is now [ktav](../../../migration-to-ktav.md), a static `key: value` data
    format with no expressions, function calls, or callbacks of any kind --
    there is nothing left in OnlyTerm that could call this method or
    construct this object. The descriptions below are kept for historical
    reference. See the [changelog](../../../changelog.md#continuousnightly)
    for the full rationale.

Color objects could be created by calling
[onlyterm.color.parse()](../onlyterm.color/parse.md) and may also be
returned by various onlyterm functions and methods.

They represent a color that is internally stored in SRGBA.

Color objects have a number of methods that are helpful to
compare and compute other color values, which is helpful
when programmatically generating color schemes.

## Available methods



