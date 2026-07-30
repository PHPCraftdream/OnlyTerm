# `pane.is_unresponsive()`

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
