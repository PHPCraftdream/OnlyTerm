---
title: wezterm.read_dir
tags:
 - utility
 - filesystem
---
# `wezterm.read_dir(path)`

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

{{since('20200503-171512-b13ef15f')}}

This function returns an array containing the absolute file names of the
directory specified.  Due to limitations in the scripting bindings, all of the paths
must be able to be represented as UTF-8 or this function will generate an
error.

```lua
local wezterm = require 'wezterm'

-- logs the names of all of the entries under `/etc`
for _, v in ipairs(wezterm.read_dir '/etc') do
  wezterm.log_error('entry: ' .. v)
end
```


