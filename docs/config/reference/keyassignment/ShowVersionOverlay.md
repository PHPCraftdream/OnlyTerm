# `ShowVersionOverlay`

Overlays the current tab with a centered box showing the running OnlyTerm
version, commit number/hash, and build time. Press `c` inside the overlay
to copy that information to the clipboard, or `Esc` to close it.

```
keys: [
  ## CTRL-I shows the version overlay
  { key: I, mods: CTRL, action: ShowVersionOverlay }
]
```
