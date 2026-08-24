# `augment-command-palette`

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

{{since('20230712-072601-f4abf8fd')}}

This event is emitted when the [Command Palette](../keyassignment/ActivateCommandPalette.md) is shown.

Its purpose is to enable you to add additional entries to the list of commands
shown in the palette.

This hook is synchronous; calling asynchronous functions will not succeed.

The return value is a table listing the additional entries.  Each element of the
returned table may have the following fields:

* `brief` - required: the brief description for the entry
* `doc` - optional: a long description that may be shown after the entry, or that
  may be used in future versions of onlyterm to provide more information about the
  command.
* `action` - the action to take when the item is activated. Can be any key assignment
  action.
* `icon` - optional Nerd Fonts glyph name to use for the icon for the entry. See
  [onlyterm.nerdfonts](../onlyterm/nerdfonts.md) for a list of icon names.

## Adding a Rename Tab entry to the palette

In this example, an entry is added for renaming tabs:

```lua
local onlyterm = require 'onlyterm'
local act = onlyterm.action

local config = onlyterm.config_builder()

onlyterm.on('augment-command-palette', function(window, pane)
  return {
    {
      brief = 'Rename tab',
      icon = 'md_rename_box',

      action = act.PromptInputLine {
        description = 'Enter new name for tab',
        initial_value = 'My Tab Name',
        action = onlyterm.action_callback(function(window, pane, line)
          if line then
            window:active_tab():set_title(line)
          end
        end),
      },
    },
  }
end)

return config
```
