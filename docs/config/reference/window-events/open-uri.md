# `open-uri`

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

The `open-uri` event is emitted when the `CompleteSelectionOrOpenLinkAtMouseCursor`
key/mouse assignment is triggered.

The default action is to open the active URI in your browser, but if you
register for this event you can co-opt the default behavior.

For example, if you prefer to launch your preferred MUA in a new window
in response to clicking on `mailto:` URLs, you could do something like:

```lua
local wezterm = require 'wezterm'

wezterm.on('open-uri', function(window, pane, uri)
  local start, match_end = uri:find 'mailto:'
  if start == 1 then
    local recipient = uri:sub(match_end + 1)
    window:perform_action(
      wezterm.action.SpawnCommandInNewWindow {
        args = { 'mutt', recipient },
      },
      pane
    )
    -- prevent the default action from opening in a browser
    return false
  end
  -- otherwise, by not specifying a return value, we allow later
  -- handlers and ultimately the default action to caused the
  -- URI to be opened in the browser
end)
```

The first event parameter is a [`window` object](../window/index.md) that
represents the gui window.

The second event parameter is a [`pane` object](../pane/index.md) that
represents the pane.

The third event parameter is the URI string.


