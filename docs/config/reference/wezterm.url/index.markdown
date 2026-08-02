# `wezterm.url` module

!!! danger "Removed: no scripting engine"

    This page documents part of the rhai (and, before that, Lua) **scripting
    API**, which has been removed entirely. OnlyTerm's configuration format
    is now [ktav](../../../migration-to-ktav.md), a static `key: value` data
    format with no expressions, function calls, or callbacks of any kind --
    there is nothing left in OnlyTerm that could call this function. The
    descriptions below are kept for historical reference. See the
    [changelog](../../../changelog.md#continuousnightly) for the full
    rationale.

{{since('20240127-113634-bbcac864')}}

The `wezterm.url` module exposed functions that allowed working
with URLs.

## Available functions and objects


