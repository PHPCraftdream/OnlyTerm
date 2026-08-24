# `window.perform_action(key_assignment, pane)`

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

{{since('20201031-154415-9614e117')}}

Performs a key assignment against the `window` and `pane`.
There are a number of actions that can be performed against a `pane` in
a `window` when configured via the `keys` and `mouse` configuration options.

This method allows your config's script code to trigger those actions for itself.

The first parameter is a key assignment such as that returned by [`act`](../onlyterm/action.md).

The second parameter is a `pane` object passed to your event callback.

For an example of this method in action, see [`onlyterm.on` Custom Events](../onlyterm/on.md#custom-events).
