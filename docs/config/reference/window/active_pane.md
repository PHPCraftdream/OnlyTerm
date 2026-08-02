# `window.active_pane()`

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

{{since('20221119-145034-49b9839f')}}

A convenience accessor for returning the active pane in the active tab of the
GUI window.

This is similar to [mux_window:active_pane()](../mux-window/active_pane.md)
but, because it operates at the GUI layer, it can return *Pane* objects for
special overlay panes that are not visible to the mux layer of the API.

