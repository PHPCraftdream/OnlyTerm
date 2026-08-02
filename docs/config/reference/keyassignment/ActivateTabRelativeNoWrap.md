# `ActivateTabRelativeNoWrap`

{{since('20220101-133340-7edc5b5a')}}

Activate a tab relative to the current tab.  The argument value specifies an
offset. eg: `-1` activates the tab to the left of the current tab, while `1`
activates the tab to the right.

This is almost identical to [ActivateTabRelative](ActivateTabRelative.md)
but this one will not wrap around; for example, if the first tab is active
`ActivateTabRelativeNoWrap=-1` will not move to the last tab and vice versa.


```
keys: [
  { key:: {, mods: ALT, action: { ActivateTabRelativeNoWrap: -1 } }
  { key:: }, mods: ALT, action: { ActivateTabRelativeNoWrap: 1 } }
]
```

Note the double colon (`key:: {` / `key:: }`): the `key` value here is the
literal `{`/`}` character, which would otherwise be parsed as the start/end
of a nested object. `key::` forces the rest of the line to be treated as a
literal string, which is the only way to write a bare `{` or `}` as a scalar
value in ktav (quoting does not help here, since ktav never strips quote
characters from a value).


