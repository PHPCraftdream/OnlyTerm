# `CopyTo(destination)`

Copy the selection to the specified clipboard buffer.

Copied terminal text is trimmed of Unicode whitespace at the beginning and end
of the whole selection, including tabs and line breaks. Internal whitespace and
blank lines are preserved. A whitespace-only selection copies an empty string.
The same trimming applies to mouse selection copying, copy mode and Quick Select.
The visible selection and literal text supplied by `CopyTextTo` are unchanged.

Possible values for destination are:

* `Clipboard` - copy the text to the system clipboard.
* `PrimarySelection` - Copy the text to the primary selection buffer (applicable to X11 and some Wayland systems only)
* `ClipboardAndPrimarySelection` - Copy to both the clipboard and the primary selection.

```
keys: [
  {
    key: C
    mods: CTRL
    action: { CopyTo: ClipboardAndPrimarySelection }
  }
]
```

{{since('20220319-142410-0fcdea07')}}

`PrimarySelection` is now also supported on Wayland systems that support [primary-selection-unstable-v1](https://wayland.app/protocols/primary-selection-unstable-v1) or the older Gtk primary selection protocol.
