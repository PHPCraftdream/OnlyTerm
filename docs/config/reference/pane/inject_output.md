# `pane.inject_output(text)`

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

Sends text, which may include escape sequences, to the output side of the
current pane.  The text will be evaluated by the terminal emulator and can thus
be used to inject/force the terminal to process escape sequences that adjust
the current mode, as well as sending human readable output to the terminal.

Note that if you move the cursor position as a result of using this method, you
should expect the display to change and for text UI programs to get confused.

In this contrived and useless example, pressing ALT-k will output `hello there`
in italics to the current pane:

```lua
local onlyterm = require 'onlyterm'

return {
  keys = {
    {
      key = 'k',
      mods = 'ALT',
      action = onlyterm.action_callback(function(window, pane)
        pane:inject_output '\r\n\x1b[3mhello there\r\n'
      end),
    },
  },
}
```

Not all panes support this method; at the time of writing, this works for local
panes but not for multiplexer panes.

