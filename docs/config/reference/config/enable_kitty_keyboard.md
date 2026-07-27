---
tags:
  - keys
---
# `enable_kitty_keyboard = false`

{{since('20220624-141144-bd1b7c5d')}}

When set to `true`, wezterm will honor kitty keyboard protocol escape
sequences that modify the [keyboard encoding](../../key-encoding.md).

OnlyTerm has twice tried defaulting this to `true` (upstream wezterm
defaults to `false`) so that apps requesting the kitty keyboard
protocol at runtime - eg. to disambiguate `Ctrl+Enter`/`Shift+Enter`
from a plain `Enter` - would get it automatically. The first attempt
broke `Ctrl+C` entirely (every `Ctrl+<letter>` combo got CSI-u encoded
once an app enabled the protocol's disambiguate-escape-codes flag). A
targeted fix in `KeyEvent::encode_kitty` kept `Ctrl+C` and similar
combos on their legacy byte while still escape-encoding genuinely
colliding combos (`Ctrl+H`/`I`/`M`/`[` vs Backspace/Tab/Enter/Escape) -
this did fix `Ctrl+Enter`/`Shift+Enter`, but `Ctrl+C` still didn't work
afterward. The likely explanation: using the enhanced keyboard
protocol at all requires the app to put the terminal into raw mode,
which independently disables the OS/tty layer's automatic
SIGINT-on-`Ctrl+C`, and the app's own manual handling in that mode may
only recognize the CSI-u form rather than the legacy byte - this needs
live byte-level tracing to confirm rather than further guessing. Since
`Ctrl+C` must always work, this is back to `false` until the actual
root cause is confirmed and a real fix can be verified.


