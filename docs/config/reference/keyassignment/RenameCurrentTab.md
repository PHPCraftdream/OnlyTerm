---
tags:
  - tab
---
# `RenameCurrentTab`

Prompts for a new title for the active tab, pre-filled with its current
title, and applies it once you press Enter (Escape cancels without
changing anything). Clearing the field entirely and pressing Enter resets
the tab back to its normal automatic title (the same as never having set a
custom one).

This is bound to `F2` by default (matching Windows Explorer's rename
convention), and the tab bar can also be double-clicked to trigger the
same prompt -- there is no context menu involved.

```
keys: [
  { key: F2, mods: NONE, action: RenameCurrentTab }
]
```

See also: [wezterm cli set-tab-title](../../../cli/cli/set-tab-title.md),
which renames a tab non-interactively (useful from scripts).
