# `tab.get_pane_direction(direction)`

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

{{since('20230320-124340-559cb7b0')}}

Returns pane adjacent to the active pane in *tab* in the direction *direction*.

Valid values for *direction* are:

* `"Left"`
* `"Right"`
* `"Up"`
* `"Down"`
* `"Prev"`
* `"Next"`

See [ActivatePaneDirection](../keyassignment/ActivatePaneDirection.md) for more information
about how panes are selected given direction.

