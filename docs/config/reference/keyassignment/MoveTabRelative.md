# `MoveTabRelative`

Move the current tab relative to its peers.  The argument specifies an
offset. eg: `-1` moves the tab to the left of the current tab, while `1` moves
the tab to the right.

```
keys: [
  { key:: {, mods: SHIFT|ALT, action: { MoveTabRelative: -1 } }
  { key:: }, mods: SHIFT|ALT, action: { MoveTabRelative: 1 } }
]
```

Note the double colon (`key:: {` / `key:: }`): the `key` value here is the
literal `{`/`}` character, which would otherwise be parsed as the start/end
of a nested object. `key::` forces the rest of the line to be treated as a
literal string, which is the only way to write a bare `{` or `}` as a scalar
value in ktav (quoting does not help here, since ktav never strips quote
characters from a value).


