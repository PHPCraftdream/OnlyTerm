# CopyMode `PriorMatchPage`

{{since('20220624-141144-bd1b7c5d')}}

Move the CopyMode/SearchMode selection to the previous matching text on the previous page of the screen, if any.

```
key_tables: {
  search_mode: [
    {
      key: PageUp
      mods: CTRL
      action: { CopyMode: PriorMatchPage }
    }
  ]
}
```

