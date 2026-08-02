# `CopyModeAssignment` enumeration

Represents a pre-defined function that can be applied
to control [CopyMode](../../../../copymode.md) and [Search Mode](../../../../scrollback.md#searching-the-scrollback).
In a ktav config these are written the same way as top-level
[KeyAssignment](../index.md) variants: a bare name for a unit variant, or a
single-key object for one with arguments (e.g. `{ CopyMode: MoveLeft }` or
`{ CopyMode: { JumpForward: { prev_char: false } } }`).

## Available Key Assignments

