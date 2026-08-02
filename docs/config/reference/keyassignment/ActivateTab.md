# `ActivateTab`

Activate the tab specified by the argument value. eg: `0` activates the
leftmost tab, while `1` activates the second tab from the left, and so on.

{{since('20200620-160318-e00b076c')}}

`ActivateTab` now accepts negative numbers; these wrap around from the start
of the tabs to the end, so `-1` references the right-most tab, `-2` the tab
to its left and so on.


!!! note "No loops in ktav"

    The original version of this example used a Lua `for` loop to generate
    sixteen key bindings programmatically. ktav is a static data format
    with no loops or expressions, so each binding below is now spelled out
    explicitly instead of generated.

```
keys: [
  // CTRL+ALT + number to activate that tab
  { key: "1", mods: "CTRL|ALT", action: { ActivateTab: 0 } }
  { key: "2", mods: "CTRL|ALT", action: { ActivateTab: 1 } }
  { key: "3", mods: "CTRL|ALT", action: { ActivateTab: 2 } }
  { key: "4", mods: "CTRL|ALT", action: { ActivateTab: 3 } }
  { key: "5", mods: "CTRL|ALT", action: { ActivateTab: 4 } }
  { key: "6", mods: "CTRL|ALT", action: { ActivateTab: 5 } }
  { key: "7", mods: "CTRL|ALT", action: { ActivateTab: 6 } }
  { key: "8", mods: "CTRL|ALT", action: { ActivateTab: 7 } }
  // F1 through F8 to activate that tab
  { key: F1, action: { ActivateTab: 0 } }
  { key: F2, action: { ActivateTab: 1 } }
  { key: F3, action: { ActivateTab: 2 } }
  { key: F4, action: { ActivateTab: 3 } }
  { key: F5, action: { ActivateTab: 4 } }
  { key: F6, action: { ActivateTab: 5 } }
  { key: F7, action: { ActivateTab: 6 } }
  { key: F8, action: { ActivateTab: 7 } }
]
```


