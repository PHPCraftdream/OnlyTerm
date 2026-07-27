---
tags:
  - keys
---
# `enable_kitty_keyboard = true`

{{since('20220624-141144-bd1b7c5d')}}

When set to `true`, wezterm will honor kitty keyboard protocol escape
sequences that modify the [keyboard encoding](../../key-encoding.md).

OnlyTerm defaults this to `true` (upstream wezterm defaults to `false`)
so that apps which request the kitty keyboard protocol at runtime - for
example to disambiguate `Ctrl+Enter`/`Shift+Enter` from a plain `Enter`,
which legacy terminal encoding cannot represent at all - get it without
requiring the user to change this setting first. Set it to `false` if
you'd like to opt back out of runtime kitty-protocol negotiation.


