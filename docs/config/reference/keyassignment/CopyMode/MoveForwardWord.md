# CopyMode `MoveForwardWord`

{{since('20220624-141144-bd1b7c5d')}}

Moves the CopyMode cursor position one word to the right.

```rhai
return #{
  key_tables: #{
    copy_mode: [
      #{ key: "w", mods: "NONE", action: act.CopyMode("MoveForwardWord") },
    ],
  },
}
```

