# `ActivateWindow(n)`

{{since('20230320-124340-559cb7b0')}}

Activates the *nth* GUI window, zero-based.

!!! note "`wezterm.gui.gui_windows()` no longer exists"

    Earlier versions of this page described this action as equivalent to
    the scripting expression `wezterm.gui.gui_windows()[n + 1]:focus()`.
    `wezterm.gui` was part of the scripting API and has been removed along
    with the rest of the scripting engine — see the
    [changelog](../../../changelog.md#continuousnightly). `ActivateWindow`
    itself is unaffected; only that illustrative scripting-equivalent
    description is now obsolete.

Here's an example of setting up hotkeys to activate specific windows:

!!! note "No loops in ktav"

    The original version of this example used a Lua `for` loop to generate
    eight key bindings programmatically. ktav is a static data format with
    no loops or expressions, so each binding below is now spelled out
    explicitly instead of generated.

```
keys: [
  ## CMD+ALT + number to activate that window
  { key: 1, mods: CMD|ALT, action: { ActivateWindow: 0 } }
  { key: 2, mods: CMD|ALT, action: { ActivateWindow: 1 } }
  { key: 3, mods: CMD|ALT, action: { ActivateWindow: 2 } }
  { key: 4, mods: CMD|ALT, action: { ActivateWindow: 3 } }
  { key: 5, mods: CMD|ALT, action: { ActivateWindow: 4 } }
  { key: 6, mods: CMD|ALT, action: { ActivateWindow: 5 } }
  { key: 7, mods: CMD|ALT, action: { ActivateWindow: 6 } }
  { key: 8, mods: CMD|ALT, action: { ActivateWindow: 7 } }
]
```


See also 
[ActivateWindowRelative](ActivateWindowRelative.md),
[ActivateWindowRelativeNoWrap](ActivateWindowRelativeNoWrap.md).
