# `tab.get_size()`

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

Returns the overall size of the tab, taking into account all of the contained
panes.

The return value is a rhai map with the following fields:

* `rows` - the number of rows (height)
* `cols` - the number of columns (width)
* `pixel_width` - the total width, measured in pixels
* `pixel_height` - the total height, measured in pixels
* `dpi` - the resolution of the tab.

Note that `pixel_width`, `pixel_height` and `dpi` may be inaccurate when there
is no GUI client associated with the tab.


