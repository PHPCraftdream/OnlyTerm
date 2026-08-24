---
tags:
  - prompt
---

# `PromptInputLine`

{{since('20230408-112425-69ae8472')}}

!!! danger "Non-functional: required a scripting callback"

    `PromptInputLine`'s `action` field was designed to hold an event
    callback registered via `onlyterm.action_callback(...)`, resolved through
    rhai's (and, before that, Lua's) event-handler registry. That registry no
    longer exists — see the [changelog](../../../changelog.md#continuousnightly)
    and the [migration guide](../../../migration-to-ktav.md). The overlay
    below still displays and accepts a line of input, but the entered text
    now goes nowhere: there is no handler left to receive it, so using this
    action currently has no observable effect beyond showing and closing a
    prompt. `action` still only accepts the internal `EmitEvent` shape (for
    backwards-compatible config loading); any config still written against
    the examples below will keep loading, but nothing runs with the entered
    text.

Activates an overlay to display a prompt and request a line of input
from the user.

`PromptInputLine` accepts four fields:

* `description` - the text to show at the top of the display area. You may
  embed escape sequences.
* `action` - previously an event callback registered via
  `onlyterm.action_callback`, called with the entered line. No longer
  connected to anything (see above).
* `prompt` - the text to show as the prompt. You may embed escape sequences.
  Defaults to: `"> "`. {{since('nightly', inline=True)}}
* `initial_value` - optional.  If provided, the initial content of the input
  field will be set to this value.  The user may edit it prior to submitting
  the input. {{since('nightly', inline=True)}}

## Historical examples (no longer functional)

These are preserved for reference only; the callbacks here will not run in
the current version of OnlyTerm.

```rhai
config.keys = [
  #{
    key: "E",
    mods: "CTRL|SHIFT",
    action: act.PromptInputLine(#{
      description: "Enter new name for tab",
      initial_value: "My Tab Name",
      action: onlyterm.action_callback(|window, pane, line| {
        // line will be `()` if they hit escape without entering anything
        // An empty string if they just hit enter
        // Or the actual line of text they wrote
        if line != () {
          window.active_tab().set_title(line);
        }
      }),
    }),
  },
]
```

```rhai
config.keys = [
  #{
    key: "N",
    mods: "CTRL|SHIFT",
    action: act.PromptInputLine(#{
      description: "Enter name for new workspace",
      action: onlyterm.action_callback(|window, pane, line| {
        if line != () {
          window.perform_action(
            act.SwitchToWorkspace(#{ name: line }),
            pane
          );
        }
      }),
    }),
  },
]
```

See also:
   * [InputSelector](InputSelector.md).
   * [Confirmation](Confirmation.md).
