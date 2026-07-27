---
title: wezterm.read_dir
tags:
 - utility
 - filesystem
---
# `wezterm.read_dir(path)`

{{since('20200503-171512-b13ef15f')}}

This function returns an array containing the absolute file names of the
directory specified.  Due to limitations in the scripting bindings, all of the paths
must be able to be represented as UTF-8 or this function will generate an
error.

!!! warning "Pending rhai conversion"

    The code example(s) below still use Lua syntax from before OnlyTerm's
    config engine switched to rhai. The *option names, event names and
    object/method shapes* are unchanged -- only the scripting syntax differs.
    See the [migration guide](../../../migration-lua-to-rhai.md) for the Lua-to-rhai
    syntax mapping to translate this example yourself, or watch for a
    follow-up documentation pass that rewrites it directly.

```lua
local wezterm = require 'wezterm'

-- logs the names of all of the entries under `/etc`
for _, v in ipairs(wezterm.read_dir '/etc') do
  wezterm.log_error('entry: ' .. v)
end
```


