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
