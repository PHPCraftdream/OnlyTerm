# Review remediation: implementation and acceptance

## Changes

- Font bytes are shared immutable owned buffers. Runtime font consumers do not
  retain file mappings that could change underneath a shaper.
- Fallback faces are appended without destroying parsed faces, shaping plans,
  or metrics. Character-specific completion generations invalidate dependent
  shape, line-element, line-quad and retained-row entries. The per-row fingerprint
  is cached by terminal sequence number and fallback notification epoch, so warm
  frames do not rescan every character. IME composition participates separately.
- A bounded background glyph queue was implemented and tested, but rejected
  during runtime acceptance: the user observed visibly slower first-use CJK
  rendering in batches ("rain"). It has been removed from the source, including
  its workers, pending placeholders and retry integration. Cache misses once
  again rasterize immediately; shared font bytes and selective fallback caches
  remain. Unit tests alone did not expose this visual latency regression.
- Search captures bounded physical-row snapshots and runs matching off the GUI
  thread and terminal lock. Two admissions include cancellation lifetime of the
  actual worker. Wrapped logical lines can exceed the snapshot batch size; total
  search memory is not claimed to have a fixed byte cap. Unicode expansion,
  final sigma, combining marks, wide cells, exact limits and range bounds are covered.
- PTY action chunks move ownership into terminal processing instead of cloning.
  Render snapshots use copy-on-write line storage, and wire frames take ownership
  of accumulated instance vectors while recycling the previous buffer.
- Decoded images use a byte/entry-bounded shared pixel cache and bounded async
  disk refills. Pixel vectors are moved, WebP frame production is lazy, and blob
  lease reuse no longer recursively locks the storage mutex.
- Process snapshots share immutable arrays; cwd-only queries skip remote argv
  reads. Single-destination clipboard operations move text. GUI diagnostics use
  bounded background buffering with flush barriers and synchronous fallback.
- Isolated startup tabs prepare concurrently in groups of four and materialize
  in configured order. Each tab waits on shared completion signals for every
  predecessor in its group, including a failed predecessor. Non-isolated domain
  spawning and UAC remain serialized because their APIs materialize during spawn.

## Rare wheel jump

Two concrete hazards were fixed: signed wheel-delta multiplication could wrap,
and an empty-scrollback viewport could retain an explicit history-start position
instead of the follow-bottom sentinel. Boundary and transition tests pass. These
are code-backed candidates for the reported rare symptom, not a claim that its
exact original event sequence was reproduced.

## Verification

All builds and tests were serialized with one Cargo job; delegated agents only
edited code and test sources. No user terminal process was terminated.

- GUI after removing the queue: 167 passed, four existing manual/UAC/benchmark probes ignored.
- Font: 66 passed, one existing manual CJK measurement ignored.
- Mux: 75 passed, including 15 search tests.
- Surface: 57 passed with all features; 55 without default features.
- Blob leases: two passed with all features.
- Process information: 16 passed.
- Diagnostic logger: eight passed.
- Windows wheel conversion: two passed; GUI viewport tests: two passed.
- Strict Clippy passed for GUI, mux, fonts, surface, blob leases, procinfo and
  window with all targets and warnings denied.

The trial optimized build succeeded and launched all ten CJK page markers, but
failed user acceptance because of the first-use rendering regression above.
Subsequent agent-side compilation hit allocation failures, but the user built
the corrected version successfully. After launching it, the user confirmed that
glyph rain and downward input displacement were fixed. The upward-displacement
follow-up (`8d24239e0`) passes all 85 terminal tests and still requires a fresh
GUI build/runtime check. A separate first-glyph lookup regression also passes.

The unchanged ten-page/500-line CJK fixture has SHA-256
`a46d1214e7986ff0dcc43a8e93ddeb502ba566d67e9ca90e1715e09b00ef1d71`.

No release/version bump, push or user configuration changes are included.

## Safety boundary

The new unsafe trait-method implementations are
`DecodedPixelsHandle::pixel_data` and `pixel_data_mut` in
`crates/onlyterm-gui/src/glyphcache/image_decode.rs`. Dimensions and exact RGBA
length are validated before constructing the handle; its `Arc<Vec<u8>>` owns
immutable storage for the full borrow. The mutable method deliberately panics
instead of exposing shared writable storage, matching the existing immutable
decoded-image implementation. No new unsafe block, manual Send/Sync impl,
cryptographic operation or external dependency was introduced. Miri was not run
for the Windows GUI/FFI target; dimension and pointer-ownership regressions were
run as native tests.

## Additional cursor report during runtime acceptance

After the user identified shrinking a maximized window as the trigger, a
terminal-level regression reproduced the 13-row mismatch. ConPTY's resize
padding branch preserved an obsolete visible cursor number after the viewport
origin moved. The fix recomputes that number from the final viewport origin.
All 84 terminal tests and strict all-target Clippy pass. Fix and reproducer are
committed as `08cb11589`; details are in
[the cursor investigation](2026-09-06-conpty-cursor-after-shrink.md).
The user confirmed this first cursor fix. A subsequent native-console comparison
identified separate trailing-blank handling on shrink; see the same investigation
for commit `8d24239e0`, whose runtime verification remains pending.
