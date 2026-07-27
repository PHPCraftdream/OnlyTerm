# `CompleteSelection`

Completes an active text selection process; the selection range is
marked closed and then the selected text is copied as though the
`Copy` action was executed.

{{since('20210203-095643-70a364eb')}}

`CompleteSelection` now requires a destination parameter to specify
which clipboard buffer the selection will populate; the copy action
is now equivalent to [CopyTo](CopyTo.md).

!!! warning "Pending rhai conversion"

    The code example(s) below still use Lua syntax from before OnlyTerm's
    config engine switched to rhai. The *option names, event names and
    object/method shapes* are unchanged -- only the scripting syntax differs.
    See the [migration guide](../../../migration-lua-to-rhai.md) for the Lua-to-rhai
    syntax mapping to translate this example yourself, or watch for a
    follow-up documentation pass that rewrites it directly.

```lua
config.mouse_bindings = {
  -- Change the default click behavior so that it only selects
  -- text and doesn't open hyperlinks, and that it populates
  -- the Clipboard rather the PrimarySelection which is part
  -- of the default assignment for a left mouse click.
  {
    event = { Up = { streak = 1, button = 'Left' } },
    mods = 'NONE',
    action = wezterm.action.CompleteSelection 'Clipboard',
  },
}
```
