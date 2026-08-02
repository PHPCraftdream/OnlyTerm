---
tags:
  - exit_behavior
---
# `window_close_confirmation`

Whether to display a confirmation prompt when the window is closed by the
windowing environment, either because the user closed it with the window
decorations, or instructed their window manager to close it.

OnlyTerm defaults this to `"NeverPrompt"` (upstream wezterm defaults to
`"AlwaysPrompt"`). Set it to `"AlwaysPrompt"` if you'd like the
confirmation prompt back.

```
window_close_confirmation: NeverPrompt
```

See also
[skip_close_confirmation_for_processes_named](../config/skip_close_confirmation_for_processes_named.md).

Note that this `window_close_confirmation` option doesn't apply to the default
`CTRL-SHIFT-W` or `CMD-w` key assignments; if you want to change prompts for
those, you will need to override the key shortcut as shown in the
[CloseCurrentTab](../keyassignment/CloseCurrentTab.md) documentation.
