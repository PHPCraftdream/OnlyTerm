# window.copy_to_clipboard(text \[,clipboard\])

{{since('20220807-113146-c2fee766')}}

Puts `text` into the specified `clipboard`.

Clipboard can be one of:

* `"Clipboard"` - the system clipboard
* `"PrimarySelection"` - the primary selection buffer (applicable to X11 and some Wayland systems only)
* `"ClipboardAndPrimarySelection"` - both the system clipboard and the primary selection.  This is the default if you don't specify the clipboard.

Note that updating the clipboard is asynchronous; this method will return
immediately while the clipboard is updated a few moments later in another
thread. If you need to ensure that the published text is visible to other
applications before you trigger some other action in your config then you may
need to add a short sleep to allow for that to complete.

!!! warning "Pending rhai conversion"

    The code example(s) below still use Lua syntax from before OnlyTerm's
    config engine switched to rhai. The *option names, event names and
    object/method shapes* are unchanged -- only the scripting syntax differs.
    See the [migration guide](../../../migration-lua-to-rhai.md) for the Lua-to-rhai
    syntax mapping to translate this example yourself, or watch for a
    follow-up documentation pass that rewrites it directly.

```lua
window:copy_to_clipboard 'put this text in the clipboard and primary selection!'
```

```lua
window:copy_to_clipboard('put me in the clipboard only', 'Clipboard')
```

```lua
window:copy_to_clipboard(
  'put me in the primary selection',
  'PrimarySelection'
)
```

