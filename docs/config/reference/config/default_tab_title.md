---
tags:
  - tab_bar
---
# `default_tab_title = "some title"`

{{since('nightly')}}

Fallback tab title applied to every newly spawned tab that hasn't been
given an explicit title some other way.

OnlyTerm always shows one of, in priority order:

1. An explicit rename from the tab-bar UI. This live user action always wins.
2. [`SpawnCommand.title`](../SpawnCommand.md), if the launch that
   created the tab specified one -- a per-launch override, for example on a
   keybinding or a `onlyterm cli spawn` invocation.
3. `default_tab_title`, if set.
4. The basename of the pane's current working directory, as observed
   directly from the operating system. This is never taken from the
   shell's own OSC 7/0/2 escape sequences, so nothing running inside the
   pane can spoof or flicker the title by printing one.

Process-provided OSC titles are ignored by default. They can be restored with
[`allow_process_title_updates`](allow_process_title_updates.md), although this
also allows shells and TUI applications to trigger frequent title refreshes.
