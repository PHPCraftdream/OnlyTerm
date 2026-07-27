# `color.triad()`

{{since('20220807-113146-c2fee766')}}

Returns the other two colors that form a triad. The other colors
are at +/- 120 degrees in the HSL color wheel.

!!! warning "Pending rhai conversion"

    The code example(s) below still use Lua syntax from before OnlyTerm's
    config engine switched to rhai. The *option names, event names and
    object/method shapes* are unchanged -- only the scripting syntax differs.
    See the [migration guide](../../../migration-lua-to-rhai.md) for the Lua-to-rhai
    syntax mapping to translate this example yourself, or watch for a
    follow-up documentation pass that rewrites it directly.

```lua
local a, b = wezterm.color.parse('yellow'):triad()
```


