---
tags:
  - prompt
---

# `PromptInputLine`

{{since('20230408-112425-69ae8472')}}

!!! warning "Legacy scripting callbacks remain unavailable"

    The native `RenameCurrentTab` action applies the accepted text as the
    captured tab's custom title. Legacy `EmitEvent` callback shapes still
    load, but the Lua/rhai handler registry has been removed, so no script
    receives the result. Other action shapes remain unsupported.

Opens a graphical value-entry dialog in the same style as F2, with a
description, editable input, and Cancel/Accept controls.

`PromptInputLine` accepts four fields:

* `description` - the text to show at the top of the display area. You may
  embed escape sequences.
* `action` - `RenameCurrentTab` for native tab renaming, or a legacy
  `EmitEvent` callback shape whose result is discarded.
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
