# `pane.is_alt_screen_active()`

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

Returns whether the alternate screen is active for the pane.

The alternate screen is a secondary screen that is activated by certain escape codes. The alternate screen has no scrollback, which makes it ideal for a "full-screen" terminal program like `vim` or `less` to do whatever they want on the screen without fear of destroying the user's scrollback. Those programs emit escape codes to return to the normal screen when they exit.
