---
tags:
  - tab_bar
---
# `show_process_tree_stats_in_title = false`

Appends a live CPU/memory usage suffix to the OS window title, refreshed every
5 seconds:

```
MyTab — bash  —  [CPU 4% - RAM 1.3 GB - 2%]
```

The numbers describe OnlyTerm's own process tree (this `onlyterm-gui.exe`
process plus its descendants -- per-tab GPU host processes, ConPTY-hosted
shells, and anything they in turn spawn):

- `CPU` -- percentage of the whole machine's CPU capacity consumed by the
  tree since the previous 5-second sample (matches Task Manager's per-process
  convention: a single core pegged at 100% on a 4-core machine shows as 25%).
- `RAM ... GB` -- combined working-set memory of the tree, in gigabytes.
- The trailing percentage -- that same memory figure as a percentage of the
  machine's total installed RAM.

The default is `true`. Set to `false` to disable the suffix and leave the
window title exactly as computed from the tab/pane title logic:

```
show_process_tree_stats_in_title: false
```

This option does not affect the tab-bar's own tab titles -- only the OS
window's titlebar text.
