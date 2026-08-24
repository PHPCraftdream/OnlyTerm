---
tags:
  - tab_bar
---
# `show_new_tab_button_in_tab_bar = true`

{{since('20221119-145034-49b9839f')}}

When set to `true` (the default), the tab bar will display the new-tab button,
which can be left-clicked to create a new tab, or right-clicked to display the
[Launcher Menu](../../launch.md).

When set to `false`, the new-tab button will not be drawn into the tab bar.

This example turns off the tabs and new-tab button, leaving just the left and
right status areas:

```
use_fancy_tab_bar: false
show_tabs_in_tab_bar: false
show_new_tab_button_in_tab_bar: false
```

!!! note "`update-right-status` no longer exists"

    An earlier version of this example also registered a
    `onlyterm.on('update-right-status', ...)` handler to populate the left
    and right status areas with static placeholder text (`"left"`/`"right"`).
    That event hook has been removed along with the rest of the scripting
    engine — see the [changelog](../../../changelog.md#continuousnightly) —
    so there is currently no way to set the status bar text from config;
    only the tab-bar visibility options above still work.

