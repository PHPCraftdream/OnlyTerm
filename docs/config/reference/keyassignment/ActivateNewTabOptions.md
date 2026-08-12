# `ActivateNewTabOptions`

Shows a modal dialog with three option groups for configuring a new tab:

- **Shell**: Choose between `cmd`, `bash`, `powershell`, or `wsl` (default: `cmd`)
- **Elevation**: `normal` or `admin` (default: `normal`)
- **Priority**: Select from the Windows process priority classes: `Idle`, `Below Normal`, `Normal`, `Above Normal`, `High`, or `Realtime` (default: `Normal`)

The dialog is fully interactive via both mouse and keyboard:

- **Mouse**: Click any radio option to select it; click the **Run** button to confirm your choices (currently logs the selected values for the upcoming spawn task).
- **Keyboard**:
  - `Tab` / `Shift+Tab`: Move focus through all radio options and the Run button.
  - `Space` / `Enter`: Select/activate the currently focused item.
  - `Escape`: Close the dialog without taking any action.

```
keys: [
  ## CTRL+SHIFT+N opens the New Tab Options dialog
  { key: N, mods: CTRL|SHIFT, action: ActivateNewTabOptions }
]
```