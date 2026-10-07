---
tags:
  - tab_bar
---
# `use_fancy_tab_bar = true`

{{since('20220101-133340-7edc5b5a')}}

When set to `true` (the default), the tab bar is rendered in
a native style with proportional fonts.

Each fancy tab has a minimum outer width of 100 pixels. Tabs do not shrink below
that width when the window narrows; overflow extends past the visible tab strip.
The Windows tab font defaults to 11 points and can be overridden with
[`window_frame.font_size`](window_frame.md).

Right-clicking a tab has no action. The new-tab button retains its tab creation
menu.

When set to `false`, the tab bar is rendered using a retro
aesthetic using the main terminal font.

