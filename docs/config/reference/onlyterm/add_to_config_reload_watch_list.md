---
title: onlyterm.add_to_config_reload_watch_list
tags:
 - reload
---

# onlyterm.add_to_config_reload_watch_list(path)

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

{{since('20210814-124438-54e29167')}}

Adds `path` to the list of files that are watched for config changes.
If [automatically_reload_config](../config/automatically_reload_config.md)
is enabled, then the config will be reloaded when any of the files
that have been added to the watch list have changed.

{{since('20220807-113146-c2fee766')}}

In the old Lua config engine, this function was also called implicitly
whenever your config `require`d another Lua file. The rhai engine does not
yet wire this up automatically for its module/`import` system, so for now
you should call `add_to_config_reload_watch_list` explicitly for any file
your config depends on if you want edits to it to trigger an automatic
config reload.
