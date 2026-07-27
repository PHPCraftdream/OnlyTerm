# `pane.inject_output(text)`

{{since('20221119-145034-49b9839f')}}

Sends text, which may include escape sequences, to the output side of the
current pane.  The text will be evaluated by the terminal emulator and can thus
be used to inject/force the terminal to process escape sequences that adjust
the current mode, as well as sending human readable output to the terminal.

Note that if you move the cursor position as a result of using this method, you
should expect the display to change and for text UI programs to get confused.

In this contrived and useless example, pressing ALT-k will output `hello there`
in italics to the current pane:

!!! warning "Pending rhai conversion"

    The code example(s) below still use Lua syntax from before OnlyTerm's
    config engine switched to rhai. The *option names, event names and
    object/method shapes* are unchanged -- only the scripting syntax differs.
    See the [migration guide](../../../migration-lua-to-rhai.md) for the Lua-to-rhai
    syntax mapping to translate this example yourself, or watch for a
    follow-up documentation pass that rewrites it directly.

```lua
local wezterm = require 'wezterm'

return {
  keys = {
    {
      key = 'k',
      mods = 'ALT',
      action = wezterm.action_callback(function(window, pane)
        pane:inject_output '\r\n\x1b[3mhello there\r\n'
      end),
    },
  },
}
```

Not all panes support this method; at the time of writing, this works for local
panes but not for multiplexer panes.

