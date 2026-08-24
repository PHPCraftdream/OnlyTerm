# `onlyterm.time.parse(str, format)`

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

Parses a string that is formatted according to the supplied format string:

```
> onlyterm.time.parse("1983 Apr 13 12:09:14.274 +0000", "%Y %b %d %H:%M:%S%.3f %z")
"Time(utc: 1983-04-13T12:09:14.274+00:00)"
```

The format string supports the [set of formatting placeholders described here](https://docs.rs/chrono/latest/chrono/format/strftime/index.html).
