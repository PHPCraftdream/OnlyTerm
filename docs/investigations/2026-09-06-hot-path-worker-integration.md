# Hot-path fixes: worktree integration review

Baseline: `09ae41e41`. Three `hl` workers used separate worktrees and target
directories. The main worktree integrates reviewed commits with merge commits.
No installed binaries or user configuration are changed by these fixes.

## Process discovery and remote UTF-16 reads

Worker commit: `913b9d2f8` (`fix/hl-process-paths`).

- Reject odd or oversized remote UTF-16 byte lengths before `ReadProcessMemory`;
  handle the empty string without invoking the API on a zero-sized allocation.
- Copy only the root cwd, not the entire descendant tree. Clone a selected
  foreground process without descendants.
- Index PPIDs once when constructing the detailed process tree. The deterministic
  regression test has 5000 records and 101 reachable nodes: 5000 PPID reads versus
  505000 in the former full-scan algorithm. This is an operation count, not a
  wall-clock speedup claim.
- Retain raw UTF-16 names for the fresh keyboard-only snapshot. Convert only
  names belonging to the selected subtree. Keep the existing detailed snapshot
  representation separate, so it does not acquire large fixed-size copies.

Review checked allocation capacity versus FFI byte count, source-tree preservation,
cycle/duplicate PID handling, and actual node discovery order. An initial test
confused discovery order with traversal order; it was corrected before merge.

Worker validation: 14 procinfo tests, 5 process-cache mux tests, targeted Clippy.
Integration validation is recorded below after all branches are merged.

## GPU buffers

Worker commit: `0010f461e` (`perf/hl-gpu-buffers`).

The child now keeps one instance buffer per draw slot. Stationary draws reuse
their handles; growing data replaces an undersized buffer, and a greater-than-4x
capacity surplus releases a transient large allocation. Empty/fewer draws drop
unused slots. Attach clears all device-owned pool handles.

Review required bounded power-of-two rounding against the device limit and an
empty queue submission on Lost/Outdated: these errors occur before normal
submission, so queued uploads otherwise accumulate during repeated failures.
Root added a contiguous-slot guard and removed duplicate capacity storage.

The initial allocation tests simulated the implementation rather than calling
it. Final tests share production bookkeeping and include a real-device test of
buffer identity, distinct slots, growth, shrink, oversized requests and empty
frames. Root removed that test's ignore marker. It is not counted as passed
until the actual GPU test executes.

## Logging and images

Worker commit: `9aac87e8e` (`fix/hl-responsiveness`).

The logger formats target/message/timestamp once and moves the strings into its
ring. An explicit length fixes the full-ring ambiguity after 16/32/etc. records.
Frequent key diagnostics use Debug. Root retained synchronous flush for **all**
enabled levels: the global logger is not dropped on process exit, so buffering
Debug would lose precisely the idle/shutdown tail needed during investigations.
The proposed background flush worker was not retained.

Image decoding uses non-blocking channel polling with a 25 ms retry deadline.
Cache hits and misses both return that deadline; an unavailable later frame
also gets a future deadline rather than a busy-looping expired one. Loading
images use the existing coalesced budget-repaint timer even without focus;
ordinary unfocused animation remains paused, as before.

Decoder disconnect exits Loading, and the transition to replay no longer skips
frame zero. Root added a test of the actual CPU atlas/cache path: initial miss,
placeholder hit, first real frame and an overdue-but-not-ready subsequent frame.

## Integration review checklist

- GPU reuse must exercise the production pool, respect the device size limit,
  separate draw slots, clear device-owned handles on attach, and flush pending
  uploads on recoverable surface failures before retrying.
- Image decoding must not block the GUI, return future retry deadlines on both
  cache-hit and cache-miss paths, and progress while the window is unfocused.
- Logging must retain the most recent 16 entries through wrap-around. A change
  to flush policy must account for idle/shutdown tails and worker-start failure.
- Modified and newly added Rust files must remain at or below 1000 lines.

The large render module was split into cell rendering, cache benchmarks and
retained-row tests. Its remaining module is 830 lines; no public rendering
contract was changed by that extraction.

## Validation environment

Root repeated the process tests after merge: 14 procinfo and 5 process-cache
mux tests passed. Worker logger tests passed before root retained Debug flush.

The first full GPU-test builds could not complete. Besides sccache/compiler
failures, a retry produced explicit Windows error 1455 when mapping
config/window/onlyterm-client/windows rlibs (reported as E0786 metadata errors).
This is not proof of corrupt source/cache files: the OS said the paging/commit
budget was insufficient. The executing session's observed parent command is
`capm.ps1 5g capc 30 idle cx resume --last`; the 5 GiB job budget is inherited
by child builds. System-wide committed memory was only 57% at the observation.
No user processes or system memory limits were changed.

Clearing `RUSTC_WRAPPER` in PowerShell was not sufficient: a rustc wrapper was
also configured in both parent Cargo configuration files. An explicit temporary
Cargo override disabled both wrapper settings. Normal `cargo check` then passed;
the full debug-symbol build still exceeded the memory budget while mapping
large rlibs. The final build/tests passed with debug symbols disabled, retaining
the ordinary unoptimized dev/test semantics and debug assertions.

The local, ignored `.scratch/2026-09-06-hl-verification.toml` used:

```toml
[build]
rustc-wrapper = ""
rustc-workspace-wrapper = ""
jobs = 1

[profile.dev]
debug = 0

[profile.test]
debug = 0
```

Final verification used the task-owned target directory
`D:/dev/rust/.cargo-target-hl-gpu`, with the config above passed via `--config`:

- `cargo build -p onlyterm-gui -j 1` — passed.
- `cargo test -p onlyterm-gui -p onlyterm-gpu-render -p env-bootstrap -p procinfo -j 1 -- --test-threads=1`
  — passed: GUI 147, GPU 38, logger 3, procinfo 14; four pre-existing GUI tests
  remain ignored. The real-device pool test ran successfully, as did the native
  GPU-child lifecycle tests.
- `cargo clippy -p onlyterm-gui -p onlyterm-gpu-render -p env-bootstrap -p procinfo -p mux --all-targets -j 1 -- -D warnings`
  — passed.
- `cargo fmt --all -- --check` and `git diff --check` — passed. The existing
  stable-rustfmt `imports_granularity` warnings are unchanged.
- Changed/new Rust file length check — passed, maximum 976 lines.

Root follow-ups include `65336041d` (render decomposition, GPU guards and tests,
all-level log flush) and `61d2f56d9` (a deterministically stale image deadline in
the regression fixture). Tests use narrow test-only constructors rather than
opening production decoder internals.

No wall-clock FPS or end-to-end input-latency speedup is claimed. Raw UTF-16
snapshot records trade larger temporary records for fewer per-process heap
allocations; they are deliberately not used for the shared detailed snapshot.
The native Windows FFI path was tested through process lookups; Miri was not
used for Win32 calls. The existing ReadProcessMemory unsafe block now has its
destination byte-count invariant checked before the call.

## Limits

The prior intermittent keyboard failure in an old installed process was not
reproduced. These fixes cover identified code paths and tested regressions;
they do not prove that a specific historical failure had the same cause.
Process-name compatibility remains scoped to local Windows panes; no remote
process-discovery protocol is introduced here.
