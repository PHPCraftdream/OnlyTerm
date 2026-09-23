---
tags:
  - keys
---
# `pass_through_next_key_on_double_ctrl`

Two quick taps of either Ctrl key arm a one-shot pass-through mode: the next
physical key press is sent to the application in the active pane without being
matched against any OnlyTerm key binding. Use it when a program running inside
the terminal needs a chord that OnlyTerm would normally intercept --
`Ctrl+Shift+C` or `Ctrl+Tab`, for example: tap Ctrl twice, then press the
chord, and the program receives it exactly as if OnlyTerm had no binding for
it. Key handling returns to normal after that one press.

A tap is a press and release of `LeftControl` or `RightControl` where:

- nothing else is pressed in between -- no other key, and no mouse button
  press or wheel tick. A Ctrl+click therefore never counts, and neither does
  the synthetic Ctrl that Windows sends ahead of `RightAlt` on AltGr layouts
  (typing `@` or `€` cannot arm the mode by accident);
- the key is released within 400ms of the press.

The second tap's press must follow the first tap's release within 400ms. The
mode arms when the second Ctrl is *released*, not when it is pressed: a tap
followed by an ordinary `Ctrl+C` chord arms nothing, because the chord's Ctrl
press breaks the second tap before `C` arrives, and `Ctrl+C` keeps working as
a binding.

## While the mode is armed

Every key press -- bare modifiers included -- skips OnlyTerm's key bindings,
leader key and key tables, exactly as if no binding had matched. The mode is
spent by the first non-modifier key press, or by composed text arriving from
an input method:

- that one physical press is spent whole: autorepeats of the held key keep
  going to the application until it is released, so holding eg. `Ctrl+Shift+Up`
  cannot switch from passing through to triggering a binding mid-hold;
- a *different* key pressed afterwards is handled by OnlyTerm normally again.

The mode also ends when the window loses focus (key releases can be lost
across Alt+Tab), or straight away if you perform the same double-tap gesture
again while it is armed. There is no timeout. The Ctrl presses that make up
the arming gesture itself still reach the application as usual -- the mode
changes how the key *after* arming is treated, not the taps.

System shortcuts such as `Alt+F4` or `Alt+Space` are out of scope: Windows
handles them before OnlyTerm ever sees a key event, and this pass-through
only ever concerns OnlyTerm's own bindings.

## Indicators

While the mode is armed:

- the active pane's cursor is drawn in the theme's `compose_cursor` color --
  the same accent already used for the leader key and dead-key composition;
- with the default fancy tab bar (`use_fancy_tab_bar: true`), the active
  tab's border is drawn in a fixed bright blue, independent of the color
  scheme (the border's thickness does not change, so the tab does not shift
  or resize). The tab border cue does not appear when `use_fancy_tab_bar` is
  `false`, and the cursor cue is not visible if the application has hidden
  the terminal cursor
  (the same limitation the leader indicator has).

## Disabling

The default is `true`. Set it to `false` and double-tapping Ctrl does nothing
special:

```
pass_through_next_key_on_double_ctrl: false
```

The setting is re-read on config reload: toggling it applies without
restarting the window, and reloading while the mode is armed also disarms it.
