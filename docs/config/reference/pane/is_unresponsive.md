# `pane.is_unresponsive()`

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

{{since('nightly')}}

Returns true if the most recent attempt to read title/progress/user-vars/
current-working-directory information from this pane gave up waiting for
the pane's terminal state and served stale, previously-cached data
instead, rather than blocking.

This is a bounded, best-effort signal, not a guarantee that the pane's
process is unresponsive or stuck: a single slow read is enough to set it,
and it clears itself again as soon as a subsequent read succeeds. It is
intended to let a `format-tab-title`/`format-window-title` handler flag a
pane that *may* currently be wedged, the same way
[has_unseen_output](has_unseen_output.md) flags a pane with unread
output -- see
[PaneInformation.is_unresponsive](../PaneInformation.md) for an example.
