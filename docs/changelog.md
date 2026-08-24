---
hide:
    - navigation
toc_depth: 3
---

<p style="display:none">
changelog
</p>

## Changes

Releases are named using the date, time and git commit hash.

### Continuous/Nightly

A bleeding edge build is produced continually (as commits are made, and at
least a daily scheduled build) from the `main` branch.  It *may* not be usable
and the feature set may change, but since @wez uses this as a daily driver, its
usually the best available version.

As features stabilize some brief notes about them will accumulate here.

#### Changed
* **Breaking**: the rhai scripting engine has been removed entirely, along
  with every `lua-api-crates/*` crate that backed it. The configuration
  language is now **ktav**, a static, engine-free `key: value` data format
  (no expressions, no function calls, no `on(...)` event hooks) — your config
  file must now be a `.onlyterm.ktav` file (previously `.wezterm.rhai`, and
  before that `.wezterm.lua`), found either as `~/.onlyterm.ktav` or as
  `onlyterm.ktav` in one of your config directories (e.g.
  `~/.config/wezterm/onlyterm.ktav`). This is a deliberate simplification, not a
  stopgap: an audit found that most of the scripting surface was already
  unreachable dead code (an earlier, incomplete Lua-to-rhai migration had
  silently dropped the wiring for most event hooks and callback plumbing), so
  rather than carry a scripting engine forward mainly to evaluate static data,
  OnlyTerm now loads config as data, directly. All rhai-only event
  hooks/callbacks — `format-tab-title`, `format-window-title`,
  `update-status`, `update-right-status`, `window-config-reloaded`,
  `window-focus-changed`, `bell`, `user-var-changed`, `open-uri`,
  `gui-startup`, `gui-attached`, `new-tab-button-click`,
  `augment-command-palette`, the `mux-startup`/`mux-is-process-stateful` mux
  events, the debug overlay's rhai REPL, and the whole scripting API surface
  under `wezterm.*`/`window.*`/`pane.*` — have been removed with **no
  scripting replacement**; the underlying default/built-in behavior that each
  of them used to be able to override (tab titles, window titles, status
  text, bell handling, and so on) is unchanged. `ExecDomain`'s `fixup`/`label`
  callbacks are gone for the same reason: an `ExecDomain` can no longer wrap
  spawned commands. **Because that makes `exec_domains` actively unsafe to
  load quietly** (it would silently spawn commands un-wrapped, directly on
  the host, instead of doing whatever the domain used to do, e.g. `docker
  exec`/`ssh`), a config whose `exec_domains` is non-empty now fails to load
  with a clear error at config-load time rather than loading and spawning
  the wrong thing; remove the `exec_domains` entries from your config
  (`wsl_domains` remains fully supported, since it is implemented natively
  rather than via scripting, and is the closest still-working alternative
  for reaching WSL). Plugins (which required a scripting engine to evaluate
  `plugin/init.rhai`) are also gone. If a legacy `onlyterm.rhai`/`onlyterm.lua`
  (or, for the dotfile location, `.onlyterm.rhai`/`.onlyterm.lua`) is found
  with no `.onlyterm.ktav` sibling, OnlyTerm prints a clear error naming the
  file and pointing at the migration guide. A separate, explicit
  external-hooks API may be added later if a real need for scripted
  customization emerges — this removal is not a signal that hooks can never
  come back, only that the old rhai-shaped mechanism is gone. See the
  [migration guide](migration-to-ktav.md) for the ktav data-format syntax and
  a side-by-side translation of real configs.
