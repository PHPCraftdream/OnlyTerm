# `tab.panes_with_info()`

!!! danger "Removed: no scripting engine"

    This page documents part of the rhai (and, before that, Lua) **scripting
    API**, which has been removed entirely. OnlyTerm's configuration format
    is now [ktav](../../../migration-to-ktav.md), a static `key: value` data
    format with no expressions, function calls, or callbacks of any kind --
    there is nothing left in OnlyTerm that could call this function, invoke
    this method, or construct this object. The description and examples
    below are kept for historical reference (e.g. if you're migrating a very
    old config and trying to understand what it used to do), but none of it
    is callable today. See the [changelog](../../../changelog.md#continuousnightly)
    for the full rationale.

{{since('20220807-113146-c2fee766')}}

Returns an array table containing an extended info entry for each of the panes
contained by this tab.

Each element is a rhai map with the following fields:

* `index` - the topological pane index
* `is_active` - a boolean indicating whether this is the active pane within the tab
* `is_zoomed` - a boolean indicating whether this pane is zoomed
* `left` - The offset from the top left corner of the containing tab to the top left corner of this pane, in cells.
* `top` - The offset from the top left corner of the containing tab to the top left corner of this pane, in cells.
* `width` - The width of this pane in cells
* `height` - The height of this pane in cells
* `pixel_width` - The width of this pane in pixels
* `pixel_height` - The height of this pane in pixels
* `pane` - The [Pane](../pane/index.md) object
