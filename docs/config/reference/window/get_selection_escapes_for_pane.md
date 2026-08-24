# `window.get_selection_escapes_for_pane(pane)`

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

Returns the text that is currently selected within the specified pane within
the specified window formatted with the escape sequences necessary to reproduce
the same colors and styling .

This is the same text that
[window:get_selection_text_for_pane()](get_selection_text_for_pane.md) would
return, except that it includes escape sequences.

This example copies the current selection + escapes to the clipboard when
`CTRL+SHIFT+E` is pressed:

```lua
local onlyterm = require 'onlyterm'

return {
  keys = {
    {
      key = 'E',
      mods = 'CTRL',
      action = onlyterm.action_callback(function(window, pane)
        local ansi = window:get_selection_escapes_for_pane(pane)
        window:copy_to_clipboard(ansi)
      end),
    },
  },
}
```

See also: [window:copy_to_clipboard()](copy_to_clipboard.md).
