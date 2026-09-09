---
tags:
  - tab_bar
---
# `show_process_tree_stats_in_title = false`

Shows live CPU/memory usage, refreshed every 5 seconds. With the standard
Windows title-bar decorations, the block is drawn separately and centered
relative to the whole window, not positioned using spaces:

```
MyTab                         [CPU 4% - RAM 1.3 GB - 2%]
```

The caption font follows the window's DPI, with a minimum size of 12pt.
System caption buttons retain their own reserved area. On narrow windows,
the title is ellipsized and the block may use `[CPU 4% / 1.3G / 2%]`.
If even that does not fit, the block is hidden rather than partially clipped.
The center is clamped only when needed to avoid the icon and system buttons.

The OS title string (used by accessibility tools and window switchers) retains
a concise `MyTab — [CPU ...]` form. It does not contain layout-padding spaces.
If DWM or the caption drawing resources are unavailable, the ordinary native
title is used. Decorations that already omit the native title bar are unchanged.

The numbers describe OnlyTerm's own process tree (this `onlyterm-gui.exe`
process plus its descendants -- per-tab GPU host processes, ConPTY-hosted
shells, and anything they in turn spawn):

- `CPU` -- percentage of the whole machine's CPU capacity consumed by the
  tree since the previous 5-second sample (matches Task Manager's per-process
  convention: a single core pegged at 100% on a 4-core machine shows as 25%).
- `RAM ... GB` -- combined working-set memory of the tree, in gigabytes.
- The trailing percentage -- that same memory figure as a percentage of the
  machine's total installed RAM.

The default is `true`. Set to `false` to restore the ordinary native caption
and leave its title exactly as computed from the tab/pane title logic:

```
show_process_tree_stats_in_title: false
```

This option does not affect the tab-bar's own tab titles.

For caption geometry diagnostics, launch with
`ONLYTERM_LOG=window::os::windows::window::caption=debug`. The per-PID log
records HWND/DWM bounds and frame calculations without logging the title text.
