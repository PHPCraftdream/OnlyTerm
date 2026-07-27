---
title: wezterm.config_dir
tags:
 - filesystem
---

# `wezterm.config_dir`

This constant is set to the path to the directory in which your `wezterm.lua`
configuration file was found.

!!! warning "Pending rhai conversion"

    The code example(s) below still use Lua syntax from before OnlyTerm's
    config engine switched to rhai. The *option names, event names and
    object/method shapes* are unchanged -- only the scripting syntax differs.
    See the [migration guide](../../../migration-lua-to-rhai.md) for the Lua-to-rhai
    syntax mapping to translate this example yourself, or watch for a
    follow-up documentation pass that rewrites it directly.

```lua
local wezterm = require 'wezterm'
wezterm.log_error('Config Dir ' .. wezterm.config_dir)
```


