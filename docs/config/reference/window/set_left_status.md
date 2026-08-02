# `window.set_left_status(string)`

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

This method can be used to change the content that is displayed in the tab bar,
to the left of the tabs.  The content is displayed
left-aligned and will take as much space as needed to display the content
that you set; it will not be implicitly clipped.

The parameter is a string that can contain escape sequences that change
presentation.

It is recommended that you use [wezterm.format](../wezterm/format.md) to
compose the string.

See [window:set_right_status](set_right_status.md) for examples.

