# `MoveTab`

Move the tab so that it has the index specified by the argument. eg: `0`
moves the tab to be  leftmost, while `1` moves the tab so that it is second tab
from the left, and so on.

!!! note "No loops in ktav"

    The original version of this example used a Lua `for` loop to generate
    eight key bindings programmatically. ktav is a static data format with
    no loops or expressions, so each binding below is now spelled out
    explicitly instead of generated.

```
keys: [
  ## CTRL+ALT + number to move to that position
  { key: 1, mods: CTRL|ALT, action: { MoveTab: 0 } }
  { key: 2, mods: CTRL|ALT, action: { MoveTab: 1 } }
  { key: 3, mods: CTRL|ALT, action: { MoveTab: 2 } }
  { key: 4, mods: CTRL|ALT, action: { MoveTab: 3 } }
  { key: 5, mods: CTRL|ALT, action: { MoveTab: 4 } }
  { key: 6, mods: CTRL|ALT, action: { MoveTab: 5 } }
  { key: 7, mods: CTRL|ALT, action: { MoveTab: 6 } }
  { key: 8, mods: CTRL|ALT, action: { MoveTab: 7 } }
]
```


