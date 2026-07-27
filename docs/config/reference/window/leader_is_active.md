# window.leader_is_active()

{{since('20220319-142410-0fcdea07')}}

Returns `true` if the [Leader Key](../../keys.md) is active in the window, or false otherwise.

This example shows `LEADER` in the right status area, and turns the cursor orange,
when the leader is active:

!!! warning "Pending rhai conversion"

    The code example(s) below still use Lua syntax from before OnlyTerm's
    config engine switched to rhai. The *option names, event names and
    object/method shapes* are unchanged -- only the scripting syntax differs.
    See the [migration guide](../../../migration-lua-to-rhai.md) for the Lua-to-rhai
    syntax mapping to translate this example yourself, or watch for a
    follow-up documentation pass that rewrites it directly.

```lua
local wezterm = require 'wezterm'

wezterm.on('update-right-status', function(window, pane)
  local leader = ''
  if window:leader_is_active() then
    leader = 'LEADER'
  end
  window:set_right_status(leader)
end)

return {
  leader = { key = 'a', mods = 'CTRL' },
  colors = {
    compose_cursor = 'orange',
  },
}
```

See also: [window:composition_status()](composition_status.md).
