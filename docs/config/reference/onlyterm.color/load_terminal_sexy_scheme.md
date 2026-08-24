# `onlyterm.color.load_terminal_sexy_scheme(file_name)`

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

Loads a json file exported from [terminal.sexy](https://terminal.sexy/)
and returns it as a onlyterm color scheme.

Note that onlyterm ships with all of the pre-defined terminal.sexy color
schemes, so this function is primarily useful if you want to design a color
scheme using terminal.sexy and then import it to onlyterm.

This function returns a tuple of the color definitions and the metadata.

For example, given a json file with these contents:

```json
{
  "name": "",
  "author": "",
  "color": [
    "#282a2e",
    "#a54242",
    "#8c9440",
    "#de935f",
    "#5f819d",
    "#85678f",
    "#5e8d87",
    "#707880",
    "#373b41",
    "#cc6666",
    "#b5bd68",
    "#f0c674",
    "#81a2be",
    "#b294bb",
    "#8abeb7",
    "#c5c8c6"
  ],
  "foreground": "#c5c8c6",
  "background": "#1d1f21"
}
```

Then:

```
> colors, metadata = onlyterm.color.load_terminal_sexy_scheme("/path/to/file.json")
> print(colors)
22:37:10.416 INFO logging > lua: {
    "ansi": [
      "#282a2e",
      "#a54242",
      "#8c9440",
      "#de935f",
      "#5f819d",
      "#85678f",
      "#5e8d87",
      "#707880",
    ],
    "background": "#1d1f21",
    "brights": [
      "#373b41",
      "#cc6666",
      "#b5bd68",
      "#f0c674",
      "#81a2be",
      "#b294bb",
      "#8abeb7",
      "#c5c8c6"
    ],
    "foreground": "#c5c8c6",
}
> print(metadata)
22:37:06.041 INFO logging > lua: {
    "name": "",
    "author": ""
}
```
