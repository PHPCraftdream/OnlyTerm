# Cursor separates from prompt after shrinking a ConPTY window

## Reproducer and cause

The reported sequence is maximized window -> smaller window. A terminal-level
test reproduces the visible displacement without GPU rendering:

1. Fill a 53-row screen, leaving nonblank rows below the cursor.
2. Put `prompt> input` and its cursor on row 13.
3. Shrink the screen to 40 rows.
4. The prompt is now on visible row 0, but the previous code leaves the cursor
   on visible row 13. Subsequent input is written into the wrong line.

`Screen::resize` computes the reflowed physical row of the cursor. Its ConPTY
branch used that position to preserve the old visible cursor number and append
padding when growing. On shrink, nonblank rows below the cursor can prevent
padding and move the viewport origin. The previously computed visible number
then no longer identifies the cursor's text line.

## Fix

Keep the existing ConPTY padding behavior, then derive the visible cursor row
from the final viewport origin for both ConPTY and other terminals:

`visible_cursor = reflowed_physical_cursor - (stored_rows - viewport_rows)`

No renderer cache reset or arbitrary 13-row adjustment is involved.

## Verification

- The shrink regression failed before the fix: actual cursor row 13, expected 0.
- It passes after the fix, including a following character appended to the
  original prompt and a simultaneous width reduction from 145 to 100 columns.
- Growth from 40 to 53 rows with scrollback and clear/home/redraw also passes.
- All 84 terminal tests pass; strict all-target Clippy and formatting pass.
- The test harness now defaults to warning-level logging, with trace available
  through `RUST_LOG`. Its previous unconditional per-cell trace logging exhausted
  captured-output memory when the full suite reached the profiling tests.

The corrected GUI binary still needs runtime verification. This proves a
matching resize defect; it does not assert that every previous cursor report
had the same cause.

## Follow-up: input above a retained prompt

After the first fix, a second case was observed in `cmd.exe`. Read-only native
console inspection showed prompt and input together on native row 0, while
OnlyTerm retained the prompt on row 1 and displayed the input on row 0.
The diagnostic trace contained height reductions followed by absolute row-0
cursor/edit updates, without a repeated prompt. Tab switching could expose it,
but a separate tab-switch rendering defect was not established.

The pre-resize cleanup removed every blank row below the cursor. That pinned
the prompt's visible row instead of matching ConPTY's upward shift on shrink.
For primary ConPTY screens, cleanup now retains enough trailing rows for that
shift, bounded by the cursor's distance from the top. Excess trailing blanks
are still removed once the cursor reaches the top. Growth retains the previous
padding behavior.

The new regression starts with a blank line and a prompt, applies repeated
height reductions, then replays a ConPTY absolute-position edit without
reprinting the prompt. It failed before the change (cursor row 1 instead of 0)
and passes afterward. All 85 terminal tests pass; GUI runtime acceptance of this
follow-up is still required.

The separate diagnostic GUI used `--skip-config`, which explained its different
font. No user font configuration was changed.
