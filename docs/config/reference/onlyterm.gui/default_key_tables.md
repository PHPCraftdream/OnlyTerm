# `onlyterm.gui.default_key_tables()`

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

{{since('20221119-145034-49b9839f')}}

Returns a table holding the effective default set of `key_tables`.  That is the
set of keys that is used as a base if there was no configuration file.

This is useful in cases where you want to override a key table assignment
without replacing the entire set of key tables.

This example shows how to add a key assignment for `Backspace` to `copy_mode`,
without having to manually specify the entire key table:

```lua
local onlyterm = require 'onlyterm'
local act = onlyterm.action

local copy_mode = nil
if onlyterm.gui then
  copy_mode = onlyterm.gui.default_key_tables().copy_mode
  table.insert(
    copy_mode,
    { key = 'Backspace', mods = 'NONE', action = act.CopyMode 'MoveLeft' }
  )
end

return {
  key_tables = {
    copy_mode = copy_mode,
  },
}
```
