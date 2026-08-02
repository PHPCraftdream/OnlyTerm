# `Time` object

!!! danger "Removed: no scripting engine"

    This page documents part of the rhai (and, before that, Lua) **scripting
    API**, which has been removed entirely. OnlyTerm's configuration format
    is now [ktav](../../../../migration-to-ktav.md), a static `key: value`
    data format with no expressions, function calls, or callbacks of any
    kind -- there is nothing left in OnlyTerm that could call this method or
    construct this object. The descriptions below are kept for historical
    reference. See the [changelog](../../../../changelog.md#continuousnightly)
    for the full rationale.

Represented a date and time that was tracked internally as UTC.

Using `tostring()` on a `Time` object will show the internally tracked UTC time
information.

## Available methods
