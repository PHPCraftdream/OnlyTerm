---
tags:
  - prompt
---

# `Confirmation`

{{since('nightly')}}

!!! danger "Non-functional: required a scripting callback"

    `Confirmation`'s `action`/`cancel` fields were designed to hold an event
    callback registered via `wezterm.action_callback(...)`, resolved through
    rhai's (and, before that, Lua's) event-handler registry. That registry no
    longer exists — see the [changelog](../../../changelog.md#continuousnightly)
    and the [migration guide](../../../migration-to-ktav.md). The overlay
    below still displays and can be dismissed, but the answer (`Yes`/`No`)
    now goes nowhere: there is no handler left to receive it, so using this
    action currently has no observable effect beyond showing and closing a
    prompt. `action`/`cancel` still only accept the internal `EmitEvent`
    shape (for backwards-compatible config loading); any config still
    written against the example below will keep loading, but nothing runs
    in response to the user's choice.

Activates an overlay to display a confirmation menu.

`Confirmation` accepts the following fields:

* `message` - the text to show for confirmation. You may embed
  escape sequences. Defaults to: `"🛑 Really continue?"`.
* `action` - previously an event callback registered via
  `wezterm.action_callback`, called when the user selects `Yes`. No longer
  connected to anything (see above).
* `cancel` - previously an event callback registered via
  `wezterm.action_callback`, called when the user selects `No` or closes the
  confirmation menu. Optional. No longer connected to anything (see above).

## Historical example (no longer functional)

This is preserved for reference only; the callback here will not run in the
current version of OnlyTerm.

```rhai
config.keys = [
  #{
    key: "E",
    mods: "CTRL|SHIFT",
    action: act.Confirmation(#{
      message: "Do you want to run htop in a new window?",
      action: wezterm.action_callback(|window, pane| {
        window.perform_action(
          act.SpawnCommandInNewWindow(#{ args: ["htop"] }),
          pane
        );
      }),
      cancel: wezterm.action_callback(|window, pane| {
        wezterm.log_error("user declined");
      }),
    }),
  },
]
```

See also:
   * [InputSelector](InputSelector.md).
   * [PromptInputLine](PromptInputLine.md).
