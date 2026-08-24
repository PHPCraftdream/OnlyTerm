# `pane.move_to_new_window([WORKSPACE])`

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

{{since('20230326-111934-3666303c')}}

Creates a window and moves `pane` into that window.

The *WORKSPACE* parameter is optional; if specified, it will be used
as the name of the workspace that should be associated with the new
window.  Otherwise, the current active workspace will be used.

Returns the newly created [MuxTab](../MuxTab/index.md) object, and the
newly created [MuxWindow](../mux-window/index.md) object.

```lua
config.keys = {
  {
    key = '!',
    mods = 'LEADER | SHIFT',
    action = onlyterm.action_callback(function(win, pane)
      local tab, window = pane:move_to_new_window()
    end),
  },
}
```

See also [pane:move_to_new_window()](move_to_new_window.md),
[onlyterm cli move-pane-to-new-tab](../../../cli/cli/move-pane-to-new-tab.md).