* **Breaking**: identity strings that still said "WezTerm"/"wezterm" in
  places a program or script outside OnlyTerm could actually observe are now
  "OnlyTerm"/"onlyterm". This project is a full, permanent split from
  upstream wezterm, so these are deliberate, unapologetic renames, not
  oversights being walked back — no upstream compatibility is preserved.
  Affected: the `TERM_PROGRAM` environment variable set on every spawned
  shell/child process is now `OnlyTerm` (was `WezTerm`); the default Win32
  window class / Wayland app_id / Linux D-Bus notification app id is now
  `org.onlyterm.onlyterm` (was `org.wezfurlong.wezterm`, configurable via
  `--class` same as before); the embedded terminal engine now reports itself
  as `OnlyTerm` rather than `WezTerm` for `TERM_PROGRAM`-style self-identification
  and iTerm2-image-protocol capability probing (a nested real upstream
  wezterm session is still recognized for that specific capability check, so
  this doesn't regress running actual wezterm inside OnlyTerm). If you have
  scripts, prompt themes, or tooling that specifically checked for
  `TERM_PROGRAM=WezTerm` or the old window class, update them to look for
  the new values instead.
* Unlike `exec_domains` above, the
  [PromptInputLine](config/reference/keyassignment/PromptInputLine.md),
  [InputSelector](config/reference/keyassignment/InputSelector.md), and
  [Confirmation](config/reference/keyassignment/Confirmation.md) key
  assignments' `action` callbacks, and the
  [EmitEvent](config/reference/keyassignment/EmitEvent.md) action, still load
  without any error — they just silently do nothing (show/dismiss an overlay
  with no handler left to receive the result, or emit an event nobody is
  listening for) now that the rhai callback registry they depended on is
  gone. These are documented as non-functional on their individual reference
  pages; they were left as silent no-ops rather than load-time errors because,
  unlike `exec_domains`, a missing callback here does not cause an actively
  wrong or dangerous outcome, just a missing one.
* Audited every `unsafe` block in the codebase (~85 files across all crates)
  and either removed it in favor of safe Rust where it wasn't strictly
  necessary, or documented it with a `// SAFETY:` comment explaining the
  invariant that makes it sound. Fixed several genuine latent
  undefined-behavior bugs found in the process (an overlapping-region
  `ptr::copy_nonoverlapping` in the mux wire codec, two `_unchecked`
  conversions on unvalidated input, an uninitialized-memory `Vec::set_len`,
  a NaN value bypassing a color-conversion clamp and indexing out of
  bounds, and several silently-ignored partial `Write::write` calls that
  could truncate a terminal's response to its child process). An
  independent review pass caught and corrected several inaccurate
  `// SAFETY:` justifications that the initial audit had left in place
  (a couple of them describing an invariant the code didn't actually
  hold). `#![warn(clippy::undocumented_unsafe_blocks)]` is now enabled in
  every affected crate, and `cargo clippy --workspace` runs clean end to
  end with zero occurrences of that lint, so future unsafe code without a
  safety comment is caught automatically.
* Replaced the `zstd`/`zstd-sys` dependency (the project's last remaining
  C/C++ dependency) with the pure-Rust `flate2` crate, then disabled mux
  protocol compression entirely: a real-unix-socket benchmark showed
  sending PDUs raw is faster end-to-end than compressing them on typical
  hardware, even though compression shrinks the payload by up to ~47x.
  `flate2` decoding is kept only so a peer that still compresses (e.g. an
  older client) remains readable. OnlyTerm is now a 100% Rust project.
* Hebrew niqqud (vowel points) and cantillation marks are now stripped
  from rendering instead of being drawn as combining marks, since the
  glyph stacking for these diacritics was unreliable across fonts;
  Hebrew consonants, punctuation (maqaf, paseq, sof pasuq, geresh/
  gershayim) and all other scripts are unaffected.
* Fixed a Hebrew rendering bug where, in a line containing several
  Hebrew words, each word's letters were correctly reversed
  right-to-left but the words themselves stayed in typed (left-to-right)
  order; consecutive Hebrew words glued by spaces/punctuation now
  reverse together as a single block, matching standard bidi behavior.
  This also fixes the case where such a multi-word phrase is split by a
  line wrap: previously the wrap-boundary heuristic left *both* halves
  completely unreversed instead of correctly reversing each row's
  content independently.
* Hebrew/RTL rendering no longer runs the full Unicode Bidirectional
  Algorithm: a line is always laid out left-to-right from column 0, and
  only the cell order *within* a maximal run of Hebrew letters (glued
  together by spaces, geresh/gershayim, maqaf, or any other neutral
  punctuation that Hebrew resumes after) is reversed. Digits, brackets,
  quotes, dashes and Latin/Cyrillic text are never moved or mirrored, so
  the terminal cursor and selection always track the typed column
  regardless of script direction. A Hebrew run is reversed the same way
  regardless of whether it touches a physical line-wrap boundary, since
  each visual row is laid out independently (see the wrap-boundary fix
  above).
* Fixed backgrounds/underlines being drawn under the wrong glyphs on lines
  containing a reversed Hebrew phrase: per-cluster decorations now use the
  cluster's actual on-screen column instead of its pre-reversal logical
  cell index, which stopped being monotonic once Hebrew phrases started
  reordering cells in place.
* `ALT` and `CTRL` key combinations (eg. `ALT-V`, `CTRL-V`) now resolve to
  the physical, layout-independent key rather than whatever character the
  active Windows keyboard layout maps that physical key to (eg. physical
  `V` no longer resolves to Cyrillic `м` under a Russian layout), both for
  wezterm's own keybinding lookup and for the `win32-input-mode` byte
  sent to the child process.
* `CTRL-C`'s default binding (copy-selection-or-interrupt) and the new
  `CTRL-Enter`/`SHIFT-Enter` newline bindings now encode through whatever
  keyboard protocol the pane's app has actually negotiated
  (`win32-input-mode` or the kitty keyboard protocol), instead of always
  writing a hardcoded legacy byte; apps that haven't negotiated either
  still get the same legacy byte as before.
* `enable_kitty_keyboard` defaults to `true` again after two earlier
  reverts; the actual root cause of the previous Ctrl+C breakage (the
  hardcoded legacy byte above) is now fixed.
* The application icon is now cropped to a circle with a transparent
  background outside it, instead of a solid square.
* Hebrew/RTL support: bundled [Cascadia Mono](https://github.com/microsoft/cascadia-code)
  (SIL OFL 1.1, genuinely monospaced Hebrew consonants) as an automatic
  fallback font, and `bidi_enabled`/`bidi_direction` now default to `true`/
  `AutoLeftToRight` so right-to-left text renders correctly out of the box
  instead of requiring manual configuration. The baseline font stays
  JetBrains Mono. Cascadia Mono doesn't cover cantillation marks or some
  niqqud (those render as `.notdef`); no secondary Hebrew fallback font is
  chained in, to avoid mixing metrics from two different Hebrew fonts in
  the same word.
* Fixed several bidi/shaping bugs that produced duplicated punctuation,
  garbled letter spacing and, in one case, a crash when rendering
  multi-word right-to-left text: bidi resolution now runs across the whole
  line instead of per attribute-run, bidi run reconstruction uses the
  exact (deduplicated) codepoint list instead of an unsafe numeric range,
  and the shaper's cluster resolver now correctly converts rustybuzz's
  cluster offsets (relative to the shaped substring) to absolute offsets
  when recursing into a fallback font mid-line.
* GPU context loss (driver TDR) now tears down the affected window's panes
  and their child processes before closing the window, instead of leaving
  them orphaned.
* `enable_scroll_bar` now defaults to `true`; the scrollbar's auto-sized
  width goes from 1 cell to 1.75 cells and its minimum thumb height from
  0.5 cell to 2 cells, so it stays visible and easy to grab.
* Fixed a regression where releasing the mouse button after dragging out a
  text selection immediately cleared it; the selection now stays visible
  until you click elsewhere, right-click it, or press `CTRL-C`.
* `CTRL-V` now has a default paste-from-clipboard binding on Windows (it
  previously only worked via `SHIFT-Insert` or the OS-level paste gesture).
* Hardened the text shaper against a crash ("byte index is not a char
  boundary") that could occur while rendering certain Hebrew text; the
  affected code path now clamps to the nearest valid character boundary
  and logs a warning instead of panicking.
* The application icon's arrow is now a darker, higher-contrast green.
* OnlyTerm defaults: a light, GitHub-style color scheme (including the
  tab-bar/titlebar chrome) instead of upstream's dark defaults;
  `use_cwd_basename_as_tab_title` is on, so tab and window titles track the
  active pane's current directory and update live as you `cd`; new windows
  start maximized (`start_maximized`); `window_close_confirmation` is
  `NeverPrompt` and the close-confirmation overlays for panes/tabs/windows
  have been removed from the code entirely, not just defaulted off; on
  Windows, `default_prog` defaults to `cmd.exe` instead of following
  whatever `ComSpec` happens to resolve to; and
  [sefer-alloc](https://github.com/PHPCraftdream/sefer-alloc) is installed
  as the process's global allocator.
* `CTRL-C` now copies the pane's selection (and clears it) when there is
  one, otherwise it's passed straight through as a literal interrupt byte -
  it always works to interrupt a running program when nothing is selected.
  Right-clicking a text selection does the same (copy and clear) when
  there's no hyperlink under the cursor.
* **Breaking**: the built-in SSH client (`wezterm ssh`, `SshDomain`/`ssh_domains`,
  `ssh_backend`) and the TLS-mux remote-multiplexing feature
  (`TlsDomainClient`/`TlsDomainServer`, `tls_clients`/`tls_servers`,
  `wezterm cli tlscreds`) have been removed entirely, along with their
  `openssl`/`ssh2`/`libssh-rs` dependencies. Local multiplexing via
  [unix domains](multiplexing.md#unix-domains) is unaffected. If you relied on
  SSH domains or TLS-mux, use a system `ssh` client together with a local
  unix domain instead.
* **Breaking**: the configuration language has switched from **Lua** (the
  embedded `mlua`/LuaJIT engine) to [**rhai**](https://rhai.rs/) (a pure-Rust
  scripting language), and `mlua`/`luahelper` have been removed from the
  workspace. Your config file must now be a `.wezterm.rhai` file (previously
  `.wezterm.lua`). The configuration *schema* — every option, event name and
  key-assignment action — is unchanged, but the *syntax* is different (rhai is
  Lua-like but not Lua: `#{}` object maps instead of tables, `let` instead of
  `local`, `//` comments instead of `--`, `+` instead of `..`, actions written
  as tagged maps like `#{ SpawnCommandInNewTab: #{ cwd: "/tmp" } }` instead of
  `wezterm.action.SpawnCommandInNewTab{...}`, etc.). If a legacy `.wezterm.lua`
  is found with no `.wezterm.rhai` sibling, WezTerm prints a clear error naming
  the file to migrate. Plugins must now be rhai (`plugin/init.rhai`, not
  `plugin/init.lua`) and required by local path via `plugin::require(path)` —
  git-URL plugin installation was already removed separately. See the
  [migration guide](migration-to-ktav.md) (the Lua/rhai-specific version of
  this guide was superseded when the config format subsequently moved to
  ktav; see the entry above) for a side-by-side translation of real configs.
* DECRQCRA is now disabled by default to prevent silent screen scraping.
  Set `enable_checksum_rectangular_area = true` to re-enable it.
  Thanks to @jquast! #7701
* Wayland: currently being reimplemented, it maybe more unstable than usual.
  Please file GH issues for any problems you see.
  Many thanks to @tzx and @tmccombs! #4777 #5781
* [show_update_window](config/reference/config/show_update_window.md) has been
  deprecated; it no longer has any effect and will be removed in a future
  release.
* X11: drag and drop is now supported for files, URLs and text. Thanks to
  @ssiegel! #5316 #640
* Added Unicode Symbols for Legacy Computing to the set of pixel-perfect block
  drawing glyphs. See
  [custom_block_glyphs](config/reference/config/custom_block_glyphs.md) for more
  details. Thanks to @stribor14! #5051 #5169
* Switched to the [nucleo](https://github.com/helix-editor/nucleo) fuzzy
  matcher which produces matches that more closely match the popular `fzf`
  program. #5532
* The Copy Mode `Close` action no longer implicitly scrolls to the bottom.
  This is to facilitate having a key assignment that closes copy mode without
  adjusting the viewport position. You can compose multiple actions together using
  `Multiple` if you wish; the default key assignments in Copy Mode use this technique
  so that the effective behavior of the defaults remains unchanged.
  Thanks to @LeszekSwirski! #4924 #3502
* Improved startup performance on X11. Thanks to @blukai! #5923 #5802
* There is now an upper bound of 999,999,999 for `scrollback_lines`. Thanks to
  @x3ro! #5996
* Migrated serial support to the `serial2` rust crate. This opens the door
  to more convenient serial support going forward. Thanks to @jeevithakannan2!
  #6411 #6460
* macOS: The wezterm terminfo file is now compiled and bundled in the
  application bundle. Thanks to @ddeville! #6538
* `wezterm record` now has a `-o outputfile` option. Thanks to @Tyarel8! #6626
* `ShowTabNavigator` now defaults to selecting the active tab. Thanks to
  @mgpinf! #6320
* macOS: toast notifications now use UNUserNotificationCenter. This requires
  that WezTerm.app be code-signed, which is the case for official binaries.
* [ShowLauncherArgs](config/reference/keyassignment/ShowLauncherArgs.md) now allows
  customizing the help text. Thanks to @mgpinf! #6606
* Preliminary support for ConEmu style progress escape sequences. See
  [pane:get_progress()](config/reference/pane/get_progress.md) for more information.
  #6581
* [InputSelector](config/reference/keyassignment/InputSelector.md) now allows
  setting `input_selector_label_bg` and `input_selector_label_fg` colors in
  the `colors` section of your configuration.  Thanks to @mgpinf! #6682
* `wezterm imgcat --hold` now avoids local echo and accepts pressing `Escape`,
  `CTRL-C` and `CTRL-D` as various ways of exiting hold mode. Thanks to
  @mgpinf! #6801
* windows: Improve detection of running in WSL. Thanks to @bew! #7137
* [QuickSelect](quickselect.md) mode now hides non-matching labels as you type, making it
  easier to spot the remaining candidates. Thanks to @mr-felixoid and @bew! #7752
* Decoupled execution so one hung tab/window can no longer take down the
  rest of the process (previously a single stuck GPU present call would
  freeze every window's message loop, since all windows in a process share
  one, and rendering ran synchronously inside it). A background watchdog
  thread now detects when the GUI message loop's
  heartbeat stalls past a configurable threshold
  (`gui_watchdog_enabled`/`gui_watchdog_threshold_ms`) and exposes that
  state for future use; the per-frame line-shaping/quad-building pass no
  longer holds each pane's terminal lock for its whole duration, only for
  a quick snapshot of the visible lines, so a slow render no longer also
  stalls that pane's pty reader; and mouse hit-testing now reads a
  lock-free snapshot of the UI's clickable regions instead of the buffer
  the render pass is actively rebuilding. GPU frame submission (present/
  swapchain) has since moved off the shared message loop onto a dedicated
  per-window thread on Windows -- see the next bullet.
* **Breaking**: OpenGL has been removed entirely, along with the `Software`
  [front_end](config/reference/config/front_end.md) (which turned out to be
  GL/Mesa-backed, not an independent renderer) and the automatic fallback
  from WebGpu to OpenGL on adapter/device initialization failure. This
  removes the `glium`/EGL/WGL code paths from both `wezterm-gui` and the
  `window` crate and the bundled ANGLE/Mesa DLLs that backed them
  (~42MB). WebGpu is now the only rendering backend. A config that still
  sets `front_end` to `OpenGL` or `Software` continues to load -- it now
  prints a warning and is treated as `WebGpu` -- so no config change is
  strictly required, but the setting should be removed (or changed to
  `WebGpu`) to silence the warning. If WebGpu's adapter/device
  initialization fails outright (e.g. a VM without GPU passthrough, certain
  RDP sessions, or a driver mismatch), OnlyTerm now reports a clear,
  actionable error explaining what went wrong, instead of silently
  degrading to a different renderer or leaving a blank window on screen.
  What happens next is decided at the moment the failure is handled by
  whether any other window of this process is already up and running: the
  first live window of the process (in practice, usually the one opened at
  startup, but formally whichever window hits this failure while no sibling
  window has finished opening yet) cannot usefully continue without it, so
  it exits cleanly, the same as any other fatal startup error. Any other
  window that already has at least one sibling window up by the time it
  hits this failure -- e.g. a later window opened via `SpawnWindow`, a new
  window on a second monitor driven by a different GPU adapter, or a
  non-first window from a multi-window session restore -- fails to open on
  its own instead: the error is still reported to the user, the placeholder
  window it had already shown is closed rather than left stranded on
  screen, and the rest of the process and all of its other windows, tabs,
  panes, and their child processes keep running untouched. WebGpu's
  dedicated per-window render thread
  (`webgpu_render_thread`) remains enabled by default, so a stuck GPU
  driver call on Windows still does not freeze the whole process's message
  loop.
* If a WebGpu window's render thread does get stuck in a GPU submit,
  reconfigure, or surface-error call for too long (or the GPU device itself
  is lost, e.g. a driver reset), that window's entire WebGpu renderer
  (device, surface and render thread) is now torn down and rebuilt in place
  automatically -- the window and every one of its tabs/panes and their
  child processes survive, at the cost of one dropped frame. This replaces
  the previous behavior of closing the window outright. If the renderer
  keeps failing to recover (3 rebuild attempts within 30 seconds), that one
  window is now closed cleanly instead, with its panes' child processes
  cleaned up first as before -- there is no longer an OpenGL backend for it
  to fall back to. Manually verified end-to-end on real hardware,
  reproduced identically across multiple independent runs, using the
  `debug_render_thread_stall_ms`/`render_thread_hang_threshold_ms` debug
  config options.
* Hardened the above renderer-rebuild path against a handful of latent
  bugs, none of which were observed causing a user-visible failure but
  which weakened the recovery guarantee: a failed child-window
  re-creation during rebuild could leave a destroyed (and
  Windows-recyclable) window handle in place, later targeted by resize/
  move/DPI-change calls; the old child window is now retired (hidden,
  not destroyed) until the previous renderer state has fully dropped on
  its own thread, instead of being destroyed out from under a
  possibly-still-live GPU surface; and the hang-check supervisor that
  watches for a stuck render thread could end up running two overlapping
  timer chains for the same window under an unlucky timing window
  between a hang being handled and the rebuilt renderer coming back
  online -- no longer possible now that it's guarded structurally instead
  of by timing (previously this was harmless in practice because the
  OpenGL-fallback path collapsed any duplicates, but that safety net is
  gone along with OpenGL itself, so the structural guard now carries the
  whole guarantee on its own). Also: the vertex-buffer rotation scheme
  originally inherited from the (now-removed) OpenGL backend (three
  buffers per layer, rotated frame to frame to avoid writing into one the
  GPU might still be reading) bought nothing on WebGpu, where a fresh
  buffer is already created every frame -- WebGpu windows keep one buffer
  per layer instead of three, saving a few MB of GPU-visible memory per
  window with no behavior change.
* A background pane whose terminal is genuinely wedged (a full/unread pty
  pipe, a stuck escape-sequence handler, or similar) can no longer stall
  the GUI thread -- and by extension every window in the process -- while
  its tab bar entry, title, or working-directory are being refreshed.
  Reading a pane's unseen-output/title/progress/user-vars/cwd, writing to
  its pty (paste, sent text, etc.), and refreshing its foreground-process
  info now either complete immediately or fall back to the last known-good
  value within a bounded, short timeout instead of blocking. A background
  tab in this state is now marked as unresponsive (available to
  `format-tab-title`/`format-window-title` scripts). Verified with a
  regression test driving two real panes side by side: a wedged pane's own
  recovery and a healthy, unrelated pane's normal operation, proving the
  two don't contend with each other.
* Building the currently-active tab's on-screen content now has a time
  budget; if an unusually large or slow-to-shape amount of visible content
  would otherwise take too long on a single frame, the remainder degrades
  to the previous frame's already-rendered content (or is skipped for that
  one frame) instead of stalling the window.
* A stuck GUI message loop (detected by the existing watchdog thread) now
  shows a toast notification instead of only being logged silently.
* Documented a supported configuration for running OnlyTerm's GUI against a
  separate, headless `onlyterm-mux-server` daemon instead of each GUI
  window embedding its own multiplexer: killing or crashing a GUI window
  process no longer kills the programs running in its panes, since they
  keep running in the daemon and reattach the next time any GUI process
  connects. See [Surviving a crashed or killed GUI
  process](multiplexing.md#surviving-a-crashed-or-killed-gui-process) for
  the recipe and its known limitations (e.g. tab titles that depend on the
  foreground process name don't work across this boundary yet).
* A hung/wedged child process invoked by a config script (`run_child_process`,
  used e.g. by status-bar scripts that shell out to `git`/`kubectl`/etc.) can
  no longer block the GUI thread forever; it's now bounded by
  [child_process_timeout_ms](config/reference/config/child_process_timeout_ms.md)
  (default 3 seconds), after which the child is killed and the script call
  returns an error instead of hanging.
* Reduced the pty reader thread's read buffer (and the socket buffer sizes
  it requested) from 1 MiB to 64 KiB per pane, saving roughly 1 MB of
  resident memory per pane on Windows with no measurable effect on
  throughput.
* The pty reader and output-parser threads now hand output to each other
  via an in-process channel instead of a loopback TCP socket pair (which
  `socketpair()` falls back to on Windows, since there's no true anonymous
  socketpair primitive) -- lower latency, and one fewer OS socket per pane
  on Windows.
* Fixed a local privilege/race issue in the Windows `socketpair()`
  emulation (used internally for pty plumbing): the loopback TCP port it
  briefly listens on could, in principle, be connected to by another local
  process racing to get there first (the same class of issue as Python's
  historical `socket.socketpair()` emulation on Windows). The accepted
  connection is now verified with a random handshake token before it's
  trusted.
* Closing a tab/pane now sends a `Ctrl+C`-equivalent interrupt (the same
  byte your physical Ctrl+C writes) before force-terminating its process
  tree, and the pty is kept alive briefly afterward instead of tearing
  down immediately, so well-behaved processes have a real window to shut
  down cleanly first. The existing hard-kill behavior is unchanged: after
  the grace period, the process and every descendant it spawned (not just
  its immediate child) are still force-terminated via the same Windows Job
  Object cleanup as before. Manually verified end-to-end with a real
  process tree (a script and a grandchild it spawned): both were
  previously being torn down within about a second of closing the tab,
  well before the intended grace period, due to a bug where the pty's
  underlying pipe closed immediately regardless of the deferral; fixed,
  and now both processes correctly survive for the full grace period
  before being cleaned up. Whether the interrupt itself is actually
  observed by child processes as Ctrl+C could not be confirmed in manual
  testing and remains an open question -- the grace-period and
  whole-process-tree cleanup guarantees hold either way.
* Fixed [colors.indexed](config/reference/config/colors.md) (and any other
  integer-keyed map in the config surface) failing to load under **ktav**
  with an opaque `Cannot convert String to u8` error. ktav object keys are
  always strings (e.g. `indexed: { 136: #af8700 }` produces the string key
  `"136"`, not the integer `136`), which the old rhai config path didn't --
  a rhai object literal's numeric-looking keys arrived as real integers.
  Integer-keyed maps now also accept a string key that parses cleanly as
  the target integer type, so the documented `colors.indexed` syntax works
  again; a key that isn't a valid integer still produces the original
  error.
* Fixed toast notifications (e.g. the "couldn't find a glyph for this font"
  warning) being attributed to WezTerm's name and icon instead of
  OnlyTerm's on Windows. The process already announces itself to Windows
  under the `org.wezfurlong.onlyterm` AppUserModelID, and the installer
  registers the Start Menu shortcut under that same ID -- but the toast
  code itself still created its notifier under the old, un-renamed
  `org.wezfurlong.wezterm` ID, so Windows couldn't resolve a shortcut for
  it and fell back to unrelated app info. One-line fix; the equivalent
  Linux D-Bus path carries the same stale ID but was left alone, since
  this fork is Windows-focused and the report was Windows-only.
* `cargo clippy --workspace --all-targets` now runs with zero warnings for
  the first time in this fork's history (roughly 1,300 lints cleared across
  every crate). Along the way, a local `cargo lint` command (`xtask`) was
  added as the one-stop formatting/clippy/compile check used to get and
  keep the workspace there; its own scoping had a blind spot where `cargo
  lint -p <crate>` resolved that crate's Cargo features in isolation and
  silently missed code gated behind a feature only some other workspace
  member enables (`wezterm-cell`'s image-attachment field, hidden behind
  `use_image`, was the crate that exposed this), so the lint pass is now
  always workspace-wide and `-p` only filters which directory's lints are
  reported. A few of the larger cleanups are worth calling out on their
  own: `wezterm-input-types`'s hand-written `impl ToString` for `KeyCode`,
  `Modifiers`, `PhysKeyCode` and `KeyboardLedStatus` became `impl Display`
  (the blanket `ToString` impl still covers `.to_string()` callers), and a
  handful of `#[allow]`s were added instead of applying a suggested fix
  where the fix would have changed a wire format, diverged a
  vendored/generated file from its upstream source, or fought a documented
  deliberate design (notably `wezterm-gui`'s per-quad boxing in the glyph
  render layer, and three RAII guards -- `UmaskSaver`, `SimpleExecutor`,
  `ScopedExecutor` -- whose constructors mutate process-global state and
  so were deliberately not given a `Default` impl).
* Every source file over ~700 lines across roughly twenty crates was split
  so that each file holds one public export (one type, trait or function)
  plus whatever private helpers only it uses, re-exported from the
  original module path so nothing importing from these crates needs to
  change. This is pure reorganization with no behavior change -- verified,
  not just asserted, by diffing the line-for-line content of the old and
  new files against each other for every split, and additionally by a
  direct diff of every relocated function body (not just its surrounding
  lines) in the highest-risk files: the wire-protocol dispatch tables in
  `codec`, `wezterm-client` and `wezterm-mux-server-impl`; the terminal
  emulation core in `term`; the CSI/OSC escape-sequence parser in
  `wezterm-escape-parser`; keyboard/physical-key lookup tables in
  `wezterm-input-types` and `termwiz`; the Unicode bidi algorithm in `bidi`
  (plus a full run of its UCD conformance suite); and font shaping/mmap
  loading in `wezterm-font`. A few pre-existing, unrelated issues were
  noticed while reading through this code and were deliberately left
  untouched rather than folded into a reorganization change: `kitty`
  keyboard-protocol encoding maps two distinct keys to the same function
  code, a Windows console `scroll_region` computation uses the wrong
  coordinate for one axis, and `termwiz`'s numpad-key encoding sends the
  PageDown escape sequence for Numpad9 instead of the conventional PageUp
  one. Each is filed separately for its own fix.

#### New
* [wezterm.serde](config/reference/wezterm.serde/index.md) module for serialization
  and deserialization of JSON, TOML and YAML. Thanks to @expnn! #4969
* `wezterm ssh` now supports agent forwarding. Thanks to @Riatre! #5345
* SSH multiplexer domains now support agent forwarding, and will automatically
  maintain `SSH_AUTH_SOCK` to an appropriate value on the destination host,
  depending on the value of the new
  [mux_enable_ssh_agent](config/reference/config/mux_enable_ssh_agent.md) option.
  ?988 #1647
* [default_ssh_auth_sock](config/reference/config/default_ssh_auth_sock.md) option
  to manage `SSH_AUTH_SOCK`.
* Search mode: now supports richer line editing. Thanks to @Mrreadiness and
  @kenchou! #5416 #3087
* [show_close_tab_button_in_tabs](config/reference/config/show_close_tab_button_in_tabs.md)
  option for the fancy tab bar. Thanks to @zummenix! #3818
* wezterm-ssh now supports `ProxyUseFdPass`. Thanks to @loops! #6103 #6093
* `PromptInputLine` now supports a optional `prompt` and `initial_value`
  parameters. Thanks to @mgpinf and @ekorchmar! #6054 #6007
* Support Unicode 16 octant characters when `custom_block_glyphs` is enabled.
  Thanks to @eschnett! #6502 #6494
* [window_content_alignment](config/reference/config/window_content_alignment.md) option
  to control where the excess pixel gap will be placed when the window is not
  a multiple of the cell dimensions. Thanks to @Shiphan! #6629 #1124
* New `MACOS_FORCE_SQUARE_CORNERS` option for
  [window_decorations](config/reference/config/window_decorations.md). Thanks to
  @amadeusdotpng!  #6587 #2182
* [QuickSelectArgs](config/reference/keyassignment/QuickSelectArgs.md) has new
  `skip_action_on_paste` option. Thanks to @nhurlock! #6405
* Docs for writing [Plugins](config/plugins.md). Thanks to @alecthegeek and
  @MLFlexer! #6188
* [macos_fullscreen_extend_behind_notch](config/reference/config/macos_fullscreen_extend_behind_notch.md)
  option. Thanks to @wryanzimmerman! #5759
* [quick_select_remove_styling](config/reference/config/quick_select_remove_styling.md)
  option to make it easier to spot matches on colorful screens. Thanks to
  @mgpinf! #6683 #4022
* `tmux -CC` support is now very usable. Thanks to @joexue! #6602 #336
* [Confirmation](config/reference/keyassignment/Confirmation.md) key assignment
  that can be used to show a confirmation prompt. Thanks to @mgpinf! #6707
* [launcher_alphabet](config/reference/config/launcher_alphabet.md) option for
  [ShowLauncherArgs](config/reference/keyassignment/ShowLauncherArgs.md).
  Thanks to @mgpinf! #6677
* [window_decorations](config/reference/config/window_decorations.md) now supports
  `MACOS_USE_BACKGROUND_COLOR_AS_TITLEBAR_COLOR` to match the macOS window
  titlebar background color to the terminal background color defined by
  your configuration. Thanks to @Jay-Madden! #6558
* [char_select_font](config/reference/config/char_select_font.md),
  [command_palette_font](config/reference/config/command_palette_font.md), and
  [pane_select_font](config/reference/config/pane_select_font.md) options to control
  the fonts for those respective overlays/modals.  Thanks to @mgpinf! #6696
* Git branch and progress bar symbols have been added to
  [custom_block_glyphs](config/reference/config/custom_block_glyphs.md). Thanks to
  @BenBergman! #6328 #6873 #6875
* [cell_widths](config/reference/config/cell_widths.md) option for explicit
  control over cell widths. Thanks to @hamano! #6289 #6290
* [wayland_window_background_blur](config/reference/config/wayland_window_background_blur.md) option
  to enable window blur on Wayland compositors supporting the `ext-background-effect-v1` protocol.
  Thanks to @psomani16k, @1Capito1 & @bew! #6905 #7615 #7939
* [reverse_video_cursor_min_contrast](config/reference/config/reverse_video_cursor_min_contrast.md)
  option. Thanks to @jameshurst! #6584 ?2861
* [text_min_contrast_ratio](config/reference/config/text_min_contrast_ratio.md) to more generally
  improve the contrast ratio for text in the terminal.
* New `launcher_label_fg` and `launcher_label_bg` options for to customize
  the [Launcher Menu](config/launch.md#the-launcher-menu). Thanks to @mgpinf!
  #6796
* [TabInformation](config/reference/TabInformation.md) now exposes `is_last_active` as
  a boolean property to indicate whether a tab was the prior active tab.
  Thanks to @masriomarm! #6895
* Indicate support for OSC 52 (clipboard extensions) in Primary DA Response.
  Thanks to @j4james! #7046
* internal: Add NixOS-based VMs configurations for live testing in fresh desktop environments.
  See dedicated section in [CONTRIBUTING.md](https://github.com/wezterm/wezterm/blob/main/CONTRIBUTING.md)
* The default tab bar rendering now shows an animated spinner when ConEmu style
  OSC 9 escapes set the progress state to "Indeterminate".
* Documented how to keep [font_dirs](config/reference/config/font_dirs.md)
  portable across operating systems: relative entries already resolve
  safely against the config file's own directory on every platform, so the
  only real risk is hardcoding an absolute, OS-specific path (e.g.
  `C:/Windows/Fonts`) directly into the config. No new placeholder syntax
  was added for this -- leaving `font_locator` unset already picks the
  right per-OS system font locator (`Gdi`/`CoreText`/`FontConfig`) with no
  path to hardcode at all, which is the recommended way to reach "the
  system fonts" portably.
* [wezterm start --start-conf](cli/start.md#--start-conf-opening-a-fixed-set-of-tabs-at-startup)
  loads a startup layout from a ktav file: a set of tabs to open in a single
  new window, each with its own title, working directory (`root_dir`),
  extra environment variables and shell commands to run once its shell is
  ready, plus layout-wide `root_dir`/vars/commands every tab inherits (a
  tab's own value always wins). A relative `root_dir` resolves against the
  layout file's own directory, not the process's launch directory, so a
  layout file checked into a project stays portable. Mutually exclusive
  with `PROG`/`--cwd`.
* Tabs in the tab bar can now be reordered by dragging them with the
  mouse, driving the same reordering logic the `MoveTab`/`MoveTabRelative`
  key assignments already used. Scoped to reordering within the tab bar;
  dragging a tab out to detach it into its own window is not implemented.
* [RenameCurrentTab](config/reference/keyassignment/RenameCurrentTab.md)
  prompts for a new title for the active tab, pre-filled with its current
  one. Bound to `F2` by default (as in Windows Explorer); double-clicking
  the tab bar does the same. No context menu is involved.

#### Fixed
* Many symbol codepoints (e.g. U+23BF and other Miscellaneous
  Technical/Dingbats characters, such as the tree-drawing glyphs some tools
  use) rendered as an empty "tofu" box: the fallback chain worked correctly
  but none of the bundled fonts (JetBrains Mono, Nerd Font Mono, Noto Color
  Emoji) had a glyph for that range. Bundles [Noto Sans
  Symbols](https://fonts.google.com/noto/specimen/Noto+Sans+Symbols) and
  Noto Sans Symbols 2 (OFL 1.1, same license as the other bundled fonts) as
  additional built-in fallback fonts to close the gap.
* Text rendered via the (default, on Windows) WebGPU backend was visibly
  thinner than the same text rendered via OpenGL. Root cause: both backends
  receive the same pre-linearized vertex color, but OpenGL blends it
  directly in gamma space (matching how GDI/ClearType/CoreText render text),
  while WebGPU wrote into an sRGB-formatted surface with no shader-side
  gamma handling, so the GPU auto-linearized the blend -- physically
  correct, but perceptibly thinner for anti-aliased glyph edges than
  gamma-space blending. `shader.wgsl` now gamma-encodes its output and
  renders through a non-sRGB view of the surface, matching the OpenGL
  backend's blending space.
* Fallback font resolution for a missing glyph was non-deterministic: the
  same codepoint could resolve to a correct glyph on one run and to
  `.notdef` (tofu) on the next. Two compounding causes: the coverage-based
  candidate sort only ran when the (default-off) `sort_fallback_fonts_by_coverage`
  option was enabled, so by default candidates were ordered by a `HashMap`'s
  iteration order (randomized per-process); and the bundled Noto Color Emoji
  font's cmap claims coverage of some Dingbat codepoints (e.g. U+2702
  SCISSORS) that it can only actually render as part of a ZWJ/ligature
  sequence, not standalone, so winning the (previously unstable) tie-break
  against the correctly-covering font produced tofu. Candidate sorting is
  now always applied, with emoji-presentation fonts deprioritized on a
  coverage tie and a final deterministic tie-break by font name.
* Startup showed a small window that briefly resized (twice, in quick
  succession) to its final size, filled in the meantime with a plain,
  static color as a placeholder while the WebGPU renderer initializes.
  The double resize came from computing the window's initial pixel
  dimensions using a hardcoded 96 DPI default instead of the target
  monitor's real DPI: `new_window` built its font metrics on 96, then
  `check_and_call_resize_if_needed` read the true DPI via
  `GetDpiForWindow`, detected the mismatch with the cached dimensions,
  and fired a `WindowEvent::Resized`, after which the GUI rebuilt its
  metrics and called `set_inner_size` (no `WM_DPICHANGED` is involved --
  this codebase has no handler for it). The window now queries the
  primary monitor's actual DPI up front. The
  static placeholder fill is now a small animated spinner, drawn the same
  lightweight (non-GPU) way the old fill was. Once the renderer has
  produced its first frame *and* the shell has produced its first output
  (whichever happens later), the spinner smoothly cross-fades into the
  real terminal content instead of being replaced abruptly.
* Review of the startup cross-fade above surfaced a robustness round, now
  fixed: the fade could start with the placeholder spinner already
  destroyed (when the shell's first output arrived after the renderer, or
  on any later renderer rebuild), leaving an unpainted, opaque overlay
  sitting over live terminal content for the whole fade duration -- it now
  bails out to the old instant hand-off instead. The fade's `WS_EX_LAYERED`
  overlay window is no longer left stale (misaligned, or hidden behind a
  newly recreated WebGPU child window) across a resize or renderer rebuild
  mid-fade -- either now ends the fade immediately rather than animating a
  wrong rectangle. Three placeholder-spinner GDI handles could leak if
  window creation failed partway through. A reentrant `WM_TIMER` during the
  fade (possible from a nested message pump, e.g. a move/resize loop) could
  panic and take down the whole process; it now just skips that tick.
* Seven issues surfaced by review during the file-splitting and clippy
  cleanup above are now fixed. Three predate this fork (inherited from
  upstream): `kitty` keyboard-protocol encoding mapped `KeyCode::VolumeDown`
  to the same function code as `MediaPrevTrack` (`57436`) in one of its two
  independent match blocks (`VolumeDown` now correctly uses `57438` in
  both, matching the `57438`/`57439`/`57440` lower/raise/mute-volume run); a
  Windows console `scroll_region` computed its destination rectangle's Y
  coordinate from the X-axis `left` variable instead of `top`; and `termwiz`
  encoded `Numpad9` as the PageDown escape instead of the conventional
  PageUp one. Four were introduced by this fork's own recent work and are
  now fixed as well: `config` failed to compile on any unix target because
  a `SAFETY`-commented `unsafe` block around `version.rs`'s WSL detection,
  added by an earlier unsafe-audit pass, was scoped too narrowly, leaving
  `mem::zeroed()` and two `CStr::from_ptr` calls outside it; `termwiz`
  warned about an unused `MouseButtons` import on non-Windows targets,
  introduced by the file-splitting pass; the workspace's own `cargo fmt
  --all -- --check` gate had started failing across dozens of files,
  also from the file-splitting pass; and a stray `#[allow(dead_code)]`
  plus a duplicated `clippy::cognitive_complexity` entry in an
  `#[allow(...)]` list, both left over from splitting and clippy cleanup,
  were removed.
* Race condition when very quickly adjusting font scale, and other improvements
  around resizing. Thanks to @jknockel! #4876 #5032 #5033
* macOS: wacky initial window size with external monitors or certain font
  sizes. #4966 #4250
* macOS: dragging non-filename data over wezterm could cause it to crash. #4771
* New tabs spawned by the gui could spawn into the wrong domain when using
  multiplexing together `default_domain`. Thanks to @bogdan2412! #4994
* Linux: the `divine_process_list` fallback function used the *vmwisze*
  rather than the intended *starttime* field to decide which process
  was the youngest. Thanks to @crides! #5001
* Wayland: fixed startup on Hyprland >= 0.37.0. Thanks to @fioncat! #5264 #5103
* Wayland: updated to SCTK 0.19. Thanks to @deviant and @tmccombs! #5276 #5154 #5079 #5071
  #4604 #5209 #5781
* Windows: Window buttons stopped working when using `win32_system_backdrop`.
  Thanks to @Kushagra2569! #5362 #5348
* `wezterm cli activate-pane` now respects `unzoom_on_switch_pane`. Thanks to
  @quantonganh! #5306 #5305
* wezterm-ssh now correctly handles two-phase processing of `%h` tokens. Thanks
  to @emc2314 and @wheatdog! #5163 #4503
* We now respect line wrapping in alt-screen mode. Thanks to @eternity74! #5396
  #3283
* Wayland: hang when launched under ChromeOS Crostini. Thanks to @dberlin!
  #5393 #5397
* macOS: Fixed notch avoidance padding in full screen mode. Thanks to @mbaird!
  #5515 #3807
* Render invalidation issue when closing tabs other than the last tab. Thanks
  to @Mrreadiness! #5441 #5304
* Search mode now accepts composed input from the IME. Thanks to @kenchou! #5564
* Quick select mode will now accept unix paths with `//` in them. #5763
* blob leases (for image rendering) could be removed by temporary directory
  cleaners, resulting in issues with rendering. We no longer store these
  in a pure temporary directory; they live in a cache dir, and if someone
  does remove or truncate these files, we now convert that error case
  into blank frame(s). #5422 #4657
* PaneInformation object returned `pixel_width` when asked to return the
  `pixel_height`.
* ssh: we now explicitly kill and reap the `ProxyCommand` associated
  with an ssh session. Thanks to @daaku! #5494 #5479
* `default_ssh_domains()` didn't use the default local echo threshold
  for ssh domains. #5547
* multiplexer: internal PKI certificate now supplements its list of
  "Subject Alternative Names" with the list of canonical hostnames returned
  for the local system via `getaddrinfo`. #5543
* DECSLRM incorrectly clamped the left margin based on the terminal height
  instead of the terminal width. Thanks to @j4james and @tmccombs! #5871 #5750
* Scrollback position was incorrectly advanced when in alt-screen mode.
  Thanks to @tbung and @loops! #6099 #4607 #6186
* Wayland: Fixed potential panic on startup when monitors have changed are
  in the process of hot plugging when wezterm starts. Thanks to @loops! #6084
* macOS: explicitly set the window to sRGB colorspace to resolve incorrect
  colors on non-sRGB monitors. Thanks to @rianmcguire! #6063 #5824
* The bell would ring each window instead of just the window containing the
  pane where the bell is ringing. Thanks to @loops! #6012 #5985
* x11: transient errors in obtaining/setting the selection could cause
  wezterm to exit. Thanks to @loops! #6135 #5482 #6128
* Wayland: potential panic when working with the clipboard. Thanks to @rengare!
  #5518
* multiplexer: could lose track of delta updates if the display changed
  while the current delta was being computed. Thanks to @loops! #5981
* Plugins: normalize the plugin path to exclude trailing slashes. Thanks to
  @joncrangle! #5883
* zooming a tab might not work if you also recently used `pane:activate()`.
  Thanks to @SpyMachine! #5964 #5928
* `pane:current_working_dir.file_path` returned incorrect results for
  paths that contained `#` or `?` characters. Thanks to @loops! #6158 #6171
* wayland: issues with losing maximized or tiled state when switching between
  applications. Thanks to @aliaksandr-trush! #4568 #5897
* Mouse multiple button click requires pixel precision. Thanks to @jbiosca78!
  #6475 #6476
* background image with width/height set to `Contain` used the wrong aspect
  ratio. Thanks to @saltkid! #6554 #3708 #4407
* wayland: `hide_cursor: Missing enter event serial` error. Thanks to @jmbaur!
  #6548 #5760
* wayland: issue tiled and maximized window states. Thanks to
  @aliaksandr-trush! #6545 #6262
* wayland: potential crash on monitors with scale > 1. Thanks to @MaeIsBad!
  #6508 #5406
* Opening an `InputSelector` while some other overlay was active could
  result in an error. Thanks to @mikkasendke! #6403
* Improved handling of implicit hyperlinks with parentheses. Thanks to
  @psyclaudeZ! #6391
* macOS: Key repeat would stop when switching between held keys when `use_ime`
  was enabled. Thanks @psyclaudeZ! #6391 #4061
* `wezterm cli split-pane --move-pane-id` could kill panes. Thanks to @scauligi!
  #6028 #6029
* Glyph '┽', was rendering as '┥' when `custom_block_glyphs` was enabled.
  Thanks to @bew! #6661 #6655
* Windows: stack overflow when using `tmux -CC`. Thanks to @joexue! #6704 #6671
* `get_text_from_semantic_zone` didn't include the last line of text. Thanks to
  @mgpinf! #6248 #5806 #5346
* Deadlock when a domain detaches due to SSH timeout. Thanks to @joexue! #6749
  #6750
* Panic when rewrapping very very long lines. #6729
* CUP position parameters were mandatory when they should have been optional.
  Thanks to @wojciech-graj! #6860
* Long CSI sequences were not parsed correctly. Thanks to @jdugan6240! #5161
  #6194
* IBus IME working unreliably. Thanks to @pjm0616! #5125
* Pixel aliasing issue when using
  [window_content_alignment](config/reference/config/window_content_alignment.md) =
  `Center`. Thanks to @juster-0! #6929 #6928 #6823
* Passing a `SpawnCommand` to the `SwitchToWorkspace` assignment would ignore
  `set_environment_variables`. Thanks to @vincentbesanceney! #6850 #6845
* `libssh` based ssh sessions will now respect `ServerAliveInterval`. #4023
* macOS: prevent infinite loop in `Services` menu validation. Thanks to @cpick!
  #7098 #6738 #6833 #6864
* Wayland: fixed issue with fractional scaling. Thanks to @kalebo! #7277
* Incorrect boundary condition in renderstate. Thanks to @I-Info! #7274
* MacOS: fix memory leak in macOS MetalLayer management. Thanks to @I-Info!
  #7283
* [max_fps](config/reference/config/max_fps.md) can now be set to values larger than
  `255`. Thanks to @beckend! #7366
* macOS: Fix toast notifications. Thanks to @nikhilm! #7483
* termwiz: Fixed parsing of fragmented mouse reporting sequence. Thanks to
  @jgiannuzzi! #7076 #7504
* docs: add missing `panes` field to [TabInformation](config/reference/TabInformation.md).
  Thanks to @KevinSilvester! #7710
* Windows: Fixed a crash (RefCell borrow conflict) when toggling IME (e.g.
  pressing Hankaku/Zenkaku) after splitting a pane. Thanks to @shiena! #7529
* Fixed a stack overflow that could occur on Windows (and other platforms) when
  the process tree contained cycles due to PID reuse. Thanks to @novoselov-ab! #7706
* Wayland: Fixed clipboard paste failing in windows that were not focused when
  the copy happened. Thanks to @bew and @XeroOl! #7863
* Fixed an infinite loop in pane search when the regex engine hit a backtracking
  limit. Thanks to @bew! #7864
* Fix ESC key encoding in kitty mode with disambiguate flag enabled.
  Thanks to @Felixoid and @the-mikedavis! #7787
* Fixed two divide-by-zero crashes in Kitty inline image placement when a program requests
  a zero-sized placement (e.g. `w=0`/`h=0`), or displaying a cell-sized image on a pane
  whose pty reported no pixel dimensions (e.g. in `tmux -CC` domain).
  Such images are now refused instead of taking down the pane. Thanks to @zakrad! #6344
* Capability queries (DA1, the kitty keyboard probe, etc.) are now answered
  while a synchronized update is open, and an update held open for too long
  now times out; see
  [mux_synchronized_output_timeout_ms](config/reference/config/mux_synchronized_output_timeout_ms.md).
  Thanks to @luizribeiro! #7918
* Fix render loop freeze when closing workspaces. Thanks to @JafarAbdi! #7444
* `FontConfigInner` never rebuilt its font locator when
  [font_locator](config/reference/config/font_locator.md) changed at
  runtime -- only `font_dirs` was rebuilt, so switching locators via a
  config reload silently kept using the old one until the process
  restarted.
* On Windows, `GdiFontLocator` always asked DirectWrite for an exact
  `Normal` stretch match regardless of what was actually requested.
  DirectWrite's exact-match lookup then failed for any config that
  explicitly requests a non-`Normal` stretch matching an installed font's
  real stretch (e.g. a config that asks for `stretch: SemiCondensed` to
  match a font whose only face genuinely is SemiCondensed, such as Lucida
  Console), silently falling through to the slower legacy GDI path even
  though DirectWrite could have resolved it directly. The requested
  stretch is now passed through correctly. Note: a config that does not
  set `stretch` at all (the common case, including a plain `font: [{
  family: "Lucida Console" }]`) already requested -- and still requests --
  the default `Normal`, so this specific fix does not change DirectWrite's
  answer for that case; it was investigated as part of a report that a
  "Unable to load a font..." warning reappears after changing Windows'
  system font-scaling setting and then the terminal's font size, but that
  exact trigger was not reproduced and remains only a hardening fix, not a
  confirmed resolution of that report.
* The "No fonts contain glyphs for these codepoints" warning (log message
  and toast) now detects one specific, easy-to-hit misconfiguration and
  says so directly: when [font_dirs](config/reference/config/font_dirs.md)
  is set but
  [search_font_dirs_for_fallback](config/reference/config/search_font_dirs_for_fallback.md)
  is `false` (the default), those directories are scanned but never
  actually consulted for fallback glyph resolution -- the warning now
  names this and suggests setting `search_font_dirs_for_fallback = true`
  instead of leaving the reader to rediscover the two settings'
  relationship from scratch.
* Startup was slow before the window appeared, dominated by resolving the
  configured `color_scheme`: resolving a single named scheme used to
  TOML-parse all ~1000 bundled
  color schemes (755KB of data) just to find the one requested. Now only
  the requested scheme is parsed (memoized after the first lookup);
  measured on a debug build, the config-resolution step this fell under
  dropped from 2.21s to 14ms and the window appeared roughly a second
  sooner. Schemes only reachable via an alias still fall back to building
  the full map, since aliases are only discoverable after parsing every
  scheme's metadata -- a rare case.
* The window shown at startup could flicker strongly right before the
  first real terminal frame appeared. The Windows GDI placeholder (the
  "Loading..." text painted while the WebGpu renderer initializes) was
  torn down as soon as the renderer object merely existed, which is
  measurably (~150-165ms) before the WebGpu child window's swapchain had
  actually presented its first frame -- in that gap nothing was painting
  the window's client area, showing undefined swapchain contents
  (typically a black flash). The placeholder teardown now waits for a
  frame to have actually been built and handed off for presentation
  before clearing.
* That fix above turned out not to fully close the gap: with
  `webgpu_render_thread` enabled (the Windows default), handing a frame
  off for presentation only means it was *enqueued* to the dedicated
  per-window render thread -- the actual GPU submit and present happen
  later, asynchronously, on that thread. Tearing the placeholder down
  right after the enqueue left
  the same undefined-swapchain-contents gap, just shifted slightly later
  and asynchronous with the GUI thread. Invisible against the desktop
  (nothing behind the window to show through), but with a second OnlyTerm
  window overlapping in the background, that other window's real content
  showed through instead -- the startup flicker and "white/transparent
  rectangles" some users saw when running more than one OnlyTerm window.
  The placeholder is now cleared only after the render thread's first
  actual present, closing the gap for real.
* The placeholder-to-terminal cross-fade (see the startup-flicker entries
  just above, and the earlier spinner/cross-fade work in this same section)
  was cut short by *any* window-position change
  notification, not just an actual resize -- so simply moving the window
  while the fade was still playing interrupted it, even though the
  fade overlay (a child window using client-relative coordinates) tracks
  the parent automatically on a move and its bounds were still perfectly
  valid. The fade is now only interrupted by a genuine size/DPI change.
* Windows: fixed a narrow but real double-free. If `CreateWindowExW` failed
  *after* `WM_NCCREATE` had already run (in practice, only reachable via
  GDI/USER handle exhaustion), Windows' own unwinding of the
  partially-created window already reclaimed and dropped an internal
  reference that window creation's own failure path then unconditionally
  dropped a second time.
* The version string reported by `wezterm -h` (and shown elsewhere in the
  UI) never reflected the project's actual release tag -- the build script
  only ever derived it from the current commit's date and short hash. It
  now prefers `git describe --tags` against the nearest reachable `v*` tag
  (falling back to the old date-hash form only when no tag is reachable at
  all, e.g. a shallow clone with tags not fetched), so a tagged build now
  reports a version like `v0.0.2-alpha` instead of a bare timestamp.
* Rendering could silently drop content -- typically visible as only the
  top half of a pane rendering, with wrong colors on what did render, once
  a window's content grew busy enough. Root cause: each content render
  layer's three quad sub-layers had hardcoded, never-growing capacities
  (32/1024/32 quad instances) left over from the instanced-rendering
  rewrite; the old grow-and-retry signal that used to catch an overflowing
  layer had been silently broken by that same rewrite, so anything past
  the fixed cap was just dropped, top-to-bottom, every frame. The
  now-vestigial capacity clamp is removed; the GPU-side instance buffer
  already grows to whatever is actually submitted.
* A busy pane whose per-frame row-shaping work exceeded
  `tab_frame_build_budget_ms` could show blank rows instead of its last
  known content, and the row(s) it deferred could get stuck rebuilding the
  same row every sweep instead of progressing through the rest of the
  pane. Deferred rows now keep showing their last successfully built quads
  (falling only mildly stale, never blank) while the budget sweep makes
  guaranteed forward progress across the pane frame over frame.
* The renderer now skips re-submitting a frame to the GPU entirely when
  its content is pixel-identical to the previous frame (compared via a
  fixed-seed content hash over the frame's quad instances, dimensions,
  and color/projection state), avoiding wasted GPU work on an idle window.
* Two per-frame CPU costs that showed up under profiling as measurable
  main-thread time even on otherwise idle screens are now cached instead
  of recomputed every frame: `Line::last_cell_was_wrapped()` (used to find
  soft-wrap/logical-line boundaries, e.g. for hyperlink detection) is
  memoized on the line itself, keyed on its existing change-sequence
  number; and each line's glyph-shaping cache key (previously cached via a
  mechanism that could never actually hit once a line was cloned for
  rendering, which happens every frame) is now cached by TermWindow,
  keyed by `(pane, stable row)`, so it survives the clone.
* Scrolling used to be a guaranteed cache miss for every visible line's
  rendered quads: the cache key included the line's absolute screen
  position, which changes on every scroll step even though the line's
  actual text content is unchanged. The key is now based on the line's
  position relative to the pane instead, so scrolling reuses cached quads
  the same way an unscrolled, unchanged frame already did.
* Copy Mode's search bar/match highlighting and QuickSelect's labels could
  silently fail to appear: both overlays render by mutating a clone of the
  underlying pane's line, but tagged those mutations with a hardcoded
  sequence number of 0, which the line's `max()`-based sequence tracking
  treated as a no-op once the line already had a higher (real) sequence
  number. The line-shaping cache introduced just above is keyed on that
  sequence number, so it could go on serving a stale, pre-overlay shape for
  the same row. Both overlays (and the local-echo input-prediction path,
  which had the same bug) now stamp their mutations with a counter reserved
  far above any real sequence number, bumped once per render pass, so every
  pass is seen as a genuinely new version of the line.
* A line ending in a double-width (e.g. CJK) character right at the wrap
  column could inconsistently reflow, copy, or search as either one logical
  line or two, depending on unrelated cache state elsewhere on the line.
  The function that marks a line as wrapped was writing the flag to a
  different cell than the one the line-wrap reader actually inspects (the
  padding cell trailing a wide character, versus that character's own lead
  cell); both now agree on the same cell.
* Fixed a related bug in the same area: once a line ending in a double-width
  character was moved into scrollback (which switches its internal storage
  representation for memory efficiency) and later re-marked as wrapped or
  un-wrapped, the character's column width bookkeeping for that internal
  representation could become corrupted, potentially splitting the wide
  character in two on subsequent redraws, copies, or resizes.
* A line could appear visibly duplicated onto another row after scrolling,
  with the duplicate never clearing no matter how much further you
  scrolled. Root cause: a terminal application that sets a scroll region
  covering only the top part of the screen (leaving some rows at the
  bottom outside it) could, once scrollback filled up, silently shift
  those bottom rows' internal row identity without marking them changed --
  the line-shaping cache introduced above is keyed on that identity, so it
  kept serving one row's cached appearance for a different row, forever,
  since nothing ever told it the identity had moved.
* Alt+<letter> keybindings (eg. Alt+V) stopped firing on a number of non-US
  keyboard layouts. Root cause: the background probe that builds the dead-key
  table queried `ToUnicode` for a plain-Alt-held state, and on some layouts
  the driver answers with the dead-key sentinel for that combination as an
  artifact unrelated to any real compose sequence -- once recorded, the
  keypress was swallowed before it ever reached the keybinding pipeline.
  Plain Alt is never used by any Windows keyboard layout to compose text (only
  AltGr legitimately does that), so it's excluded from the probe entirely --
  a layout-independent fix rather than a per-layout patch.
* The GUI-thread watchdog (added to detect a genuinely stuck message loop)
  produced false positives -- logged as multi-hour or multi-minute "hangs"
  that recovered within a second -- because its heartbeat only ticks once
  before the message loop parks waiting for the next message, so a long but
  entirely normal idle period (or the whole process being suspended by OS
  sleep/hibernate) looked identical to a real stall. The watchdog no longer
  counts idle waiting time or a post-suspend gap as a stall, only genuine
  stuck-message-loop time; the "OnlyTerm is not responding" toast this used
  to trigger has been removed (the watchdog still logs and records metrics
  on a real hang).
* WebGpu initialization (`Instance`/`Adapter`/`Device` creation) ran on a
  background thread, but the GUI thread synchronously blocked on it
  finishing before returning -- so the message loop sat idle and
  unresponsive for the whole duration, even though the actual driver work
  never touched it. Window creation now `await`s that work asynchronously
  instead, so the message loop keeps pumping while the GPU driver
  initializes. Two windows created around the same time (eg. multi-window
  session restore) could each still separately race into that
  initialization and briefly stand up two live GPU devices before one was
  discarded; the shared context is now properly serialized so only one
  initialization ever happens.
* Under the WebGpu render thread (`webgpu_render_thread`, on by default), a
  frame could be built and its instance data uploaded to a persistent GPU
  buffer while the previous frame was still being submitted and possibly
  still reading that same buffer -- a genuine data race that could produce
  a garbled or mixed frame. The GUI thread now checks render-thread
  backpressure *before* touching the buffer, skipping the build entirely
  (and scheduling a fresh repaint once the in-flight frame finishes) rather
  than racing it. A narrow follow-up race in that same backpressure check
  (the render thread could finish and check for a pending repaint in the
  gap before the GUI thread flagged one) is also closed -- a freshly built
  frame could otherwise sit unpainted until an unrelated event happened to
  trigger a repaint.
* If the shared GPU device was lost (e.g. a Windows TDR) and successfully
  recovered once, every window sharing the process-wide GPU context kept
  reusing the *same* now-dead `Device` afterward -- recovery from a real
  device loss could never actually succeed, since nothing ever rebuilt the
  underlying `Instance`/`Adapter`/`Device`/`Queue` chain. The cached context
  is now invalidated and rebuilt from scratch the next time it's requested
  after a device-lost event.
* A pane with sustained, continuous output (e.g. `yes`, or any command that
  never goes quiet) could drive the internal notification-delivery path
  into unbounded recursion, one stack frame per redelivery round, with no
  upper bound -- and while it kept recursing, the GUI thread never got a
  chance to service any other pane, window, or input event. Redelivery is
  now a bounded loop that periodically yields back to the event loop
  instead of recursing indefinitely; no output is dropped either way.
* A closed window's GPU device-lost subscription (kept for future
  device-loss recovery notifications) was never actually removed from the
  process-wide registry: the entry held a strong reference to the window's
  own liveness flag instead of the weak one its doc comment already
  claimed, so the registry itself was what kept that flag reachable. In
  practice this meant a closed window's entry could be notified as if it
  were still live on the next device-lost event, rather than being pruned.
  Pruning (which only runs as part of handling an actual device-lost
  event, not on a timer) now correctly recognizes and drops entries whose
  owning window has been closed.
* The glyph cache's internal hash tables used a fixed, compile-time-constant
  hash seed rather than one randomized per process. Since the cache keys
  are derived from characters/styles/fonts that ultimately come from
  terminal content, a specially crafted stream of terminal output could in
  principle target hash collisions and degrade lookups toward O(n) -- a CPU
  cost driven purely by terminal content. The hash seed is now randomized
  per process, substantially raising the bar against that class of attack
  (this is not a cryptographic hash, so it isn't an absolute guarantee)
  while keeping the same measured 40-80% speedup over the standard
  library's default hasher.
* The shared GPU context could still be cached and reused as "healthy" even
  when its own `Device` was lost during initialization itself (a narrow
  window between the device-lost callback firing and the context finishing
  setup) -- closing a residual race left over after the device-lost recovery
  fix above. Staleness is now tracked directly on the context that owns the
  lost device instead of a separate counter that couldn't distinguish "this
  context's device died" from "an earlier, already-replaced one did".
  Two remaining gaps in that same path are closed as well: the device-lost
  handler used to be registered a good deal later than the device itself was
  created (after shader/buffer setup), and a loss landing in between was
  swallowed permanently -- leaving a dead device cached as healthy for the
  rest of the session -- since the graphics API does not replay a loss that
  already happened; and a context whose device died before initialization
  finished is no longer handed to the window that requested it (that window
  would have had no way to recover), initialization is retried instead.
* A device-lost recovery notification could still be sent to a window that
  had already closed: closing a window stopped its render thread but never
  marked its GPU state as superseded, and the render thread kept its own
  separate reference to that state that could outlive window close (e.g.
  while stuck in a hung driver call). Window close now marks the GPU state
  stale immediately, before tearing down the render thread.
* The device-lost subscriber registry (see the pruning fix above) only
  pruned closed windows' entries when an actual device-lost event fired,
  which may never happen during a normal session -- so the registry could
  grow by one entry per window ever opened and closed, unbounded, over a
  long-running process. It's now also pruned every time a new window
  registers.
* Opening a window while another OnlyTerm window was already on screen made
  the overlapping area flicker several times during startup, briefly showing
  the window behind. The GPU rendering surface lives on its own child window
  covering the whole client area, and that child was both created visible
  before it had any pixels of its own and configured to erase itself on every
  resize -- while erasing it painted nothing at all once the startup
  placeholder had been retired. Each of the handful of geometry changes a
  window makes as it starts up therefore blanked it until the next frame was
  drawn. The GPU surface is no longer shown until it has actually presented a
  frame (the animated startup placeholder covers that time, as intended), and
  no longer erases itself on resize, so the previous frame stays on screen
  until the next one is ready. This was only visible when a window was already
  open behind the new one, because in that case the new window is created by
  the already-running instance and its renderer is ready almost immediately,
  retiring the placeholder while the startup resizes were still happening.
* `--start-conf`: a startup command that failed to be written to its pane
  (a rare pty-write failure) used to fail completely silently, with layout
  startup otherwise reporting success -- now logged as a warning identifying
  which tab and which command list (layout-wide or the tab's own) it came
  from, plus the underlying error (not the command's own text, since startup
  commands can carry credentials that shouldn't end up in a log file).

#### Updated
* Bundled conpty.dll and OpenConsole.exe to build 1.22.250204002.nupkg
* Bundled harfbuzz to 11.2.1
* Bundled libssh to 0.11.1
* Bundled freetype to 2.13.3
* Bundled Nerd Font Symbols font to v3.3.0
* Bundled Noto Color Emoji font to 2.047
* image crate to 0.25, which means that JPEG images are now decoded via
  [zune-jpeg](https://docs.rs/zune-jpeg/latest/zune_jpeg/), which improves
  handling of non-conforming jpeg images. #5365
* Color schemes: [Astrodark (Gogh)](colorschemes/a/index.md#astrodark-gogh),
  [Blue Dolphin (Gogh)](colorschemes/b/index.md#blue-dolphin-gogh),
  [Breadog (Gogh)](colorschemes/b/index.md#breadog-gogh),
  [Butrin (Gogh)](colorschemes/b/index.md#butrin-gogh),
  [City Lights (Gogh)](colorschemes/c/index.md#city-lights-gogh),
  [CutiePro](colorschemes/c/index.md#cutiepro),
  [Ef-Dream](colorschemes/e/index.md#ef-dream),
  [Ef-Reverie](colorschemes/e/index.md#ef-reverie),
  [Eldritch](colorschemes/e/index.md#eldritch),
  [Everforest Dark Hard (Gogh)](colorschemes/e/index.md#everforest-dark-hard-gogh),
  [Everforest Dark Medium (Gogh)](colorschemes/e/index.md#everforest-dark-medium-gogh),
  [Everforest Dark Soft (Gogh)](colorschemes/e/index.md#everforest-dark-soft-gogh),
  [Everforest Light Hard (Gogh)](colorschemes/e/index.md#everforest-light-hard-gogh),
  [Everforest Light Medium (Gogh)](colorschemes/e/index.md#everforest-light-medium-gogh),
  [Everforest Light Soft (Gogh)](colorschemes/e/index.md#everforest-light-soft-gogh),
  [Github Light (Gogh)](colorschemes/g/index.md#github-light-gogh),
  [Iceberg (Gogh)](colorschemes/i/index.md#iceberg-gogh),
  [Kanagawa Dragon (Gogh)](colorschemes/k/index.md#kanagawa-dragon-gogh),
  [kurokula](colorschemes/k/index.md#kurokula),
  [Mellifluous](colorschemes/m/index.md#mellifluous),
  [Miramare (Gogh)](colorschemes/m/index.md#miramare-gogh),
  [Modus Operandi (Gogh)](colorschemes/m/index.md#modus-operandi-gogh),
  [Modus Operandi Tinted (Gogh)](colorschemes/m/index.md#modus-operandi-tinted-gogh),
  [Modus Vivendi (Gogh)](colorschemes/m/index.md#modus-vivendi-gogh),
  [Modus Vivendi Tinted (Gogh)](colorschemes/m/index.md#modus-vivendi-tinted-gogh),
  [NvimDark](colorschemes/n/index.md#nvimdark),
  [NvimLight](colorschemes/n/index.md#nvimlight),
  [Paper (Gogh)](colorschemes/p/index.md#paper-gogh),
  [Quiet (Gogh)](colorschemes/q/index.md#quiet-gogh),
  [Selenized Black (Gogh)](colorschemes/s/index.md#selenized-black-gogh),
  [Selenized White (Gogh)](colorschemes/s/index.md#selenized-white-gogh),
  [Seoul256 (Gogh)](colorschemes/s/index.md#seoul256-gogh),
  [Seoul256 Light (Gogh)](colorschemes/s/index.md#seoul256-light-gogh),
  [Sparky (Gogh)](colorschemes/s/index.md#sparky-gogh),
  [Sugarplum](colorschemes/s/index.md#sugarplum),
  [Vesper](colorschemes/v/index.md#vesper)
