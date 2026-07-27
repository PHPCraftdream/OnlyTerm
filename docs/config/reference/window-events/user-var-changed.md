# `user-var-changed`

{{since('20220903-194523-3bb1ed61')}}

The `user-var-changed` event is emitted when a *user var* escape sequence is
used to set a user var.

You can use something like the following from your shell:

```bash
printf "\033]1337;SetUserVar=%s=%s\007" foo `echo -n bar | base64`
```

to set the user var named `foo` to the value `bar`.

!!! note
    On some systems the `base64` command wraps the output by default after some
    amount of characters limiting the maximum length of the value. If this is
    the case an argument like `-w 0` might help to avoid wrapping.

Then, if you have this in your config:

!!! warning "Pending rhai conversion"

    The code example(s) below still use Lua syntax from before OnlyTerm's
    config engine switched to rhai. The *option names, event names and
    object/method shapes* are unchanged -- only the scripting syntax differs.
    See the [migration guide](../../../migration-lua-to-rhai.md) for the Lua-to-rhai
    syntax mapping to translate this example yourself, or watch for a
    follow-up documentation pass that rewrites it directly.

```lua
local wezterm = require 'wezterm'

wezterm.on('user-var-changed', function(window, pane, name, value)
  wezterm.log_info('var', name, value)
end)

return {}
```

your event handler will be called with `name = 'foo'` and `value = 'bar'`.

See also [pane:get_user_vars()](../pane/get_user_vars.md).
