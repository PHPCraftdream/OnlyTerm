# `wezterm` module

!!! danger "Removed: no scripting engine"

    This page documents part of the rhai (and, before that, Lua) **scripting
    API**, which has been removed entirely. OnlyTerm's configuration format
    is now [ktav](../../../migration-to-ktav.md), a static `key: value` data
    format with no expressions, function calls, or callbacks of any kind --
    there is nothing left in OnlyTerm that could call this function, invoke
    this method, or construct this object. The descriptions and examples
    below are kept for historical reference (e.g. if you're migrating a very
    old config and trying to understand what it used to do), but none of it
    is callable today. See the [changelog](../../../changelog.md#continuousnightly)
    for the full rationale.

The `wezterm` module was the primary module that exposed wezterm
configuration and control to a scripting config file. A Lua config would
typically place:

```lua
local wezterm = require 'wezterm'
```

at the top of the file to enable it; there is no equivalent in a ktav config,
since there is nothing left to `require`.

## Available functions, constants

