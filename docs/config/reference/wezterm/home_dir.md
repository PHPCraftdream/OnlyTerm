---
title: wezterm.home_dir
tags:
 - utility
 - filesystem
---

# `wezterm.home_dir`

This constant is set to the home directory of the user running `wezterm`.

!!! warning "Pending rhai conversion"

    The code example(s) below still use Lua syntax from before OnlyTerm's
    config engine switched to rhai. The *option names, event names and
    object/method shapes* are unchanged -- only the scripting syntax differs.
    See the [migration guide](../../../migration-lua-to-rhai.md) for the Lua-to-rhai
    syntax mapping to translate this example yourself, or watch for a
    follow-up documentation pass that rewrites it directly.

```lua
local wezterm = require 'wezterm'
wezterm.log_error('Home ' .. wezterm.home_dir)
```


