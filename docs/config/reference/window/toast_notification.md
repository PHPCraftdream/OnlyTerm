# `window.toast_notification(title, message,  [url, [timeout_milliseconds]])`

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

!!! warning "Pending rhai conversion"

    The code example(s) below still use Lua syntax from before OnlyTerm's
    config engine switched to rhai. The *option names, event names and
    object/method shapes* are unchanged -- only the scripting syntax differs.
    See the [migration guide](../../../migration-lua-to-rhai.md) for the Lua-to-rhai
    syntax mapping to translate this example yourself, or watch for a
    follow-up documentation pass that rewrites it directly.

```lua
local wezterm = require 'wezterm'

wezterm.on('window-config-reloaded', function(window, pane)
  window:toast_notification('wezterm', 'configuration reloaded!', nil, 4000)
end)

return {}
```
