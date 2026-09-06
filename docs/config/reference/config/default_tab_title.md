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
4. When `allow_process_title_updates` is false (the default), the basename of the pane's current working directory, as observed
   directly from the operating system. This is never taken from the
   shell's own OSC 7/0/2 escape sequences, so nothing running inside the
   pane can spoof or flicker the title by printing one. If cwd is unavailable,
   the process-derived title is used as a fallback.

When `allow_process_title_updates` is true, the fourth choice is instead the
process-derived title (including accepted OSC titles), even before a custom OSC
title arrives. Explicit UI and configured titles above still take precedence.
The option also controls the CLI `set-tab-title` and `set-window-title` commands:
they can be invoked by scripts and are intentionally disabled by default. Their
check uses the CLI process's loaded configuration; this is a convenience policy,
not an authorization boundary on the mux protocol.

Process-provided OSC titles are ignored by default. They can be restored with
[`allow_process_title_updates`](allow_process_title_updates.md), although this
also allows shells and TUI applications to trigger frequent title refreshes.
