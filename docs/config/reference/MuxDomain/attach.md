# `domain.attach()`

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

Attempts to attach the domain.

Attaching a domain will attempt to import the windows, tabs and panes from the
remote system into those of the local GUI.

Unlike the [AttachDomain](../keyassignment/AttachDomain.md) key assignment,
calling `domain:attach()` will *not* implicitly spawn a new pane into the
domain if the domain contains no panes. This is to provide flexibility when
used in the [gui-startup](../gui-events/gui-startup.md) event.

If the domain is already attached, calling this method again has no effect.

See also: [domain:detach()](detach.md) and [domain:state()](state.md).
