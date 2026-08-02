# CopyMode `MoveLeft`

{{since('20220624-141144-bd1b7c5d')}}

Moves the CopyMode cursor position one cell to the left.

```
key_tables: {
  copy_mode: [
    { key: h, mods: NONE, action: { CopyMode: MoveLeft } }
    { key: LeftArrow, mods: NONE, action: { CopyMode: MoveLeft } }
  ]
}
```
