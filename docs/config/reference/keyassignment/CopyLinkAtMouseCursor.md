# `CopyLinkAtMouseCursor(destination)`

If the current mouse cursor position is over a cell that contains
a hyperlink, this action copies that link's URL to the clipboard
(or primary selection) instead of opening it.

If the link's URI is a single Markdown link (`[label](url)` or
`[label](<url>)`), only its destination is copied. When no link is under the
mouse, a selection is copied and cleared; a selection consisting entirely of
one such link is likewise copied as its destination. Other selected text is
preserved. Ordinary selection copying via Ctrl+C is unchanged.

`destination` accepts the same values (`Clipboard`, `PrimarySelection` or
`ClipboardAndPrimarySelection`) as
[CompleteSelection](CompleteSelection.md).

This is bound to a right-click release by default, so that right-clicking
a link copies its URL, while left-clicking it continues to open it via
[OpenLinkAtMouseCursor](OpenLinkAtMouseCursor.md) /
[CompleteSelectionOrOpenLinkAtMouseCursor](CompleteSelectionOrOpenLinkAtMouseCursor.md).

```
mouse_bindings: [
  ## Right-click will copy the link under the mouse cursor to the clipboard
  {
    event: { Up: { streak: 1, button: Right } }
    mods: NONE
    action: { CopyLinkAtMouseCursor: ClipboardAndPrimarySelection }
  }
]
```
