# `SendString`

Sends the string specified argument to the terminal in the current tab, as
though that text were literally typed into the terminal.

```
keys: [
  { key: m, mods: CMD, action: { SendString: Hello } }
]
```

You can also emit escape sequences using `SendString`.  This example shows
how to bind Alt-LeftArrow/RightArrow to the Alt-b/f, an emacs style
keybinding for moving backwards/forwards through a word in a line editor.

`\x1b` is the ESC character. **Known ktav limitation:** ktav's string escape
set is fixed to `\\`, `\,`, `\}`, `\]`, `\{`, `\[`, `\n`, `\r`, `\.` and `\:`
only (see the [migration guide](../../../migration-to-ktav.md)) — there is
no `\xNN` hex-byte escape, so a literal ESC character cannot currently be
written directly in a ktav string value. Prefer [SendKey](SendKey.md) for
this specific use case, since it lets you express `Alt-b`/`Alt-f` as a key
press rather than as a raw escape sequence byte:

```
keys: [
  ## Make Option-Left equivalent to Alt-b which many line editors interpret as backward-word
  { key: LeftArrow, mods: OPT, action: { SendKey: { key: b, mods: ALT } } }
  ## Make Option-Right equivalent to Alt-f; forward-word
  { key: RightArrow, mods: OPT, action: { SendKey: { key: f, mods: ALT } } }
]
```

See also [SendKey](SendKey.md) which makes the example above much more convenient,
and [Multiple](Multiple.md) for combining multiple actions in a single press.
