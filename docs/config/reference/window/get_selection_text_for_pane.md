# `window.get_selection_text_for_pane(pane)`

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

{{since('20210404-112810-b63a949d')}}

Returns the text that is currently selected within the specified pane within
the specified window.  This is the same text that would be copied to the
clipboard if the [CopyTo](../keyassignment/CopyTo.md) action were to be
performed.

Why isn't this simply a method of the `pane` object?  The reason is that the
selection is an attribute of the containing window, and a given pane can
potentially be mapped into multiple windows.

This example logs the current selection when a CTRL+SHIFT+E is pressed:

```lua
local wezterm = require 'wezterm'

wezterm.on('log-selection', function(window, pane)
  local sel = window:get_selection_text_for_pane(pane)
  wezterm.log_info('selection is: ' .. sel)
end)

return {
  keys = {
    {
      key = 'E',
      mods = 'CTRL',
      action = wezterm.action.EmitEvent 'log-selection',
    },
  },
}
```
