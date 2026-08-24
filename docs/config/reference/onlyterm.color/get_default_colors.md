# `onlyterm.color.get_default_colors()`

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

Returns the set of colors that would be used by default.

This is useful if you want to reference those colors in a color scheme
definition.

This contrived example sets up two color schemes and overrides their background
colors to red.  One of the schemes is the default set of colors, while the
other is one of the many built-in schemes:

```rhai
let my_gruvbox = color::get_builtin_schemes()["Gruvbox Light"]
my_gruvbox.background = "red"

let my_default = color::get_default_colors()
my_default.background = "red"

return #{
  color_schemes: #{
    "My Gruvbox": my_gruvbox,
    "My Default": my_default,
  },
  color_scheme: "My Gruvbox",
}
```
