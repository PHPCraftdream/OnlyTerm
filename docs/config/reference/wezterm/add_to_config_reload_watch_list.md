---
title: wezterm.add_to_config_reload_watch_list
tags:
 - reload
---

# wezterm.add_to_config_reload_watch_list(path)

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
