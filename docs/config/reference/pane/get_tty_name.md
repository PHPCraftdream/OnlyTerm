# `pane.get_tty_name()`

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

{{since('20230408-112425-69ae8472')}}

Returns the tty device name, or `nil` if the name is unavailable.

* This information is only available for local panes.  Multiplexer panes do not report this information.  Similarly, if you are using eg: `ssh` to connect to a remote host, you won't be able to access the name of the remote process that is running.
* This information is only available on unix systems.  Windows systems do not have an equivalent concept.

This example sets the right status to show the tty name:

```lua
local onlyterm = require 'onlyterm'

onlyterm.on('update-status', function(window, pane)
  local tty = pane:get_tty_name()
  if tty then
    window:set_right_status(tty)
  else
    window:set_right_status ''
  end
end)

return {}
```


