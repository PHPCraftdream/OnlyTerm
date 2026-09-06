# Follow-up to the independent recent-commits review

Scope: verified findings A-J against the current code and fixed confirmed defects.
The original independent review is preserved; corrections are recorded here.

| Finding | Validation / action | State |
|---|---|---|
| A | Restored escaped Unicode banner symbols; production formatter regression passes | fixed |
| B | Windows workflows at both reviewed and current revisions run cargo-nextest; an ordinary no-conflict merge need not add a merge-resolution diff | report corrected |
| C | Optional missing-adapter path is explicit; required GPU mode fails without an adapter; device/validation errors still fail | fixed; four targeted tests passed |
| D | Explicit Debug/Trace uses synchronous output; full-queue fallback waits for earlier records to flush | fixed; ten logger tests passed |
| E | Fresh Toolhelp reads retained; executable names packed into a shared UTF-16 buffer instead of MAX_PATH-sized entries; remaining per-key info log lowered | improved; 17 tests passed in debug and optimized profiles |
| F | Added coverage of an old cursor row that leaves the viewport entirely; text stays in history and live cursor clamps to the visible screen; no destructive workaround | covered; behavior boundary retained |
| G | Documented title precedence, opt-in process titles, CLI policy and client-side configuration scope; existing CLI classification regression passes | docs corrected; intended policy retained |
| H | Snapshot lock waiting yields without marking the pane unresponsive; cancellation releases the wait; evicted wrapped prefixes are discarded without joining unrelated suffixes or failing all results | fixed; 17 search tests passed |
| I | Every decoder frame retains its timeline entry and duration; independent pixel refills cannot consume a repeated frame by hash | fixed; duplicate-frame timing regression passed |
| J | Empty fallback map avoids text traversal; line fingerprint streams cell text without building an owned string; unrelated characters are not hashed | improved; selective cache regressions passed |

## Verification

- Full GUI suite: 170 passed; four existing manual/UAC/benchmark tests ignored.
- Full mux suite: 78 passed.
- Full terminal suite: 91 passed.
- Logger: 10 passed.
- procinfo: 17 passed in both debug and optimized dev-install profiles.
- CLI title-command classification: one passed.
- GPU test policy: two tests passed with graphics backends disabled. Both real
  buffer/atlas tests also passed with ONLYTERM_REQUIRE_GPU_TESTS=1, so these runs
  did not take the optional adapter-skip path.
- Strict all-target Clippy passed for mux, GUI, logger, procinfo, terminal and
  GPU rendering. Nightly formatting and git diff whitespace checks passed.

All commands were serialized. No new dependency, unsafe block, public wire-format
change or user configuration edit was introduced.

## Limits and deliberate choices

- CI was already using nextest; the review's search for cargo test missed that.
  Local unpublished changes are not claimed to have passed remote CI. No push
  or tag mutation is part of this follow-up.
- Ordinary Info-level GUI logging remains buffered. Explicit Debug/Trace avoids
  this buffering; neither mode promises power-loss durability. Forced termination
  can still interrupt an in-progress write.
- F is not represented as a fixed regression: retaining every full row while
  shrinking can push the old cursor's text into history. The live cursor stays
  within the viewport; application redraw is covered. A different viewport/data
  retention policy would need a separate design and native-conformance check.
- Fresh keyboard snapshots intentionally remain fresh on each relevant chord;
  the change reduces copying/allocation pressure, not the Toolhelp call count.
- Title CLI commands remain opt-in because scripts can invoke them. The CLI
  configuration gate is not a mux authorization/security boundary.

The previous requested scrollbar build and launch completed before this work
(CMD preview PID recorded in the session). That running binary does not contain
these additional review fixes. Changes here still need committing and final
GUI runtime acceptance.
