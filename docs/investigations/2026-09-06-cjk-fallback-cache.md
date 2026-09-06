# CJK repeated rendering: parsed fallback fonts were discarded

## Confirmed mechanism

`RustybuzzShaper::do_shape` dropped a successfully parsed font whenever its
first shaping run resolved entirely through later fallback fonts. The next
run loaded the same font again, including font bytes, metrics, the rustybuzz
face and shaping plans. Coverage misses on CJK-only rows are normal, so this
turned reusable setup into repeated work during frame construction.

The GUI's progressive row sweep defers rows when its frame preparation budget
is exhausted. Expensive shaping can therefore manifest as bands of text being
updated over successive frames. This explains a mechanism for the reported
visual symptom, but is not an end-to-end proof that every observed pause had
the same cause.

## Change and verification

Keep successfully parsed fonts and plans for the lifetime of the shaper.
Entries remain bounded by that shaper's font-handle list, and are freed with
the shaper. This trades some retained font memory for eliminating repeated
loads. Failed font loading and fallback order are unchanged.

The portable regression uses bundled fonts with disjoint script coverage.
It failed before the fix and passes afterwards; it checks retained face data
and unchanged glyph IDs, clusters, cell widths and horizontal positioning.

A manually invoked probe uses a system CJK font supplied through
`ONLYTERM_CJK_FONT`. With JetBrains Mono, Cascadia Mono and Microsoft YaHei:

| Same-machine debug probe, 100 CJK rows | Before | After |
|---|---:|---:|
| Shaping elapsed | 537.24 ms | 134.05 ms |
| Sum of unloaded slots after each row | 200 | 0 |

These timings are measurements of shaping, not FPS or a release-speed claim.
The structural cache-retention assertion is the regression gate.

63 font unit tests passed. The manual probe was run separately. Font Clippy
and workspace nightly rustfmt checks passed. GUI, CLI and mux-server were
built with the optimized `dev-install` profile.

## Interactive check

An isolated GUI instance displayed a UTF-8 file containing 500 lines, grouped
into ten pages, with thousands of different CJK characters. A screenshot
confirmed readable glyphs. A second output of the same file completed, and
the scrollback contained all twenty page headings and the shell prompt.

The first display included a 468.14 ms CPU frame-preparation sample. After
starting the repeat, the maximum of 389 subsequent preparation samples was
4.45 ms. Those samples also include idle repaints; they are not a scrolling
benchmark, GPU presentation timings, or a controlled before/after GUI test.
No warning/error messages were recorded in that repeat interval. Cold-start
font preparation remains a separate source of latency to investigate.

## Follow-up: scrolling reproduced stalls and crashes

The first improvement was insufficient: interactive scrolling subsequently
produced CPU frame preparation times of 100–196 ms. Both preview processes
crashed; Windows recorded exception `0xc0000409` (fatal application exit).
The diagnostic preview logged repeated `Queue::write_texture` errors stating
`Not enough memory left`, 19 same-size atlas recreations and a renderer reset.
These establish memory allocation failures preceding the crash; no application
dump stack was obtained to attribute the final abort to a particular allocation.

Two further defects were addressed:

* HostProcess atlas writes were both recorded for the child and uploaded on
  the parent's GPU queue. Wire frames do not submit draws on that queue, so
  those staging uploads could accumulate indefinitely. Mirrored writes now
  update only the bounded CPU replay log. Enabling mirroring submits the one
  initial clear queued before mirroring was enabled. In-process rendering
  continues uploading normally; backend recovery creates a fresh atlas.
* An atlas large enough for one viewport could repeatedly clear at the same
  size while scrolling through a larger character set. Small atlases now grow
  opportunistically up to 2048 square (16 MiB RGBA), before applying the
  existing eviction/retry policy. Larger frames retain the old growth retry.

A real-device regression destroys the parent texture before writing: mirrored
writes still deliver the correct replay pixels with no GPU validation error,
while non-mirrored writes produce the expected validation error. All 39 GPU
tests, two atlas retry tests, and GUI/GPU Clippy passed. The GPU subprocess
tests used a copy of the freshly built optimized GUI in their expected test
fixture location. Initial failures due to a missing fixture were resolved and
the full suite rerun successfully.

The rebuilt optimized GUI displayed the same file. Two full up/down scroll
cycles (720 wheel events, sent only to the preview window) completed without
GPU errors or a crash. After initial filling, parent private memory remained
approximately 933 MiB. The atlas grew once from 1024 to 2048 during scrolling,
with no subsequent recreations. Cold startup still recorded 283 ms and the
first scroll-time atlas growth 79 ms; this is not a claim that all latency is
eliminated. The fixed preview remains available for interactive validation.
