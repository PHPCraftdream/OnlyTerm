# TabInformation

!!! danger "Removed: no scripting engine"

    This page documents part of the rhai (and, before that, Lua) **scripting
    API**, which has been removed entirely. OnlyTerm's configuration format
    is now [ktav](../../migration-to-ktav.md), a static `key: value` data
    format with no expressions, function calls, or callbacks of any kind --
    there is nothing left in OnlyTerm that could call this function, invoke
    this method, or construct this object. The description and examples
    below are kept for historical reference (e.g. if you're migrating a very
    old config and trying to understand what it used to do), but none of it
    is callable today. See the [changelog](../../changelog.md#continuousnightly)
    for the full rationale.

The `TabInformation` struct describes a tab.  `TabInformation` is purely a
snapshot of some of the key characteristics of the tab, intended for use in
synchronous, fast, event callbacks that format GUI elements such as the window
and tab title bars.

The `TabInformation` struct contains the following fields:

* `tab_id` - the identifier for the tab
* `tab_index` - the logical tab position within its containing window, with 0 indicating the leftmost tab
* `is_active` - is true if this tab is the active tab
* `is_last_active` - is true if this tab is the previously active tab. {{since('nightly', inline=True)}}
* `active_pane` - the [PaneInformation](PaneInformation.md) for the active pane in this tab
* `panes` - the [PaneInformation](PaneInformation.md) for all panes in this tab {{since('20220319-142410-0fcdea07', inline=True)}}
* `window_id` - the ID of the window that contains this tab {{since('20220807-113146-c2fee766', inline=True)}}
* `window_title` - the title of the window that contains this tab {{since('20220807-113146-c2fee766', inline=True)}}
* `tab_title` - the title of the tab {{since('20220807-113146-c2fee766', inline=True)}}


