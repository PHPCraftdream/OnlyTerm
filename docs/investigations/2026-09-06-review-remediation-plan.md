# Performance review remediation

This tracks both follow-up reviews after the CJK atlas/cache fix. Completion
requires behavior and regression evidence, not just implementation or a green
test unrelated to the reviewed path.

| Requirement | Implementation owner | Acceptance evidence | Status |
|---|---|---|---|
| Share immutable font data; avoid eager color rasterizer loading | hl fonts | 66 font tests passed; one existing manual probe ignored | verified |
| Incremental fallback insertion; preserve unaffected shaping state | hl fonts + integration | retained-face/plan tests, selective CJK-vs-Latin row tests and cached fingerprint tests passed | verified |
| Bounded background preparation of new glyphs | hl raster | rejected after user observed first-use glyph rain; queue removed, immediate rasterization restored and regression-tested | closed; rejected experiment |
| Search outside terminal lock with bounded snapshots | hl search + root | 75 mux tests passed, including 15 search tests and cancellation admission | verified |
| Exact search result limit and Unicode case-insensitive offsets | hl search + root | 15 search tests passed: zero/exact cap, expanding mappings, Greek sigma, CJK and wrapped ranges | verified |
| Move owned PTY actions instead of cloning chunks | hl search | 75 mux tests passed, including PTY batch equivalence and resize exclusion | verified |
| Fix blob lease recursive mutex acquisition | hl images | 2 blob tests passed with all features | verified |
| Bounded decoded-image RAM cache and reduced WebP copies | hl images | 16 glyph/image tests passed; one existing benchmark ignored | verified |
| CoW render snapshot line contents | hl CoW | 57 all-feature and 55 no-default-feature surface tests passed | verified |
| Reduce full-frame copies across GUI/GPU process transport | root | 2 ownership-transfer and pool reuse tests passed | verified |
| Shared immutable process snapshots and lightweight cwd queries | root | all 16 procinfo tests passed, including shared storage, expiry/failure and no remote argv read for cwd | verified |
| Avoid unnecessary clipboard text clone | root | single destination moves; dual destination clones once then moves | verified |
| Reduce hot-path logging I/O without losing critical diagnostics | hl startup | 8 logger tests passed: backpressure, flush barriers, UTF-8 truncation and I/O failure | verified |
| Reduce serialized startup layout latency | hl startup | bounded isolated preparation, shared predecessor completion regression passed; non-isolated/UAC remain ordered | verified |
| Integration gates and runtime acceptance | root | scoped tests and strict Clippy passed; user confirmed glyph rain and downward cursor displacement fixed; upward-displacement follow-up has 85 passing terminal tests | verified; user confirmed input alignment and CJK rendering |
| Final commits and launch with the same ten-page Chinese fixture | root | committed changes; successful sccache build; normal-config launch with all ten page markers; user acceptance | complete |
| Rare wheel jump to history start | root | 2 viewport boundary/empty-history tests and 2 Windows wheel arithmetic tests passed | candidate fixed; runtime acceptance pending |

The preceding CJK fix is already committed separately. Unrelated checkpoints
and existing worktrees are not part of this remediation.

Remaining low-priority follow-up: a blank history row can expose the scrollbar
following window shrink; fresh tabs at the same size do not have that row.
