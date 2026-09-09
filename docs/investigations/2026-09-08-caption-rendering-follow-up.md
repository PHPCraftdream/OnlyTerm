# Centered caption: implementation and runtime verification

## Result

The CPU/RAM block is a separately painted caption element, not a string padded
with hundreds of spaces. Its desired center is the whole window's center.
The title is ellipsized when necessary; the status can use a complete compact
form, or disappear entirely if even that cannot fit. The closing bracket is
never intentionally ellipsized. Native caption buttons have a separate area.

The caption uses the system font family at the actual HWND DPI, at least 12pt.
The regular OS title stays a short `title — [CPU ...]` string for window switchers
and accessibility. Turning the option off restores the native caption.
The personal config was not changed; previews used process-local CLI overrides.

## Integration failures, now covered

1. **Successful GDI calls did not mean visible text.** Direct painting into the
   DWM-overlapping band returned a nonzero DrawText result and correct colors,
   but both the user and targeted captures showed an empty title. Rendering
   through a private 32-bit DIB and making its pixels opaque before transfer
   restored the text. GdiFlush synchronizes drawing before the CPU touches the
   bitmap. A native pixel test checks actual glyphs, the closing bracket, and
   opaque alpha, rather than only checking the input string.
2. **A one-pixel top inset produced a click-through phantom caption.** With a
   restored HWND starting at y=26, DWM reported its visible frame starting at y=3.
   The apparent caption above the HWND had no corresponding mouse region.
   Removing DefWindowProc from the calculation and trying different message
   forwarding did not by themselves fix this. The required condition is an
   exactly zero top client inset. Subsequent hover testing established that
   maximized windows also require zero, rather than a native resize-frame inset.
   Their offscreen resize padding belongs inside the caption's text layout.
   The band consumes the native top margin, preserving terminal content origin.
3. **The parent background obscured native buttons.** The extended-frame area
   must be cleared to black for DWM composition, rather than the ordinary
   window background. Both WM_PAINT and WM_ERASEBKGND clear only the exposed
   button area; they must not erase the opaque label child. Terminal invalidation
   also excludes the caption. Native bitmap/update-region tests check both
   boundaries. The user confirmed that this eliminated label flickering.
4. **Maximized buttons were visible but reported caption drag hits.** When DWM
   and the default procedure do not identify a button, WM_GETTITLEBARINFOEX
   supplies the native individual button rectangles. Their horizontal ranges
   are clipped to the client area and their hit regions cover the actual
   caption height, including the visible top edge. Hidden, unavailable, and
   offscreen buttons are excluded. This fallback repaired clicks, but did not
   restore native hover. A standalone DWM reference reproduced the real cause:
   retaining 8px above the maximized client makes DwmDefWindowProc return false;
   retaining 0px makes it return true with HTCLOSE. The fix keeps client top at
   zero in both states and moves maximized padding into text/icon placement.
   Native DWM handling takes precedence over the geometry fallback.
