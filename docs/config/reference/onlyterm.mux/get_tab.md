# `onlyterm.mux.get_tab(TAB_ID)`

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

{{since('20220624-141144-bd1b7c5d')}}

Given a tab ID, verifies that the ID is a valid tab known to the mux
and returns a [MuxTab](../MuxTab/index.md) object that can be used to
operate on the tab.

This is useful for situations where you have obtained a tab id from
some other source and want to use the various `MuxTab` methods with it.

