# `onlyterm.mux` module

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

{{since('20220624-141144-bd1b7c5d')}}

The `onlyterm.mux` module exposed functions that operated on the multiplexer layer.

The multiplexer manages the set of running programs into panes, tabs, windows
and workspaces.

The multiplexer may not be connected to a GUI so certain operations that require
a running Window management system were not present in the interface exposed
by this module.

A Lua config would typically use something like:

```lua
local onlyterm = require 'onlyterm'
local mux = onlyterm.mux
```

at the top of the file to access it.

## Important Note!

*You should **avoid using, at the file scope in your config**, mux functions that cause new splits, tabs or windows to be created. The configuration file can be evaluated multiple times in various contexts. If you want to spawn new programs when onlyterm starts up, look at the [gui-startup](../gui-events/gui-startup.md) and [mux-startup](../mux-events/mux-startup.md) events.*

## Available functions, constants


