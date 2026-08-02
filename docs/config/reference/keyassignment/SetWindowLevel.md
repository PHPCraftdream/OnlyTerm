# `SetWindowLevel`

{{since('20240127-113634-bbcac864')}}

Set window level specified by the argument value. eg: `AlwaysOnTop` keeps the current window on top of other windows.

Accepted values: 

 * `"AlwaysOnBottom"`
 * `"Normal"` (this is the default)
 * `"AlwaysOnTop"`

```
keys: [
  {
    key:: [
    mods: CMD
    action: { SetWindowLevel: AlwaysOnBottom }
  }
  {
    key: 0
    mods: CMD|SHIFT
    action: { SetWindowLevel: Normal }
  }
  {
    key:: ]
    mods: CMD
    action: { SetWindowLevel: AlwaysOnTop }
  }
]
```

Note the double colon (`key:: [` / `key:: ]`): the `key` value here is the
literal `[`/`]` character, which would otherwise be parsed as the start/end
of an array. `key::` forces the rest of the line to be treated as a literal
string, which is the only way to write a bare `[` or `]` as a scalar value
in ktav (quoting does not help here, since ktav never strips quote
characters from a value).

!!! note 
    This functionality is currently only implemented on macOS. 
    The assigned values for window level will have no effect on other operating systems.
