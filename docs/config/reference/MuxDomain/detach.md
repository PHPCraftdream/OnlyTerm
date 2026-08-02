# `domain.detach()`

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

{{since('20230320-124340-559cb7b0')}}

Attempts to detach the domain.

Detaching a domain causes it to disconnect and remove its set of windows, tabs
and panes from the local GUI. Detaching does not cause those panes to close; if
or when you later attach to the domain, they'll still be there.

Not every domain supports detaching, and will log an error to the error
log/debug overlay.
