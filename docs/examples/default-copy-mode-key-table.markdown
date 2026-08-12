```
key_tables: {
    copy_mode: [
      { key: Tab, mods: NONE, action: { CopyMode: MoveForwardWord } }
      { key: Tab, mods: SHIFT, action: { CopyMode: MoveBackwardWord } }
      { key: Enter, mods: NONE, action: { CopyMode: MoveToStartOfNextLine } }
      { key: Escape, mods: NONE, action: { Multiple: [ScrollToBottom, { CopyMode: Close }] } }
      { key: Space, mods: NONE, action: { CopyMode: { SetSelectionMode: Cell } } }
      { key: $, mods: NONE, action: { CopyMode: MoveToEndOfLineContent } }
      { key: $, mods: SHIFT, action: { CopyMode: MoveToEndOfLineContent } }
      { key: \,, mods: NONE, action: { CopyMode: JumpReverse } }
      { key: phys:0, mods: NONE, action: { CopyMode: MoveToStartOfLine } }
      { key: ;, mods: NONE, action: { CopyMode: JumpAgain } }
      { key: F, mods: NONE, action: { CopyMode: { JumpBackward: { prev_char: false } } } }
      { key: F, mods: SHIFT, action: { CopyMode: { JumpBackward: { prev_char: false } } } }
      { key: G, mods: NONE, action: { CopyMode: MoveToScrollbackBottom } }
      { key: G, mods: SHIFT, action: { CopyMode: MoveToScrollbackBottom } }
      { key: H, mods: NONE, action: { CopyMode: MoveToViewportTop } }
      { key: H, mods: SHIFT, action: { CopyMode: MoveToViewportTop } }
      { key: L, mods: NONE, action: { CopyMode: MoveToViewportBottom } }
      { key: L, mods: SHIFT, action: { CopyMode: MoveToViewportBottom } }
      { key: M, mods: NONE, action: { CopyMode: MoveToViewportMiddle } }
      { key: M, mods: SHIFT, action: { CopyMode: MoveToViewportMiddle } }
      { key: O, mods: NONE, action: { CopyMode: MoveToSelectionOtherEndHoriz } }
      { key: O, mods: SHIFT, action: { CopyMode: MoveToSelectionOtherEndHoriz } }
      { key: T, mods: NONE, action: { CopyMode: { JumpBackward: { prev_char: true } } } }
      { key: T, mods: SHIFT, action: { CopyMode: { JumpBackward: { prev_char: true } } } }
      { key: V, mods: NONE, action: { CopyMode: { SetSelectionMode: Line } } }
      { key: V, mods: SHIFT, action: { CopyMode: { SetSelectionMode: Line } } }
      { key: ^, mods: NONE, action: { CopyMode: MoveToStartOfLineContent } }
      { key: ^, mods: SHIFT, action: { CopyMode: MoveToStartOfLineContent } }
      { key: b, mods: NONE, action: { CopyMode: MoveBackwardWord } }
      { key: b, mods: ALT, action: { CopyMode: MoveBackwardWord } }
      { key: b, mods: CTRL, action: { CopyMode: PageUp } }
      { key: c, mods: CTRL, action: { Multiple: [ScrollToBottom, { CopyMode: Close }] } }
      { key: d, mods: CTRL, action: { CopyMode: { MoveByPage: 0.5 } } }
      { key: e, mods: NONE, action: { CopyMode: MoveForwardWordEnd } }
      { key: f, mods: NONE, action: { CopyMode: { JumpForward: { prev_char: false } } } }
      { key: f, mods: ALT, action: { CopyMode: MoveForwardWord } }
      { key: f, mods: CTRL, action: { CopyMode: PageDown } }
      { key: g, mods: NONE, action: { CopyMode: MoveToScrollbackTop } }
      { key: g, mods: CTRL, action: { Multiple: [ScrollToBottom, { CopyMode: Close }] } }
      { key: h, mods: NONE, action: { CopyMode: MoveLeft } }
      { key: j, mods: NONE, action: { CopyMode: MoveDown } }
      { key: k, mods: NONE, action: { CopyMode: MoveUp } }
      { key: l, mods: NONE, action: { CopyMode: MoveRight } }
      { key: m, mods: ALT, action: { CopyMode: MoveToStartOfLineContent } }
      { key: o, mods: NONE, action: { CopyMode: MoveToSelectionOtherEnd } }
      { key: q, mods: NONE, action: { Multiple: [ScrollToBottom, { CopyMode: Close }] } }
      { key: t, mods: NONE, action: { CopyMode: { JumpForward: { prev_char: true } } } }
      { key: u, mods: CTRL, action: { CopyMode: { MoveByPage: -0.5 } } }
      { key: v, mods: NONE, action: { CopyMode: { SetSelectionMode: Cell } } }
      { key: v, mods: CTRL, action: { CopyMode: { SetSelectionMode: Block } } }
      { key: w, mods: NONE, action: { CopyMode: MoveForwardWord } }
      { key: y, mods: NONE, action: { Multiple: [{ CopyTo: ClipboardAndPrimarySelection }, { Multiple: [ScrollToBottom, { CopyMode: Close }] }] } }
      { key: phys:B, mods: ALT, action: { CopyMode: MoveBackwardWord } }
      { key: phys:B, mods: CTRL, action: { CopyMode: PageUp } }
      { key: phys:c, mods: CTRL, action: { Multiple: [ScrollToBottom, { CopyMode: Close }] } }
      { key: phys:d, mods: CTRL, action: { CopyMode: { MoveByPage: 0.5 } } }
      { key: phys:F, mods: ALT, action: { CopyMode: MoveForwardWord } }
      { key: phys:F, mods: CTRL, action: { CopyMode: PageDown } }
      { key: phys:G, mods: CTRL, action: { Multiple: [ScrollToBottom, { CopyMode: Close }] } }
      { key: phys:M, mods: ALT, action: { CopyMode: MoveToStartOfLineContent } }
      { key: phys:u, mods: CTRL, action: { CopyMode: { MoveByPage: -0.5 } } }
      { key: phys:v, mods: CTRL, action: { CopyMode: { SetSelectionMode: Block } } }
      { key: PageUp, mods: NONE, action: { CopyMode: PageUp } }
      { key: PageDown, mods: NONE, action: { CopyMode: PageDown } }
      { key: End, mods: NONE, action: { CopyMode: MoveToEndOfLineContent } }
      { key: Home, mods: NONE, action: { CopyMode: MoveToStartOfLine } }
      { key: LeftArrow, mods: NONE, action: { CopyMode: MoveLeft } }
      { key: LeftArrow, mods: ALT, action: { CopyMode: MoveBackwardWord } }
      { key: RightArrow, mods: NONE, action: { CopyMode: MoveRight } }
      { key: RightArrow, mods: ALT, action: { CopyMode: MoveForwardWord } }
      { key: UpArrow, mods: NONE, action: { CopyMode: MoveUp } }
      { key: DownArrow, mods: NONE, action: { CopyMode: MoveDown } }
    ]

}
```
