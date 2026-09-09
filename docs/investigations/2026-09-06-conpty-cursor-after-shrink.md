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

## Runtime acceptance and remaining cosmetic issue

The user subsequently confirmed that input stays aligned in the new build;
the previous first-use CJK glyph-rain regression is also gone.

One low-priority difference remains: shrinking a window can expose a scrollbar
while opening a fresh tab at that same size does not. Read-only inspection of
the running build found one history line containing only whitespace in each of
the sampled non-CJK tabs. The scrollbar condition counts history rows, including
blank ones (`scrollback_rows > viewport_rows`). The CJK tab had real text history
and correctly remained scrollable.

This empty-history scrollbar issue is recorded, not fixed in this release. Any
follow-up should preserve real scrollback and the validated cursor/resize
behavior, without adding a full history scan to each rendered frame.

## Unreleased follow-up: blank-history scrollbar

On a primary ConPTY screen with no pre-existing scrollback, a shrinking resize
now discards the visually empty prefix newly moved into history. This runs only
at resize, stops at the first meaningful row and does not scan existing history
in the paint path. Stable row offsets advance by the removed count, keeping
remaining text and cursor identities unchanged.

Rows with text, images, hyperlinks, visible backgrounds, underline, overline or
strikethrough are preserved. Background comparisons use the active terminal
palette, including palette overrides. Existing history is not pruned.

The reproducer failed before the fix (24 stored rows for a 23-row viewport) and
passes afterward. All 90 terminal tests and strict all-target Clippy pass,
including both cursor resize regressions, repeated shrink/grow, stable row
identity, existing history, decorated whitespace, active palette and images.
This follow-up still requires GUI runtime acceptance and is not in the existing
`v0.0.20-alpha` tag.

## September 9 follow-up: output extent is not viewport height

The old shrink rule was insufficient after a window had first grown. A fresh
maximized CMD followed by restore/maximize reproduced the user's report:
OnlyTerm retained the prompt on row 0 and placed new input on row 1, while a
read-only native console snapshot showed both together on row 1. Subsequent
cooked-read redraws could also erase the displaced prompt.

Captures from the bundled OpenConsole/ConPTY 1.22.250204002 established distinct
cases (row numbers below are zero-based):

| Sequence from a 100-column, 24-row CMD | Native prompt/input row |
|---|---:|
| No `color` command, shrink to 23 rows | 1 |
| `color f8` full-screen fill, shrink directly to 23 rows | 0 |
| Same fill, grow to 145×53, shrink to 100×24 | 1 |
| Same fill, grow to 145×53, shrink to 100×18 | 0 |

The native resize code distinguishes the virtual output extent from the
allocated viewport. Growing the viewport does not make its new empty rows
output. See the matching release's
[ResizeWindow](https://github.com/microsoft/terminal/blob/v1.22.10352.0/src/host/outputStream.cpp)
and [ResizeWithReflow](https://github.com/microsoft/terminal/blob/v1.22.10352.0/src/host/screenInfo.cpp).

`Screen` now tracks the output extent independently. Printing extends it;
new viewport padding and erase-to-end do not. Resize removes unused trailing
padding before calculating the upward shift, preserves the extent on growth,
and derives the final cursor from the retained text's physical position.
No frame-by-frame history scan or hard-coded row displacement is involved.

A first attempt that recognized only zero-length rows passed tests but failed
again in the GUI: `CSI J` fills those rows with attributed blank cells. The
final regressions therefore replay native cooked-read redraws (CUP to the
input start, erase-to-end, input-only rewrite) through multiple grow/shrink
cycles, not merely a new character after the first resize.

Older history-preservation tests assumed that untouched blank padding forced
scrolling. Their fixtures now shrink to one row to force real history
pressure; assertions still preserve image, hyperlink, decoration, palette,
semantic-zone and stable-row identities. The original color-startup fixture
remains covered, including fragmented input.

All 97 terminal tests pass. The final optimized GUI also retained Latin and
Cyrillic input beside the prompt through 30 window-state transitions,
including minimize/restore. Native and emulated row contents were compared;
the observed input displacement is no longer present in those scenarios.
