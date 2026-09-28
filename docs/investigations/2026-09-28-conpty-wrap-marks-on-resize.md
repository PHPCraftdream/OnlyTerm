# Input detaches from the cmd prompt after resizes (ConPTY wrap marks)

## Report

Rarely, after returning to a tab (from another tab of the same window)
after a long absence, typed input appeared several rows below the prompt,
starting at the prompt's end column:

```text
C:\src\app>




           git status
```

## Evidence

- Logs of all running windows show simultaneous system-wide resizes during
  the night (48 -> 49 -> 48 rows at 23:40 and 00:19, larger ones at 07:21),
  i.e. display power events. The affected tab need not be active.
- The tabs start `cmd /k "chcp 65001>nul & color f8"`. `color` fills the
  whole buffer through the console fill API.
- A headless harness (`crates/session/mux/examples/conpty_resize_cycle.rs`)
  drives the bundled OpenConsole 1.22.250204002 with cmd.exe and reads the
  native console buffer via read-only `AttachConsole`
  (`conpty-native-dump.ps1` next to it). ConPTY emits nothing on resize; the
  next keystroke is echoed with an absolute cursor move to the native row.
- With the fill, narrowing the window moved the native prompt up (120 -> 80
  columns: row 31 -> 22) while OnlyTerm kept it on row 31. Widening, then
  shortening, then restoring (120x48 -> 160x48 -> 160x43 -> 120x48) left the
  native prompt on row 31 but moved OnlyTerm's to row 26: input landed five
  rows below the prompt, exactly the report. Without `color` both agreed.

## Cause

OpenConsole marks a row wrap-forced as soon as its last column is written
and clears the mark on a line feed (`_stream.cpp`, `AdaptDispatch::_DoLineFeed`,
GH#15602). `ROW::MeasureRight` counts a wrap-forced row at full width, so its
reflow (`TextBuffer::Reflow`, run by `ResizeWindow` once for the width at the
taller height and again for a shorter height) treats the fill from the prompt
to the bottom as one long soft-wrapped line. Narrowing makes it longer and
pushes rows off the top; widening shortens it, which also shrinks the output
extent that later height changes use.

ConPTY paints the fill row by row with absolute moves, so the terminal never
saw a wrap and did not reflow those rows.

## Fix

In ConPTY mode the model mirrors the native marks and reflow:

- writing the last column marks the row; LF, IND and NEL clear the mark;
  partial erases (ED 0/1, EL, ECH) keep it, as `TextBuffer::FillRect` does,
  while a full `CSI 2J` resets rows. cmd's cooked read redraws pending input
  after every resize with `CSI J`, so this matters whenever a resize arrives
  while a command is half typed;
- a width reflow keeps wrapped rows at full width (only the trailing blanks
  of a logical line's last row are dropped), flushes a wrapped bottom row,
  splits a line wrapped from history at the viewport top (ConPTY's buffer
  starts there), stops one buffer height below the cursor, and places the
  viewport where ConPTY's buffer top ends up;
- a shorter height copies rows only up to the output extent (last row with
  text or a wrap mark, at least the cursor row), and a same-width reflow that
  is not cut short clears the mark of its last copied row.

## Verification

- `crates/terminal/term/src/test/screen/conpty_reflow.rs` contains a
  transcription of OpenConsole's resize path. Its results match every native
  capture taken (width-only shrinks for several prompt rows, the reported
  sequence, combined width/height sequences and two random 7-step
  sequences); `native_captures` asserts both the model and OnlyTerm against
  those rows.
- `random_resizes_match_native_model` compares OnlyTerm with that model over
  300 random sequences of 10 resizes (with and without the fill, with long
  wrapped output, with short and wrapping pending input, one-row nudges and
  large jumps), checking every visible row and the cursor, and that a
  native-positioned echo lands after the prompt. A local run over 3000
  sequences of 12 resizes (before pending input was added) found no
  divergence.
- Against the previous code, five of the six new tests fail (the sixth
  guards that non-ConPTY terminals and DECAWM-off rows are not marked);
  `native_captures` already fails on its first case (one column narrower).
  Reverting only the erase change makes `conpty_erase_keeps_wrap_marks` and
  the differential test (a pending-input seed) fail.
- The harness confirmed the model against the real ConPTY for the reported
  and fuzzed sequences, a pending-input sequence, `cls`, and four 48/49-row
  cycles.

Not covered: full-width `WriteConsoleOutput` rectangles on the primary
buffer, which ConPTY paints like a fill but does not mark; they are now
reflowed as wrapped rows. Full-screen console applications normally use an
alternate buffer, which is not reflowed.
