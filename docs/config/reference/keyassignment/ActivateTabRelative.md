# `ActivateTabRelative`

Activate a tab relative to the current tab.  The argument value specifies an
offset. eg: `-1` activates the tab to the left of the current tab, while `1`
activates the tab to the right.

```
keys: [
  { key: "{", mods: "ALT", action: { ActivateTabRelative: -1 } },
  { key: "}", mods: "ALT", action: { ActivateTabRelative: 1 } },
]
```

See also [ActivateTabRelativeNoWrap](ActivateTabRelativeNoWrap.md)


