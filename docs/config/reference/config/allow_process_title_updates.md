---
tags:
  - tab_bar
---
# `allow_process_title_updates = false`

Controls whether processes may change tab and window titles through OSC 0/1/2
terminal escape sequences or the `onlyterm cli set-tab-title` and
`set-window-title` commands.

The default is `false`. Process-provided titles are ignored, so shell prompts
and full-screen applications cannot replace an explicit config/UI title or
cause repeated title updates and tab-bar redraws.

Set the root-level option to `true` to restore process-controlled titles:

```
allow_process_title_updates: true
```

This option does not affect titles assigned by `default_tab_title`,
`SpawnCommand.title`, `--start-conf`, or the F2/double-click rename UI.
