# CopyMode `MoveUp`

{{since('20220624-141144-bd1b7c5d')}}

Moves the CopyMode cursor position one cell up.

```rhai
return #{
  key_tables: #{
    copy_mode: [
      #{ key: "UpArrow", mods: "NONE", action: act.CopyMode("MoveUp") },
    ],
  },
}
```

