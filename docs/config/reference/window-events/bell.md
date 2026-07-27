# `bell`

{{since('20211204-082213-a66c61ee9')}}

The `bell` event is emitted when the ASCII BEL sequence is emitted to
a pane in the window.

Defining an event handler doesn't alter wezterm's handling of the bell;
the event supplements it and allows you to take additional action over
the configured behavior.

The first event parameter is a [`window` object](../window/index.md) that
represents the gui window.

The second event parameter is a [`pane` object](../pane/index.md) that
represents the pane in which the bell was rung, which may not be active
pane--it could be in an unfocused pane or tab..

!!! warning "Pending rhai conversion"

    The code example(s) below still use Lua syntax from before OnlyTerm's
    config engine switched to rhai. The *option names, event names and
    object/method shapes* are unchanged -- only the scripting syntax differs.
    See the [migration guide](../../../migration-lua-to-rhai.md) for the Lua-to-rhai
    syntax mapping to translate this example yourself, or watch for a
    follow-up documentation pass that rewrites it directly.

```lua
local wezterm = require 'wezterm'

wezterm.on('bell', function(window, pane)
  wezterm.log_info('the bell was rung in pane ' .. pane:pane_id() .. '!')
end)

return {}
```

See also [audible_bell](../config/audible_bell.md) and [visual_bell](../config/visual_bell.md).
