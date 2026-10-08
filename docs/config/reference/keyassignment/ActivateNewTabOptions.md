# `ActivateNewTabOptions`

Shows a modal dialog with three option groups for configuring a new tab:

- **Shell**: Choose between `cmd`, `bash`, `powershell`, or `wsl` (default: `cmd`)
- **Elevation**: `normal` or `admin` (default: `normal`)
- **Priority**: Select from the Windows process priority classes: `Idle`, `Below Normal`, `Normal`, `Above Normal`, `High`, or `Realtime` (default: `Normal`)

The dialog is fully interactive via both mouse and keyboard:

- **Mouse**: Click a radio option to select it; click **Run** to create a tab with the selected shell, elevation and process priority.
- **Keyboard**:
  - `Tab` / `Shift+Tab`: Move focus through all radio options and the Run button.
  - `UpArrow` / `LeftArrow`: Move focus to the previous item.
  - `DownArrow` / `RightArrow`: Move focus to the next item.
  - `Space` / `Enter`: Select/activate the currently focused item.
  - `Escape`: Close the dialog without taking any action.

```
keys: [
  ## CTRL+SHIFT+N opens the New Tab Options dialog
  { key: N, mods: CTRL|SHIFT, action: ActivateNewTabOptions }
]
```