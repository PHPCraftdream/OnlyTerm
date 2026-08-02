# `wezterm.gui` module

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

{{since('20220807-113146-c2fee766')}}

The `wezterm.gui` module exposed functions that operated on the gui layer.

The multiplexer may not be connected to a GUI, so attempting to resolve
this module from the mux server would return `nil`.

A Lua config would typically use something like:

```lua
local wezterm = require 'wezterm'
local gui = wezterm.gui
if gui then
  -- do something that depends on the gui layer
end
```

## Available functions, constants


