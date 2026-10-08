---
tags:
  - tab
---
# `RenameCurrentTab`

Opens a graphical **Rename Tab** dialog in the same style as the F3/F4 menus.
The current custom title is pre-filled and selected, so typing replaces it.
Enter or **Rename** applies the new title; Escape, F2, or **Cancel** closes the
dialog without changing it. Clearing the field entirely and applying resets
the tab to its normal automatic title.

Up/Down and Tab/Shift+Tab move between the input and buttons. Left/Right moves
the caret in the input or selects a neighboring button. Ctrl+A selects the
whole title, and clipboard paste is supported. Buttons also work with the mouse.

This is bound to `F2` by default (matching Windows Explorer's rename
convention). Clicking or double-clicking a tab only selects it; neither
opens the rename dialog.

```
keys: [
  { key: F2, mods: NONE, action: RenameCurrentTab }
]
```

See also: [onlyterm cli set-tab-title](../../../cli/cli/set-tab-title.md),
which renames a tab non-interactively (useful from scripts).
