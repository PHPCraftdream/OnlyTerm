---
tags:
  - prompt
---

# `InputSelector`

{{since('20230408-112425-69ae8472')}}

!!! danger "Non-functional: required a scripting callback"

    `InputSelector`'s `action` field was designed to hold an event callback
    registered via `wezterm.action_callback(...)`, resolved through rhai's
    (and, before that, Lua's) event-handler registry. That registry no
    longer exists — see the [changelog](../../../changelog.md#continuousnightly)
    and the [migration guide](../../../migration-to-ktav.md). The overlay
    below still displays and lets you pick a choice, but the selection now
    goes nowhere: there is no handler left to receive it, so using this
    action currently has no observable effect beyond showing and closing a
    list. `action` still only accepts the internal `EmitEvent` shape (for
    backwards-compatible config loading); any config still written against
    the example below will keep loading, but nothing runs with the selected
    choice.

Activates an overlay to display a list of choices for the user
to select from.

`InputSelector` accepts the following fields:

* `title` - the title that will be set for the overlay pane
* `choices` - an array consisting of the potential choices. Each entry
  is itself an object with a `label` field and an optional `id` field.
  The label will be shown in the list, while the id can be a different
  string that is meaningful to your action.
* `action` - previously an event callback registered via
  `action_callback`, called with the selected `id`/`label` (or with both
  unset if cancelled). No longer connected to anything (see above).
* `fuzzy` - a boolean that defaults to `false`. If `true`, InputSelector will start
  in its fuzzy finding mode (this is equivalent to starting the InputSelector and
  pressing / in the default mode).

{{since('20240127-113634-bbcac864')}}

These additional fields are also available:

* `alphabet` - a string of unique characters. The characters in the string are used
  to calculate one or two click shortcuts that can be used to quickly choose from
  the InputSelector when in the default mode. Defaults to:
  `"1234567890abcdefghilmnopqrstuvwxyz"`. (Without j/k so they can be used for movement
  up and down.)
* `description` - a string to display when in the default mode. Defaults to:
  `"Select an item and press Enter = accept,  Esc = cancel,  / = filter"`.
* `fuzzy_description` - a string to display when in fuzzy finding mode. Defaults to:
  `"Fuzzy matching: "`.


### Key Assignments

The default key assignments in the InputSelector are as follows:

| Action  |  Key Assignment |
|---------|-------------------|
| Add to selection string until a match is found (if in the default mode) | Any key in `alphabet` {{since('20240127-113634-bbcac864', inline=True)}} |
| Select matching number (if in the default mode) | <kbd>1</kbd> to <kbd>9</kbd> {{since('20230408-112425-69ae8472', inline=True)}} |
| Start fuzzy search (if in the default mode) | <kbd>/</kbd> |
| Add to filtering string (if in fuzzy finding mode) | Any key not listed below |
| Remove from selection or filtering string | <kbd>Backspace</kbd> |
| Pick currently highlighted line | <kbd>Enter</kbd> |
|                                 | <kbd>LeftClick</kbd> (with mouse) |
| Move Down      | <kbd>DownArrow</kbd> |
|                | <kbd>Ctrl</kbd> + <kbd>N</kbd> |
|                | <kbd>Ctrl</kbd> + <kbd>J</kbd> {{since('20240127-113634-bbcac864', inline=True)}} |
|                | <kbd>j</kbd> (if not in `alphabet`) |
| Move Up        | <kbd>UpArrow</kbd>  |
|                | <kbd>Ctrl</kbd> + <kbd>P</kbd> |
|                | <kbd>Ctrl</kbd> + <kbd>K</kbd> {{since('20240127-113634-bbcac864', inline=True)}} |
|                | <kbd>k</kbd>  (if not in `alphabet`)   |
| Quit     | <kbd>Ctrl</kbd> + <kbd>G</kbd> |
|          | <kbd>Ctrl</kbd> + <kbd>C</kbd> {{since('20240127-113634-bbcac864', inline=True)}} |
|          | <kbd>Escape</kbd> |

Note: If the InputSelector is started with `fuzzy` set to `false`, then <kbd>Backspace</kbd> can go from fuzzy finding mode back to the default mode when pressed while the filtering string is empty.

## Historical example (no longer functional)

This is preserved for reference only; the callback here will not run in the
current version of OnlyTerm.

```rhai
config.keys = [
  #{
    key: "E",
    mods: "CTRL|SHIFT",
    action: act.InputSelector(#{
      action: wezterm.action_callback(|window, pane, id, label| {
        if id == () && label == () {
          wezterm.log_info("cancelled");
        } else {
          wezterm.log_info("you selected " + id + label);
          pane.send_text(id);
        }
      }),
      title: "I am title",
      choices: [
        #{ label: "No thanks", id: "Regretfully, I decline this offer." },
        #{ label: "WTF?", id: "An interesting idea, but I have some questions about it." },
        #{ label: "LGTM", id: "This sounds like the right choice" },
      ],
    }),
  },
]
```

See also:
   * [PromptInputLine](PromptInputLine.md).
   * [Confirmation](Confirmation.md).
