# `new-tab-button-click`

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

{{since('20230326-111934-3666303c')}}

The `new-tab-button-click` event is emitted when the user clicks on the
"new tab" button in the tab bar. This is the `+` button that is drawn
to the right of the last tab.

The first event parameter is a [`window` object](../window/index.md) that
represents the gui window.

The second event parameter is a [`pane` object](../pane/index.md) that
represents the active pane in the window.

The third event parameter is an indication of which mouse button was clicked.
The following values are possible:

* `"Left"` - the left mouse button
* `"Right"` - the right mouse button
* `"Middle"` - the middle mouse button

The last event parameter is a [KeyAssignment](../keyassignment/index.md) which
encodes the default, built-in action that onlyterm will take.  It may be `nil`
in the case where onlyterm would not take any action.

You may take any action you wish in this event handler.

If you return `false` then you will prevent onlyterm from carrying out its
default action.

Otherwise, onlyterm will proceed to perform that action once your event
handler returns.

This following two examples are equivalent in functionality:

```lua
onlyterm.on(
  'new-tab-button-click',
  function(window, pane, button, default_action)
    -- just log the default action and allow onlyterm to perform it
    onlyterm.log_info('new-tab', window, pane, button, default_action)
  end
)
```

```lua
onlyterm.on(
  'new-tab-button-click',
  function(window, pane, button, default_action)
    onlyterm.log_info('new-tab', window, pane, button, default_action)
    -- We're explicitly going to perform the default action
    if default_action then
      window:perform_action(default_action, pane)
    end
    -- and tell onlyterm that we handled the event so that it doesn't
    -- perform it a second time.
    return false
  end
)
```

See also [window:perform_action()](../window/perform_action.md).
