# `pane.get_title()`

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

Returns the title of the pane. Process-supplied OSC 1/2 titles are ignored by
default and are accepted only when `allow_process_title_updates` is enabled.

The value returned by this method is the same as that used to display the
tab title if this pane were the only pane in the tab; if `OSC 1` was used
to set a non-empty string then that string will be returned.  Otherwise the
value for `OSC 2` will be returned.

On Microsoft Windows the OS-level PTY may emit OSC 2 as programs attach to the
console; the default setting prevents those updates from affecting the title.

If the title text is `onlyterm` and the pane is a local pane, then onlyterm will
attempt to resolve the executable path of the foreground process that is
associated with the pane and will use that instead of `onlyterm`.
