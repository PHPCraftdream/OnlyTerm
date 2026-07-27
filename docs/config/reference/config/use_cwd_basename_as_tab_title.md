---
tags:
  - tab_bar
---
# `use_cwd_basename_as_tab_title = false`

{{since('nightly')}}

When set to `true`, the default tab title (used when the tab doesn't have an
explicitly assigned title and no
[format-tab-title](../window-events/format-tab-title.md) event
handler is registered) is derived from the last path component (basename) of
the active pane's current working directory, instead of from the pane's title
(which is usually the name of the running foreground process).

For example, if the active pane's current working directory is
`/home/user/my-project`, the tab title will be `my-project` rather than
something like `zsh` or `nvim`.

The default is `false`, which preserves the historical behavior of deriving
the tab title from the pane title.

An explicitly assigned tab title (for example, one set via
`wezterm cli set-tab-title` or via a `format-tab-title` handler) always takes
priority over this setting. If the active pane hasn't reported a current
working directory yet, the pane title is used as a fallback.

See also: [show_tab_index_in_tab_bar](show_tab_index_in_tab_bar.md)
