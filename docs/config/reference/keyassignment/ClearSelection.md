# `ClearSelection`

{{since('20220624-141144-bd1b7c5d')}}

Clears the selection in the current pane.

This example shows how it's used together with [Multiple](Multiple.md) to
copy to the clipboard and then clear the selection in a single key press
(the built-in default `CTRL-C` binding already does something similar to
this, conditionally, but this shows how you could always run both steps
back-to-back):

```
keys: [
  {
    key: c
    mods: CTRL|SHIFT
    action: {
      Multiple: [
        { CopyTo: ClipboardAndPrimarySelection }
        ClearSelection
      ]
    }
  }
]
```

!!! danger "Non-functional: conditional variant required a scripting callback"

    An earlier version of this example rebound plain `CTRL-C` to
    conditionally copy-and-clear only when there was an active selection
    (falling back to sending a literal `CTRL-C` interrupt byte otherwise),
    using `wezterm.action_callback(...)` to inspect
    `window:get_selection_text_for_pane(pane)` at run time and branch on it.
    That relied on the scripting engine, which has been removed — see the
    [changelog](../../../changelog.md#continuousnightly). There is currently
    no way to express that conditional behavior in ktav; the unconditional
    `Multiple` binding above is the closest static equivalent. (OnlyTerm's
    own default `CTRL-C` binding implements the conditional behavior
    natively in Rust, not via config.)
