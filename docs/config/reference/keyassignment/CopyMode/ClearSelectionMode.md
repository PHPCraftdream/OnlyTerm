# CopyMode `ClearSelectionMode`

{{since('20220807-113146-c2fee766')}}

Clears the current CopyMode selection mode without leaving CopyMode.

```
key_tables: {
  copy_mode: [
    {
      key: y
      mods: NONE
      action: {
        Multiple: [
          { CopyTo: PrimarySelection }
          ClearSelection
          ## clear the selection mode, but remain in copy mode
          { CopyMode: ClearSelectionMode }
        ]
      }
    }
  ]
}
```

See also: [SetSelectionMode](SetSelectionMode.md).
