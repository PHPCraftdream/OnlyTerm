# Investigation: centering the CPU/RAM block in the OS window title (task #666)

Date: 2026-09-08. Status: **historical record of the abandoned padding attempts**.
The user subsequently authorized a separately painted caption; see
[implementation and runtime findings](2026-09-08-caption-rendering-follow-up.md).
The button-allowance explanation and Windows-version claim below were hypotheses,
not established causes. This record is preserved rather than rewritten as a success.
Ships on top of `60503d1c9` (the already-tagged `v0.0.24-alpha`
window-title CPU/RAM stats feature, task #665), which used a fixed `"  —  [...]"`
separator and is unaffected by anything in this document.

## Goal

Task #666: replace the fixed em-dash separator with dynamic space padding so the
`[CPU X% - RAM Y.Y GB - Z%]` block appears visually centered in the native OS window
title, recomputed on resize.

## Attempts, in order, with what was learned

### 1. Character-count approximation (first attempt)

`center_padding_spaces` (now `center_padding_spaces_by_char_count` in
`crates/onlyterm-gui/src/termwindow/process_stats.rs`) estimated padding by treating
every character — including the padding spaces themselves — as
`self.render_metrics.cell_size.width` pixels wide (the *terminal's* monospace cell
width).

**Result:** visually, almost no gap appeared even though the computed padding (52
spaces, confirmed via a temporary diagnostic log) should have been substantial.

**Root cause:** the OS title bar renders with a *proportional* font (not the
terminal's monospace font), and a space glyph in a proportional font is typically
2-4x narrower than a regular character. Assuming "1 space ≈ 1 terminal cell" (12px)
against a real space width of ~3px meant the real visual gap was only ~1/4 of what
the math intended.

### 2. Pixel-accurate measurement via the wrong font

Added `Window::measure_title_text_width` (`crates/window/src/os/windows/window.rs`)
using `GetTextExtentPoint32W` against a font obtained from the existing
`get_title_log_font(hwnd, hdc)` helper (already in this file, used elsewhere for
`TITLE_FONT`/`update_title_font`). `center_padding_spaces_by_pixel_width` was added
to compute padding from real measured pixel widths (title/block/space) instead of
assumed character counts.

**Result:** padding jumped to 225 spaces — so much that the block was pushed
**entirely off-screen** (invisible), because the calculation centered against the
*full* client width (`self.dimensions.pixel_width`) with no allowance for the
window icon and the minimize/maximize/close button cluster, which occupy real space
in the title bar but aren't part of the client-area width.

### 3. Reserve a fixed allowance for the button cluster

Added `TITLE_BAR_BUTTON_CLUSTER_ALLOWANCE_PX = 160` and clamped the computed block
position so it can never land past `window_px - 160`, in both the pixel-based and
char-count-based functions. Added a regression test
(`pixel_width_block_never_lands_past_the_button_cluster_allowance`) asserting this
invariant directly.

**Result:** the block became visible again, but **truncated** — the trailing `0%]`
was cut off on screen — and the user reported it still didn't look centered.

**Root cause found:** `Get-Process | Select MainWindowTitle` (used to "verify" the
fix after each build) reads the string via `GetWindowText`, which returns *exactly*
what was passed to `SetWindowTextW` — it says nothing about whether the OS's
non-client caption painting actually has room to *draw* all of it. The verification
method itself was unable to detect the truncation; only an actual screenshot did.
The 160px allowance was evidently a bit too small for this system's real button
cluster width (Windows 11, default theme) — off by roughly the width of `" 0%]"`.

### 4. Switch measurement to the correct system caption font

Suspected `get_title_log_font` was measuring the wrong font: it asks the `"HEADER"`
theme class (`OpenThemeData`/`GetThemeFont(HP_HEADERITEM, ...)`), which is the
Explorer/list-view *column header* font — a different UI element that happens to
share a `TMT_CAPTIONFONT` constant name, not necessarily what `DefWindowProc`'s
non-client caption painting actually renders with. Added
`get_system_caption_log_font()` using `SystemParametersInfoW(SPI_GETNONCLIENTMETRICS)`
→ `NONCLIENTMETRICSW.lfCaptionFont`, the documented way to get the exact native
caption font, and switched `measure_title_text_width` to use it.

**Result:** identical measured widths (`title_w=54, block_w=150, space_w=3`) to
before switching fonts. On this system/theme, the two fonts apparently coincide —
so this was a legitimate fix to make (measuring against the officially-documented
caption font instead of an unrelated theme part is more correct on principle,
independent of this specific system's outcome), but it did not explain the observed
truncation. The truncation is still attributed to attempt 3's fixed allowance being
too small for this system's real button-cluster-plus-margin width, not to a font
mismatch.

## Where this leaves things

- The right edge of the actually-drawable native caption area is not reliably
  obtainable from plain Win32 APIs. `SPI_GETNONCLIENTMETRICS` describes the *font*,
  not the caption *rectangle*; the button cluster's width varies by Windows version,
  DPI, theme (classic vs. Fluent/rounded corners on Windows 11), and is not exposed
  as a simple queryable pixel count pre-Windows-11. The one precise API for this is
  `DwmGetWindowAttribute(DWMWA_CAPTION_BUTTON_BOUNDS)`, which only exists on
  Windows 11 22H2+ and needs a documented fallback for everything older (or for
  non-DWM-composited sessions).
- A fixed pixel allowance (currently 160, evidently a little low on this system) is
  inherently an approximation and will always be somewhat wrong on *some*
  Windows version/theme/DPI combination — it can be tuned empirically, but "exactly
  centered, never clipped, on every system" is not achievable with a constant.
- `GetWindowText`/PowerShell's `MainWindowTitle` cannot verify visual truncation —
  only a real screenshot (or a Win32 call that asks the OS to actually measure/paint
  the caption, if one exists) can. This cost several iterations before being
  noticed.

## Options going forward (not decided — user asked to pause here)

1. **Bump the allowance** (e.g. to ~250px) and accept "reliably visible, close to
   center, biased left of true center to clear the buttons" rather than exact
   centering.
2. **Query `DWMWA_CAPTION_BUTTON_BOUNDS`** for a precise right-edge boundary on
   Windows 11 22H2+, falling back to the empirical constant everywhere else — more
   correct, more code, another OS-version branch to maintain.
3. **Drop the centering goal entirely** — keep the block at a fixed, safe offset
   (e.g. right-aligned a fixed distance before the button cluster, not
   mathematically centered) — simpler and robust, but doesn't visually "float to
   the middle" as originally requested.

## Current uncommitted state (as of pausing)

Working tree (on top of `60503d1c9`, uncommitted, **not yet reverted or cleaned up**):

- `crates/onlyterm-gui/src/termwindow/actions.rs` — pixel-measurement-with-fallback
  title-join logic, **including a temporary `log::info!("diag: title centering
  ...")` line** that should be removed before this is considered ready to commit.
- `crates/onlyterm-gui/src/termwindow/process_stats.rs` —
  `center_padding_spaces_by_pixel_width`/`center_padding_spaces_by_char_count` (both
  with the button-cluster allowance and clamp), 13 passing unit tests.
- `crates/window/Cargo.toml` — added the `wingdi` winapi feature.
- `crates/window/src/os/windows/window.rs` — `Window::measure_title_text_width` and
  `get_system_caption_log_font`.
- `docs/config/reference/config/show_process_tree_stats_in_title.md` — example
  updated to show padding instead of the em dash (now stale relative to what's
  actually shipped in `v0.0.24-alpha`; should be reverted or updated depending on
  which approach ships).

All of the above builds clean (`cargo build`), passes `cargo clippy -p onlyterm-gui
-p window --all-targets -- -D warnings`, passes `cargo +nightly fmt --all -- --check`,
and passes `cargo test -p onlyterm-gui process_stats` (13/13). The code is not
*broken* — it visibly improves on the original em-dash version in most window
sizes — it just doesn't yet achieve reliable, non-clipped, visually-centered
placement on this specific system.
