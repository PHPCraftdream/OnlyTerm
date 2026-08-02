# `tab.set_zoomed(bool)`

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

Sets the zoomed state for the active pane within this tab.

A zoomed pane takes up all available space in the tab, hiding all other panes
while it is zoomed. Switching its zoom state off will restore the prior split
arrangement.

Setting the zoom state to true zooms the pane if it wasn't already zoomed.
Setting the zoom state to false un-zooms the pane if it was zoomed.

Returns the prior zoom state.

See also: [`unzoom_on_switch_pane`](../config/unzoom_on_switch_pane.md),
[SetPaneZoomState](../keyassignment/SetPaneZoomState.md).
