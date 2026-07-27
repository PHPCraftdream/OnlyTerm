# `wezterm.color.parse(string)`

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

!!! warning "Pending rhai conversion"

    The code example(s) below still use Lua syntax from before OnlyTerm's
    config engine switched to rhai. The *option names, event names and
    object/method shapes* are unchanged -- only the scripting syntax differs.
    See the [migration guide](../../../migration-lua-to-rhai.md) for the Lua-to-rhai
    syntax mapping to translate this example yourself, or watch for a
    follow-up documentation pass that rewrites it directly.

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

