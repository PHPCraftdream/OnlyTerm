# `pane.get_text_from_region(start_x, start_y, end_x, end_y)`

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

Returns the text from the specified region.

* `start_x` and `end_x` are the starting and ending cell column, where 0 is the
  left-most cell
* `start_y` and `end_y` are the starting and ending row, expressed as a stable
  row index.  Use [pane:get_dimensions()](get_dimensions.md) to retrieve the
  currently valid stable index values for the top of scrollback and top of
  viewport.

The text within the region is unwrapped to its logical line representation,
rather than the wrapped-to-physical-display-width.

