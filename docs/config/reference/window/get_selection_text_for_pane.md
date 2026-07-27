# `window.get_selection_text_for_pane(pane)`

{{since('20210404-112810-b63a949d')}}

Returns the text that is currently selected within the specified pane within
the specified window.  This is the same text that would be copied to the
clipboard if the [CopyTo](../keyassignment/CopyTo.md) action were to be
performed.

Why isn't this simply a method of the `pane` object?  The reason is that the
selection is an attribute of the containing window, and a given pane can
potentially be mapped into multiple windows.

This example logs the current selection when a CTRL+SHIFT+E is pressed:

!!! warning "Pending rhai conversion"

    The code example(s) below still use Lua syntax from before OnlyTerm's
    config engine switched to rhai. The *option names, event names and
    object/method shapes* are unchanged -- only the scripting syntax differs.
    See the [migration guide](../../../migration-lua-to-rhai.md) for the Lua-to-rhai
    syntax mapping to translate this example yourself, or watch for a
    follow-up documentation pass that rewrites it directly.

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
