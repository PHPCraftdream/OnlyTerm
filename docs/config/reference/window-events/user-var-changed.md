# `user-var-changed`

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

```lua
local onlyterm = require 'onlyterm'

onlyterm.on('user-var-changed', function(window, pane, name, value)
  onlyterm.log_info('var', name, value)
end)

return {}
```

your event handler will be called with `name = 'foo'` and `value = 'bar'`.

See also [pane:get_user_vars()](../pane/get_user_vars.md).
