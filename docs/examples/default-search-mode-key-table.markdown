```
key_tables: {
  search_mode: [
    { key: Enter, mods: NONE, action: { CopyMode: PriorMatch } }
    { key: Escape, mods: NONE, action: { CopyMode: Close } }
    { key: n, mods: CTRL, action: { CopyMode: NextMatch } }
    { key: p, mods: CTRL, action: { CopyMode: PriorMatch } }
    { key: r, mods: CTRL, action: { CopyMode: CycleMatchType } }
    { key: u, mods: CTRL, action: { CopyMode: ClearPattern } }
    { key: PageUp, mods: NONE, action: { CopyMode: PriorMatchPage } }
    { key: PageDown, mods: NONE, action: { CopyMode: NextMatchPage } }
    { key: UpArrow, mods: NONE, action: { CopyMode: PriorMatch } }
    { key: DownArrow, mods: NONE, action: { CopyMode: NextMatch } }
  ]
}
```
