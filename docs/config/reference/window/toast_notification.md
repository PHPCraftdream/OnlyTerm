# `window.toast_notification(title, message,  [url, [timeout_milliseconds]])`

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

{{since('20210502-154244-3f7122cb')}}

Generates a desktop "toast notification" with the specified *title* and *message*.

An optional *url* parameter can be provided; clicking on the notification will
open that URL.

An optional *timeout* parameter can be provided; if so, it specifies how long
the notification will remain prominently displayed in milliseconds.  To specify
a timeout without specifying a url, set the url parameter to `nil`.  The timeout
you specify may not be respected by the system, particularly in X11/Wayland
environments, and Windows will always use a fixed, unspecified, duration.

The notification will persist on screen until dismissed or clicked, or until its
timeout duration elapses.

This example will display a notification whenever a window has its configuration
reloaded.  The notification should remain on-screen for approximately 4 seconds
(4000 milliseconds), but may remain longer depending on the system.

It's not an ideal implementation because there may be multiple windows and thus
multiple notifications:

```lua
local wezterm = require 'wezterm'

wezterm.on('window-config-reloaded', function(window, pane)
  window:toast_notification('wezterm', 'configuration reloaded!', nil, 4000)
end)

return {}
```