5. **Restore/maximize could overwrite the caption child.** Giving the child
   its own paint state removed its dependency on a reentrant `WindowInner`
   borrow, but did not by itself stop the disappearance. An on-screen A/B
   test then isolated missing `WS_CLIPCHILDREN`: without it, seven of thirty
   transitions lost all title glyph pixels; with it, none did. The parent now
   clips its children while the custom caption is enabled. A scalar property
   records ownership of this style bit, so disabling the caption removes only
   a bit this feature added, not a pre-existing one. This follows Microsoft's
   [parent/child clipping contract](https://learn.microsoft.com/en-us/windows/win32/winmsg/window-features#clipping).
6. **DWM can return button coordinates from the previous window size.** The
   reserved button width is validated against the current window edge, cached
   with its DPI, and anchored to the current client edge. Stale restored or
   maximized absolute coordinates cannot extend the label child over buttons.
   The geometry key includes maximized state, client offset and DPI; minimized
   windows hide the child and discard that key.

The zero-inset behavior has an independent, matching native reproduction in
[Simon Anciaux's custom-frame investigation](https://handmade.network/forums/articles/t/9073-custom_window_title_bar_and_almost_correctly_drawing_windows_10_borders)
(the `rect->top += 1` example). The general frame and button protocol follows
[Microsoft's custom-frame documentation](https://learn.microsoft.com/en-us/windows/win32/dwm/customframe).

The earlier claim that DWMWA_CAPTION_BUTTON_BOUNDS requires Windows 11 22H2
was incorrect. It is used with return-value/rectangle validation; the first
paint has a conservative DPI-scaled fallback until usable bounds are available.
That fallback is not used as the desired center.

## Runtime observations

Measured on a real preview HWND at 96 DPI, without changing installed binaries:

- Normal window: outer rectangle `(78,78)-(1318,619)`;
  DWM visible rectangle `(85,78)-(1311,612)`. Their top coordinates now agree.
- Caption band: y=78, height 31. GPU content: y=109, preserving the normal
  native client origin of 31 pixels below the outer top.
- Actual WM_NCHITTEST responses: minimize=8, maximize=9, close=20,
  caption drag=2, top resize=12. Before the zero-inset correction, all three
  button probes incorrectly returned 2.
- At 380px window width, a targeted bitmap capture showed title ellipsis and
  the complete compact `[CPU 1% / 0.0G / 0%]` block. At 360px it did not fit and
  was hidden whole. Restoring 1240px restored the full centered block.
- A maximized preview had outer rectangle `(-8,-8)-(1780,1088)`, visible rectangle
  `(0,0)-(1772,1080)`, a 31px caption child starting at y=-8 (23px visible),
  and GPU content starting at y=23.
  A targeted screen capture of its unobscured header showed all three native
  buttons and the centered status; no window raising or focus change was needed
  for that capture. Hit probes at y=3, 10, and 17 returned 8/9/20 over the
  corresponding buttons, rather than 2 (caption drag).
- The final optimized build passed 30 scripted window-state transitions,
  including three minimize/restore pairs. All 31 raw screen captures retained
  title, status and native-button pixels. The test did not add clipping itself;
  the application supplied it. Captures used screen copying, not PrintWindow
  or forced redraw, which can repaint the defect away before observing it.
- Hovering the final maximized build changed 935/900/945 pixels over the three
  buttons; the close hover contained 925 red pixels. Cursor position and focus
  were restored. Native hit tests still returned minimize=8, maximize=9,
  close=20, including the visible top edge.
- Latin and Cyrillic input stayed attached to the prompt through those
  transitions. See the
  [ConPTY output-extent follow-up](2026-09-06-conpty-cursor-after-shrink.md).
- Multi-monitor DPI transitions and fullscreen have not received runtime
  acceptance in this task; DPI/maximized arithmetic has unit coverage.

## Idle CPU

The repeated whole-system process enumeration in the pane's process-info
refresh was the first measured cost. Idle refresh now probes creation time,
execution cycles and liveness of known processes on a blocking worker. Activity,
uncertainty, PID reuse or a changed descendant set forces a full refresh. A
five-second deadline still discovers descendants that appear without observed
parent activity. Immediate/fresh-keyboard lookups bypass this optimization.

An epoch rejects obsolete background results. Shared snapshots carry their
start time: a snapshot that predates the activity probe cannot be accepted as
proof that the process tree is unchanged. Tests cover these races and failures,
including a process that exited with code 259 (not a live process).

Measurements used optimized builds, an empty CMD, an active foreground window,
ten seconds of warm-up and six quiet five-second intervals. CPU is summed over
all four processes, including the GPU host, and normalized to 16 logical CPUs.
Input-contaminated intervals are rejected; process-tree changes invalidate a run.

| Build / cursor | Mean CPU over 30 quiet seconds |
|---|---:|
| Before process-refresh optimization, original cursor settings | 0.899% |
| Optimized, original smooth blinking cursor, first run | 0.306% |
| Optimized, original cursor, repeated run with thread breakdown | 0.485% |
| Optimized, blinking disabled only in a separate diagnostic process | 0.062% |

The second optimized run attributed 0.254% to the GPU host and 0.231% to the GUI;
the sampled blocking worker used 0.020%. Without cursor animation the GPU host
consumed no measurable CPU time in that interval. These are observations, not
a guarantee of literal zero CPU. The normal preview retains the user's cursor
settings; no personal configuration was changed to obtain the normal results.

Claude's earlier missing colors were a launcher-environment issue:
`NO_COLOR=1` was inherited by the diagnostic process. Removing that variable
only for the launcher restored colors, as confirmed by the user. No renderer
color workaround or global environment change was introduced.

## Checks

- 416 tests passed: window 28, terminal 97, mux 87, procinfo 23, GUI 181.
  Four existing GUI manual/benchmark tests remained ignored.
- Clippy for all five packages, all targets, `-D warnings`: passed.
- Nightly formatting check and `git diff --check`: passed.
- Development and optimized `dev-install` GUI builds: passed.
  The combined test compilation initially exhausted Windows commit memory
  (error 1455). After closing only two obsolete, task-owned diagnostic windows,
  the complete default-feature suite passed with `CARGO_INCREMENTAL=0`, one
  Cargo job and `profile.test.package.onlyterm-gui.codegen-units=16`. These
  were command-local verification settings, not project profile changes.
  Native graphics tests are not Miri tests:
  Miri cannot execute these real Win32/GDI/DWM calls; the pixel test is excluded
  under `cfg(miri)`. No claim of Miri verification is made.
- No new registry dependencies, manual Send/Sync implementations, version bumps,
  commits, pushes or installations were made for this task.

## Native-resource audit

All new native objects stay on the owning GUI thread. Child paint state owns
its cached font; a thread-local registry holds an `Rc` until `WM_NCDESTROY`.
No Rust pointer is stored in a window property. A native test paints a caption
whose parent has no `WindowInner`, checks visible pixels, destroys the parent
and verifies that the registry releases its reference.
The child HWND is owned by Windows through its parent. Each paint owns a
private bitmap/DC pair. SaveDC/RestoreDC and BeginPaint/EndPaint have RAII guards;
the bitmap is deselected before DeleteObject/DeleteDC. Buffer size is checked
before allocation and capped at 32 MiB. Native creation failures fall back to
the ordinary caption. Callback panics cannot unwind across the system ABI;
unknown panic payloads are intentionally leaked instead of invoking unknown
destructors at that boundary.

The GDI bitmap allocation, DWORD alignment, deletion ownership and required
flush-before-direct-access follow
[CreateDIBSection's contract](https://learn.microsoft.com/en-us/windows/win32/api/wingdi/nf-wingdi-createdibsection).

Unsafe sites (line numbers at this revision; paths relative to
`crates/window/src/os/windows/window/`):

| File and lines | Contract checked |
|---|---|
| caption/mod.rs:33,40 | Forward original HWND/message arguments to DWM; initialized output result |
| caption/mod.rs:70,75 | Read-only geometry queries into initialized structures |
| caption/mod.rs:149 | Invalidate only the live parent-owned child |
| caption/mod.rs:162,166,170,173,179,185,191,202,204 | Composition, scalar properties, owned clipping-style bit and enable rollback |
| caption/mod.rs:226,228,240,242 | Stored caption height and valid client rectangle |
| caption/mod.rs:247,250,269,271 | Live parent DC, borrowed stock brush; button-only clear and content-only invalidation |
| caption/mod.rs:282,293,301,303 | Real WM_NCCALCSIZE payload; bounds before publication |
| caption/mod.rs:346,353 | Native messages and coordinate conversion |
| caption/mod.rs:421,423,426 | Initialized TITLEBARINFOEX; synchronous scalar query without a parent-state borrow |
| caption/mod.rs:469,471,475,483,493,576 | Live same-thread child visibility, creation, placement and geometry |
| caption/mod.rs:584,590 | Stable window class/procedure; hidden child registered before first display |
| caption/mod.rs:636,648,655,663,666 | ABI panic boundary; paint/print DC, destruction cleanup and unchanged default messages |
| caption/paint.rs:33,61,72 | Exactly-once font deletion, EndPaint and RestoreDC guards |
| caption/paint.rs:80,83,85,91 | Initialized PAINTSTRUCT and paired paint DC |
| caption/paint.rs:98,100,108,115,125 | Live child and parent; independent paint state and private surface |
| caption/paint.rs:134,137,140,143,160,167,173 | DPI-sized metrics, owned font, saved DC and borrowed stock objects |
| caption/paint.rs:268,272,281,289 | Bounded UTF-16 with explicit counts; no DT_MODIFYSTRING |
| caption/surface.rs:20,29,37 | Checked 32-bit DIB dimensions; owned allocation and initialization |
| caption/surface.rs:70,73 | GDI flush, exclusive pixel writes within allocation, opaque transfer |
| caption/surface.rs:98,116,122,210,216,316 | Deselect/delete ownership and native tests; private HWNDs destroyed by guards |

Integration calls in parent `window.rs` use the same live-HWND/thread contract
for geometry, setup, IME/mouse offsets, teardown, frame extension, background
painting and native-message dispatch: 586,679,930,1239 (setup/geometry);
1940 (IME); 2174,3204 (invalidation); 2495 (teardown); 2531 (NCCALCSIZE);
2880 (frame extension); 3158,3244 (background); 3181,3278 (placeholder bounds);
3390,3402,3448,3547 (mouse coordinates); 4305,4402 (native dispatch).

The additional native CPU probe in `crates/procinfo/src/windows.rs:632,637,645`
opens only query/synchronization rights, wraps the handle in `OwnedHandle`,
checks liveness without waiting and supplies initialized outputs to
`GetProcessTimes` / `QueryProcessCycleTime`. Every failure releases the handle
and reports uncertainty rather than idle. It neither suspends nor terminates
application processes.

Geometry logging is opt-in with
`ONLYTERM_LOG=window::os::windows::window::caption=debug`; no title content is logged.
