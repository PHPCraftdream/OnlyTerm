# `color.hsla()`

{{since('20220807-113146-c2fee766')}}

Converts the color to the HSL colorspace and returns those values +
alpha:

!!! warning "Pending rhai conversion"

    The code example(s) below still use Lua syntax from before OnlyTerm's
    config engine switched to rhai. The *option names, event names and
    object/method shapes* are unchanged -- only the scripting syntax differs.
    See the [migration guide](../../../migration-lua-to-rhai.md) for the Lua-to-rhai
    syntax mapping to translate this example yourself, or watch for a
    follow-up documentation pass that rewrites it directly.

```lua
local h, s, l, a = color:hsla()
```


