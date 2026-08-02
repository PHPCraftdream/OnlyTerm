# window.copy_to_clipboard(text \[,clipboard\])

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

