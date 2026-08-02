# CopyMode `CycleMatchType`

{{since('20220624-141144-bd1b7c5d')}}

Move the CopyMode/SearchMode cycle between case-sensitive, case-insensitive
and regular expression match types.

```
key_tables: {
  search_mode: [
    { key: r, mods: CTRL, action: { CopyMode: CycleMatchType } }
  ]
}
```

