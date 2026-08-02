# `wezterm.color.parse(string)`

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

{{since('20220807-113146-c2fee766')}}

Parses the passed color and returns a [Color
object](../color/index.md).  `Color` objects evaluate as strings but
have a number of methods that allow transforming and comparing
colors.

```
> wezterm.color.parse("black")
#000000
```

This example picks a foreground color, computes its complement in
the "artist's color wheel" to produce a purple color and then
darkens it to use it as a background color:

```lua
local wezterm = require 'wezterm'

local fg = wezterm.color.parse 'yellow'
local bg = fg:complement_ryb():darken(0.2)

return {
  colors = {
    foreground = fg,
    background = bg,
  },
}
```

