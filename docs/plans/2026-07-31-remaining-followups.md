# Remaining follow-ups after the execution-decoupling initiative

Three items were left as un-started notes when the execution-decoupling initiative
(`docs/plans/2026-07-29-execution-decoupling.md`) and the two-round `@oh` review of it
(commits `3fb93fd0e..3709c088c`) were closed out: the deferred "221.9" per-frame
`create_buffer` question, the observation that `spawn-funcs`' rhai registration is never
called, and the leftover non-blocking findings from the first review round. This document
records what each of them actually is against the code as it stands today, what is worth
doing about each, and a concrete task decomposition at the end. Two of the three turned
out to be materially different from their original one-line descriptions: item 1 is
smaller than it sounds (and the interesting part of it is not what was flagged), item 2 is
*much* bigger (it is not a spawn-funcs problem at all).

---

## 1. Per-frame `create_buffer` on the GUI thread (was TaskList #235, "221.9")

### What the code actually does

The call in question is `WebGpuVertexBuffer::recreate()`
(`crates/onlyterm-gui/src/renderstate.rs:241-253`), invoked from
`TermWindow::build_webgpu_frame` at `crates/onlyterm-gui/src/termwindow/render/draw.rs:71`,
which runs on the GUI thread once per painted frame:

```
for layer in render_state.layers.borrow().iter() {   // draw.rs:65
    for idx in 0..3 {                                // draw.rs:66
        ...
        let vertex_buffer = vertices.webgpu_mut().recreate();  // draw.rs:71
        vertex_buffer.unmap();                                 // draw.rs:72
```

So the count is *one `create_buffer` per (`RenderLayer`, sub-layer) pair with a non-zero
vertex count, per frame*. In the common single-window/single-pane case that is layer
zindex 0 with sub-layers 0 (cell backgrounds, `render/screen_line.rs:266`) and 1 (glyphs,
`render/screen_line.rs:643`), plus whatever `box_model.rs:832`'s `layer_for_zindex` has
created for the fancy tab bar / modal. Realistically 2-5 per frame.

Buffer size is the **layer capacity**, not the used quad count:
`recreate()` re-creates at `self.num_vertices`, which is `capacity * VERTICES_PER_CELL`.
`Vertex` (`crates/onlyterm-gui/src/quad.rs:33-43`) is 17 `f32` = 68 bytes, so a quad is 272
bytes. The main layer's glyph sub-layer starts at 1024 quads (`renderstate.rs`,
`RenderLayer::new(&context, 1024, 0)`) and grows in multiples of 128 via
`allocated_more_quads` (`renderstate.rs:598-604`). A 200x60 dense text window needs on the
order of 12k glyph quads -> ~3.3 MB for that one sub-layer.

### What that actually costs (this is the part the original note got wrong)

The interesting cost is **not** `create_buffer` itself. `mapped_at_creation: true` on a
buffer whose usage does *not* include `MAP_WRITE` (ours is `BufferUsages::VERTEX` only)
takes wgpu-core's staging path — verified in the pinned wgpu 25.0.2 sources:

* `wgpu-core-25.0.2/src/device/resource.rs:735-749`: allocates a **second** buffer
  (`StagingBuffer::new`, `MAP_WRITE | COPY_SRC`) of the same aligned size, maps it, and
  calls `staging_buffer.write_zeros()`.
* `wgpu-core-25.0.2/src/resource.rs:901-903`: `write_zeros` is
  `ptr::write_bytes(ptr, 0, size)` — a **full-capacity memset over host-visible (upload
  heap / write-combined) memory**, every frame, regardless of how many quads are actually
  used.
* `wgpu-core-25.0.2/src/device/global.rs:2231-2280` (`unmap` on `BufferMapState::Init`):
  takes `queue.pending_writes.lock()` and records a `copy_buffer_to_buffer` of
  `self.size` — again the **full capacity**, not the used range.
* `wgpu-hal-25.0.2/src/dx12/device.rs:414` -> `suballocation.rs:135`: each of the two
  buffers is a `gpu_allocator` suballocation + `CreatePlacedResource` + `SetName` under a
  mutex.

So per frame per non-empty sub-layer: 2 buffer allocations, 1 full-capacity memset over WC
memory, and 1 full-capacity GPU-side copy.

**Estimated, not measured** (no profiling run was performed; the checked-in
`target/*/onlyterm-gui.exe` binaries predate this work and a fresh build was out of scope):
at ~4 GB/s for a WC memset, a 3.3 MB glyph sub-layer costs ~0.8 ms of GUI-thread time per
frame; an 80x24 window (~2k quads, 544 KB) costs ~0.14 ms. Against the existing
`tab_frame_build_budget_ms` default of 40 ms, that is 2-3% at the large end and noise at
the small end. Repaint frequency at idle is bounded by `animation_fps` (default 10,
`crates/config/src/config.rs:2121`) driving the cursor blink, so the idle steady-state
waste is small; under heavy pty output it scales to frame rate.

There is one further, on-theme observation that is *not* a performance point: the GUI
thread's `unmap()` takes `queue.pending_writes.lock()`, and the render thread's
`queue.submit` holds that same lock across the raw HAL submit
(`wgpu-core-25.0.2/src/device/queue.rs:1221-1310`). That is the only per-frame wgpu-core
lock shared between the GUI thread and the render thread. `ExecuteCommandLists` is not
normally a blocking call (the blocking one, `present()`, is outside the lock at
`webgpu.rs:854`), so this is a theoretical rather than observed coupling — but it is the
one thing that would make the pooling work worth doing for reasons other than throughput.

### Two adjacent findings worth more than the original item

**(a) `create_uniform` is called inside the per-draw loop.**
`WebGpuState::submit_frame` builds the uniform buffer + bind group once *per draw call*
(`crates/onlyterm-gui/src/termwindow/webgpu.rs:841`, inside the
`for draw in &frame.draws` loop at line 815) from `frame.uniform`, which is a `Copy`
struct that is identical for every draw in the frame. This is a redundant
`create_buffer_init` + `create_bind_group` per draw. It is on the render thread rather
than the GUI thread, so it does not block anything, but it is free to fix.

**(b) The 3-slot vertex-buffer rotation is vestigial in the WebGpu path.**
`TripleVertexBuffer` holds `bufs: RefCell<[VertexBuffer; 3]>` (`renderstate.rs:321-327`)
and `next_index()` rotates through them once per frame (`draw.rs:81`). That rotation
exists for the Glium path, where the same buffer object is written and re-read. In the
WebGpu path `recreate()` swaps a *brand-new* buffer into the current slot every frame and
hands the filled one to the render thread, so slot N is never reused for a buffer the GPU
has seen — the rotation buys exactly nothing, while keeping 3x the vertex-buffer memory
alive per sub-layer (~6.5 MB of dead upload-heap memory per window for a large window's
glyph sub-layer alone).

### Recommendation

* **Do (a) and (b), and add measurement.** All three are small, low-risk, and (b) is a
  real memory win.
* **Do NOT do the actual buffer-pooling redesign now.** The estimate above puts it at
  sub-millisecond for typical windows and ~1 ms for very large ones, against a real
  implementation cost: you cannot simply reuse the buffer, because `MAP_WRITE` cannot be
  combined with `VERTEX` under WebGPU's usage rules
  (`wgpu-core-25.0.2/src/device/resource.rs:618-628`). A pool would mean persistent
  `MAP_WRITE | COPY_SRC` staging buffers per slot, `map_async` (whose callbacks only run
  when the device is polled — which happens on the *render* thread inside
  `queue.submit`'s trailing `device.maintain`), a readiness handshake back to the GUI
  thread, a fallback allocation path for "slot not ready yet", and a per-frame
  `CommandEncoder` recorded on the GUI thread and shipped inside `GpuFrame`. That is a
  day's work on the hottest path in the renderer, for a few percent of a frame budget that
  is already policed by `tab_frame_build_budget_ms`.
* **Revisit if** the instrumentation from task 2 below shows the per-frame buffer bytes
  regularly exceeding ~8 MB/frame or the recreate+unmap step regularly exceeding ~2 ms,
  **or** a user reports GUI-thread stalls that profile to `build_webgpu_frame` rather than
  to shaping/quad-building.

---

## 2. The rhai API crates are not wired into the live engine (was TaskList #264)

### What was reported vs. what is actually true

The note said `spawn-funcs::register_rhai` is never wired into the GUI's rhai engine. That
is true, but it dramatically understates the problem. **None of the `lua-api-crates` are
wired in.** The only `register_rhai` functions that reach a live engine are `onlyterm-gui`'s
own two.

Evidence:

* `config::rhai_engine::make_rhai_engine` (`crates/config/src/rhai_engine.rs:294`) is the
  single engine factory. It registers a small fixed set of built-ins (`on`,
  `add_to_config_reload_watch_list`, `config_file`, `config_dir`, `target_triple`,
  `version`, `home_dir`, `hostname`, `has_action`) and then, at line 374-376, applies
  everything in `RHAI_SETUP_FUNCS`.
* `RHAI_SETUP_FUNCS` is populated only by `config::rhai_engine::add_rhai_setup_func`
  (`crates/config/src/rhai_engine.rs:399`). A repo-wide search finds exactly two call
  sites, both in `crates/onlyterm-gui/src/main.rs:1173-1174`:
  `crate::scripting::register_rhai` (GuiWin) and `crate::termwindow::register_rhai`
  (TabInformation/PaneInformation).
* Every `lua-api-crates/*/src/lib.rs::register_rhai` is referenced *only* from that
  crate's own `tests/rhai_smoke.rs`. Nothing in `env-bootstrap`, `config`, `mux`,
  `onlyterm-gui`, `onlyterm`, or `onlyterm-mux-server` calls any of them.

### Why: it was dropped during L5, not "never finished"

`env-bootstrap` was the central wiring point. `git show 48e38e224 -- env-bootstrap/src/lib.rs`
("config, lua-api-crates, mux, onlyterm-gui: remove mlua and luahelper from workspace (L5)")
deleted this function outright and added no rhai replacement:

```rust
fn register_lua_modules() {
    for func in [ battery::register, color_funcs::register, termwiz_funcs::register,
                  logging::register, mux_lua::register, procinfo_funcs::register,
                  filesystem::register, serde_funcs::register, plugin::register,
                  spawn_funcs::register, share_data::register, time_funcs::register,
                  url_funcs::register ] {
        config::lua::add_context_setup_func(func);
    }
}
```

It was called from `bootstrap()`, which all three binaries still call
(`crates/onlyterm/src/main.rs:735`, `crates/onlyterm-gui/src/main.rs:1168`,
`crates/onlyterm-mux-server/src/main.rs:80`). The L5 commit message asserts rhai "is the
only registered/reachable scripting path" — that assertion was wrong for the API crates.

`crates/env-bootstrap/Cargo.toml` **still declares dependencies on all 13 crates** and
never calls into any of them, which is why nothing (build, clippy, tests) noticed.

### What is actually unreachable from a user's `.rhai` config today

Everything except the nine built-ins listed above plus `GuiWin`/`TabInformation`/
`PaneInformation`. Concretely, and including things the shipped docs promise:

* `format(...)` (`crates/lua-api-crates/termwiz-funcs/src/lib.rs:241`) — the single most
  common onlyterm config idiom, used by every `format-tab-title` / `update-status`
  recipe. `docs/migration-lua-to-rhai.md:339` documents it as available.
* `run_child_process` / `background_child_process` / `open_with`
  (`crates/lua-api-crates/spawn-funcs/src/lib.rs:27-32`). Documented with a rhai call site
  at `docs/config/reference/onlyterm/run_child_process.md:17`.
* `serde::json_encode` etc. and `plugin::require` — documented at
  `docs/migration-lua-to-rhai.md:286-288`.
* The whole `mux` module (`crates/lua-api-crates/mux/src/lib.rs:53`), plus `color_funcs`,
  `filesystem`, `logging` (`log_info`/`log_warn`/`log_error`), `procinfo_funcs`,
  `battery`, `share_data` (`global_data()`), `nerdfonts`, `pad_left`/`pad_right`/
  `truncate_*`, `permute_any_mods`, ...

A `.rhai` config that calls any of these fails at evaluation time with rhai's
"function not found", which for a top-level config call means the config fails to load.

Two related inaccuracies fall out of this:

* `crates/onlyterm-gui/src/overlay/debug.rs:33-35` claims the REPL engine exposes
  "every `register_rhai` binding (`onlyterm.mux.*`, `GuiWin`, `TabInformation`, ...)".
  Only the last two are true today.
* `time-funcs`, `url-funcs` and `window-funcs` have **no** `register_rhai` at all.
  `crates/lua-api-crates/time-funcs/src/lib.rs:13-18` documents this explicitly
  (`onlyterm.time.call_after` has no rhai port; `schedule_all` is a no-op skeleton).
  `url-funcs` and `window-funcs` are now plain data-type crates consumed by other crates,
  which is fine and needs no wiring.

### Recommendation

**Do it, and treat it as the highest-priority item in this document.** This is not a
nice-to-have: it is a shipped, documented API surface that does not exist at runtime.

Scope-wise it is *mostly* mechanical, not API-design work: `RhaiSetupFunc` is
`fn(&mut Engine) -> anyhow::Result<()>` (`rhai_engine.rs:394`), which every crate's
`register_rhai` already matches exactly. The design decisions (flat globals vs. static
modules, arity-based overloads, sync-over-async via `smol::block_on`) were all already
made and are documented in each crate's `register_rhai` doc comment. What genuinely needs
care:

1. Registration must happen before the first `Config::load()`; `bootstrap()` is already
   the first thing every binary does, and `add_rhai_setup_func` is already used from
   `onlyterm-gui/src/main.rs` immediately after it, so restoring the call inside
   `bootstrap()` is the right place.
2. Registering in `onlyterm` (CLI) and `onlyterm-mux-server` as well as the GUI matches the
   pre-L5 behavior and is safe — `mux_lua`'s functions resolve the `Mux` lazily inside
   each native fn (`get_mux_rhai()`), not at registration time.
3. Name collisions: I checked every `register_fn`/`set_native_fn`/`register_static_module`
   name across `lua-api-crates`, `config/src/rhai_engine.rs` and `onlyterm-gui`'s two setup
   funcs. The only repeats are deliberate arity overloads (`open_with` x2, `glob` x2) and
   per-type `to_string` methods. No crate shadows a `make_rhai_engine` built-in. Note that
   `RHAI_SETUP_FUNCS` runs *last*, so a future collision would silently shadow a built-in
   — worth a comment.
4. There is currently **zero** test coverage of the composed engine.
   `crates/config/tests/rhai_config_smoke.rs` builds a bare `Engine::new()`; the per-crate
   `rhai_smoke.rs` tests each build their own engine and call their own `register_rhai`
   directly. A regression test that builds the real composed engine and asserts the
   headline functions resolve is what prevents this from silently regressing again.

---

## 3. Remaining non-blocking findings from the first `@oh` review round (was TaskList #270)

I re-checked all of these against current `main`. **None of them were incidentally fixed by
the #265-#273 commits.** `crates/window/src/os/windows/window.rs` was not touched by any of
those eight commits at all (verified via `git show --stat` on each), and the parts of
`crates/onlyterm-gui/src/termwindow/mod.rs` they did rewrite were the OpenGL-fallback relay
(#265), `created()`'s error return (#266), device-lost staleness (#267), the unresponsive
flag split (#269) and the `RenderState`-build retry (#272) — none of which intersect the
hang-check scheduling. If anything #267 *widened* the surface for finding 3.3 by adding a
second way to reach `handle_render_error_recovery`.

### 3.1 Destroying the WebGpu child HWND while a live surface may still be bound — STILL LIVE

`TermWindow::begin_renderer_rebuild` (`crates/onlyterm-gui/src/termwindow/mod.rs:1502`)
calls `rt.shutdown()` (line 1512), which by design **does not join** the render thread
(`crates/onlyterm-gui/src/renderthread.rs:84-93, 246-249`). It then drops its own
`Arc<WebGpuState>` (line 1535) — but the render thread holds a second `Arc` via
`RenderThreadSeed::webgpu`, and if it is wedged inside `submit_frame`/`present()` (which is
the whole reason we are rebuilding) that `Arc` — and therefore the `wgpu::Surface` and its
DXGI swapchain — stays alive. Step 3 then destroys the child HWND out from under it
(`termwindow/mod.rs:1568` -> `crates/window/src/os/windows/window.rs:861-863`).

Severity assessment, honestly: less bad than it first looks. wgpu-hal calls
`MakeWindowAssociation(hwnd, DXGI_MWA_NO_WINDOW_CHANGES | DXGI_MWA_NO_ALT_ENTER)`
(`wgpu-hal-25.0.2/src/dx12/mod.rs:1305-1312`), so DXGI is explicitly told not to monitor
the window's message queue — which removes the classic "DestroyWindow re-enters DXGI's
message hook and deadlocks against the in-flight Present" scenario. What remains is
presenting to a destroyed HWND, which DXGI treats as an error rather than a fault in
practice, but which is formally not allowed and is entirely driver-dependent. And this is
the one place in the whole hang-isolation design where the *recovery* path reaches into
the wedged GPU's resources from the GUI thread.

**Recommendation: do it**, using a deferred-destruction scheme rather than trying to join
the render thread (joining would reintroduce exactly the block the architecture forbids).
Concrete approach: on rebuild, *retire* the old child HWND (`ShowWindow(SW_HIDE)` + stash
it) instead of destroying it, and pair the stashed HWND with a `Weak<WebGpuState>`
downgraded from the `Arc` that `begin_renderer_rebuild` takes at line 1535. A retired HWND
becomes safe to `DestroyWindow` exactly when `weak.strong_count() == 0`, i.e. when the
render thread has finally returned, exited its loop and dropped the last `Arc` (which
drops the `Surface`). Sweep the retired list from `check_render_thread_hang_tick` (already
a ~2 s timer while a render thread exists) and from `WindowInner::close`. Since the child
is `WS_CHILD` of the top-level window, anything still retired at window-close time is
destroyed by the OS anyway, so there is no leak even in the worst case.

### 3.2 Stale `webgpu_child_hwnd` when child-window creation fails during a rebuild — STILL LIVE

`Window::recreate_webgpu_child_window`
(`crates/window/src/os/windows/window.rs:841-871`) does:

```rust
let old_child = handle.borrow().webgpu_child_hwnd.0;     // :850
if !old_child.is_null() { unsafe { DestroyWindow(old_child); } }   // :861-863
let new_child = Self::create_webgpu_child_window(parent)?;         // :866  <-- early return
handle.borrow_mut().webgpu_child_hwnd = HWindow(new_child);        // :867
```

If line 866 fails, the `?` returns with `webgpu_child_hwnd` still holding the HWND that was
destroyed at line 862. Windows recycles HWND values, so the subsequent
`SetWindowPos(self.webgpu_child_hwnd.0, ...)` on every resize/move/DPI change
(`window.rs:378-395`) targets a dead — and possibly reassigned — window. It also makes
`webgpu_child_hwnd()` (`window.rs:807-816`) return `Some(dead_hwnd)` instead of `None`, so
the next `WebGpuState::new` builds its surface against it
(`crates/onlyterm-gui/src/termwindow/webgpu.rs:343`) rather than taking the documented
"fall back to the top-level window" path.

**Recommendation: do it.** This is a genuine bug with a three-line fix (null the field
immediately after `DestroyWindow`, set it only on success). Land it before 3.1, or fold it
into 3.1 since 3.1 rewrites the same function.

### 3.3 The hang-supervisor timer chain can fork into two chains — STILL LIVE (narrow)

`schedule_render_thread_hang_check` (`crates/onlyterm-gui/src/termwindow/mod.rs:1110`)
arms a `Timer` -> `notify` -> `check_render_thread_hang_tick` (line 1154), which re-arms
itself. The chain terminates when a tick sees `render_thread_hang_handled == true`
(line 1155) or `render_thread == None` (line 1164).

The fork: `handle_render_error_recovery` (line 1209) — reachable from the render thread's
non-`Lost`/`Outdated` surface errors (`renderthread.rs:442`) and, since #267, from the
device-lost callback — starts a rebuild *asynchronously with respect to the timer*. It sets
`render_thread_hang_handled = true`, and `finish_renderer_rebuild` later resets it to
`false` (line 1705) and starts a **new** chain (line 1698). If the pre-existing chain's
pending tick happens to land *after* that reset rather than during the rebuild, it sees a
healthy render thread, re-arms, and now two chains run in parallel — and the same thing can
happen again on a later episode.

How narrow: the poll interval is `max(threshold/2, 500ms)` = 2000 ms with the default
`render_thread_hang_threshold_ms = 4000` (`crates/config/src/config.rs:2165`), and a
hang-triggered rebuild was measured at 2.3-2.9 s (`termwindow/mod.rs:1295`), so on the hang
path the pending tick almost always lands inside the rebuild and dies. On the
*error-recovery* path against a basically healthy adapter, `WebGpuState::new` can finish
well under 2 s, which is where the fork becomes reachable. The consequence is purely
wasted timers/notifies, and the OpenGL fallback (which leaves `render_thread == None`)
collapses all chains, so accumulation is bounded in practice.

**Recommendation: do it, but as a small task.** The current single-chain property holds
only by timing coincidence, which is a bad thing to rely on. A `Cell<bool>`
`hang_check_scheduled` guard (set in `schedule_...`, cleared at the top of the tick, early
return if already set) makes it structural. ~15 lines; turning the associated function into
a `&self` method is fine — both call sites (`mod.rs:1079`, `mod.rs:1698`) have a
`TermWindow` in scope.

### 3.4 `WriterWrapper`'s unbounded-channel rationale is self-contradictory — STILL LIVE

`crates/mux/src/domain.rs:558-566`:

> "Queue depth is intentionally unbounded, matching `ThreadedWriter`: pty write throughput
> vastly exceeds realistic input rates (typed input, pastes, IME composition), so in steady
> state nothing can out-produce the real write side."

This directly contradicts lines 536-542 of the same comment, which describe the exact
motivating case: a child process that is not reading its stdin, so the pipe buffer is full
and the real write side makes *zero* progress. The second half of the paragraph (lines
561-566: bounding the queue would either block the caller or drop data, which is precisely
what this type exists to avoid) is the correct and sufficient argument on its own.

**Recommendation: do it (doc-only).** Rewrite the first sentence to state the real
position: unbounded growth *is* possible when a child stops draining stdin, it is bounded in
practice by human input rate and paste size, and it is the deliberately chosen trade against
reintroducing a blocking or lossy write path. No code change.

### 3.5 Nit: the lock-contention test's calibration comment cites a range its own table contradicts — STILL LIVE (trivial)

`crates/mux/src/test/terminal_lock_contention.rs:550-551` says the healthy ratio "stays in
the 33x-264x range"; line 570-571 says it "never dropped below 33x". The measurement table
immediately below (lines 552-557) has a minimum of **34x**, not 33x. The threshold itself
(`MIN_HOLD_TIME_REDUCTION_RATIO = 8`, line 577) and the "clean separation" conclusion are
correct and unaffected. One-character fix.

### 3.6 Nit: the changelog scopes an unconditional change to Windows — STILL LIVE

`docs/changelog.md:301` opens with "On Windows, the pty reader and output-parser threads
now hand output to each other via an in-process channel instead of a loopback TCP socket
pair". The change (commit `0ba6e9b68`) deleted **both** the `#[cfg(unix)]` and the
`#[cfg(windows)]` variants; `crates/mux/src/lib.rs:413` creates a
`crossbeam::channel::bounded` unconditionally on every platform. The parenthetical about
`socketpair()` falling back to loopback TCP is Windows-specific and worth keeping, but the
change itself is not.

### 3.7 Nit: `docs/multiplexing.md` uses Lua `=` syntax inside its rhai section — STILL LIVE

The "Surviving a crashed or killed GUI process" section opens with an explicit note
(`docs/multiplexing.md:170-176`) that everything above predates the rhai migration and that
"the recipe below is written directly in rhai", followed by a ```` ```rhai ```` block using
`:` correctly (lines 188-194). But line 222 then says "unless you pass
`no_serve_automatically = true` on the domain" — Lua assignment syntax, inside the rhai
section. Should be `no_serve_automatically: true`. (Lines 70/75/83 also use `=`, but those
are inside the older Lua block that the note explicitly disclaims, so they are fine as-is.)

### 3.8 Nit: the handshake token comparison is not constant-time — DO NOT DO

`crates/filedescriptor/src/windows.rs:758` does `received == *token` on a `[u8; 16]`
(`HANDSHAKE_TOKEN_LEN = 16`, line 543). I am recommending **against** changing this:

* There is no oracle. A mismatch closes the connection and the loop keeps waiting
  (`window.rs`-adjacent logic at `windows.rs:696-703`), so an attacker cannot distinguish
  "wrong token" from "you lost the race to connect" in any load-bearing way.
* The attacker must win a race against our own `client`, which connects microseconds after
  `listen()`, within a 2 s total budget (`HANDSHAKE_TOTAL_BUDGET`, line 550).
* A fixed-size 16-byte array comparison lowers to one or two word compares; the timing
  delta is ~1 ns, buried under loopback socket and scheduler noise measured in microseconds.

If a future reviewer raises it again, the three-line xor-fold (`a.iter().zip(b).fold(0u8,
|acc, (x, y)| acc | (x ^ y)) == 0`) is cheap and needs no new dependency — but it is not
worth a task now.

---

## Proposed task decomposition

Ordered roughly by value. Dependencies are called out explicitly; everything else is
independent.

1. **Wire the rhai API crates into the live engine via `env-bootstrap`**
   Restore the equivalent of the `register_lua_modules()` function that commit `48e38e224`
   deleted, as `register_rhai_modules()` in `crates/env-bootstrap/src/lib.rs`, calling
   `config::rhai_engine::add_rhai_setup_func` for each of `battery`, `color_funcs`,
   `filesystem`, `logging`, `mux_lua`, `plugin`, `procinfo_funcs`, `serde_funcs`,
   `share_data`, `spawn_funcs`, `termwiz_funcs` (the 11 crates that have a `register_rhai`;
   `time-funcs`/`url-funcs`/`window-funcs` have none), and call it from `bootstrap()` so all
   three binaries get it before the first `Config::load()`. Today none of these reach a live
   engine at all, so `format(...)`, `run_child_process(...)`, `mux::*`, `serde::*`,
   `plugin::*`, `log_info(...)` and everything else the docs advertise fail with "function
   not found" in a real `.rhai` config. Note in a comment that `RHAI_SETUP_FUNCS` runs after
   `make_rhai_engine`'s own built-ins, so a future name collision would silently shadow one.

2. **Add an end-to-end regression test for the composed rhai engine**
   Depends on task 1. There is currently zero coverage of the engine that
   `config::rhai_engine::make_rhai_engine` actually produces — `config/tests/rhai_config_smoke.rs`
   uses a bare `Engine::new()` and every `lua-api-crates/*/tests/rhai_smoke.rs` calls its own
   `register_rhai` directly, which is exactly why the L5 regression went unnoticed. Add a
   test that registers the setup funcs the way `bootstrap()` does, builds an engine via
   `make_rhai_engine`, and asserts that a representative function from each wired crate
   resolves (e.g. `format`, `run_child_process`, `read_dir`, `log_info`, `serde::json_encode`,
   `plugin::list`, `mux::get_workspace_names`, `battery_info`) rather than raising
   `ErrorFunctionNotFound`.

3. **Audit and correct the rhai API documentation against what is actually registered**
   Depends on task 1. Once the wiring lands, reconcile the docs with reality: fix
   `crates/onlyterm-gui/src/overlay/debug.rs:33-35`, which claims the REPL exposes
   "`onlyterm.mux.*`, `GuiWin`, `TabInformation`" when only the latter two were ever true;
   and document the genuinely-missing surface — `onlyterm.time.call_after` has no rhai port
   at all (`crates/lua-api-crates/time-funcs/src/lib.rs:13-18`), and `onlyterm.emit` is
   deliberately not script-visible (`crates/config/src/rhai_engine.rs:358-372`) — in
   `docs/migration-lua-to-rhai.md` so users are not left guessing.

4. **Fix the stale `webgpu_child_hwnd` left behind when a rebuild's child-window creation fails**
   In `Window::recreate_webgpu_child_window` (`crates/window/src/os/windows/window.rs:841-871`),
   `create_webgpu_child_window` failing at line 866 returns early with `webgpu_child_hwnd`
   still holding the HWND destroyed at line 862. Every subsequent resize/move/DPI change then
   calls `SetWindowPos` on a dead (and possibly Win32-recycled) handle at `window.rs:386`, and
   `webgpu_child_hwnd()` returns `Some(dead)` instead of `None`, defeating the documented
   "fall back to the top-level window" path. Null the field immediately after `DestroyWindow`
   and assign the new handle only on success.

5. **Defer destruction of a retired WebGpu child HWND until the old `WebGpuState` is dropped**
   Should land after task 4 (same function). `begin_renderer_rebuild`
   (`crates/onlyterm-gui/src/termwindow/mod.rs:1502-1583`) deliberately does not join the render
   thread, so that thread's `Arc<WebGpuState>` — and its live DXGI swapchain — can still be
   alive, possibly mid-`present()`, when `recreate_webgpu_child_window` destroys the child HWND
   the swapchain targets. Change the rebuild to *retire* the old child window (`SW_HIDE` + stash)
   instead of destroying it, pairing each stashed HWND with a `Weak<WebGpuState>` downgraded
   from the `Arc` taken at `mod.rs:1535`; destroy a retired HWND only once
   `Weak::strong_count() == 0`, sweeping from `check_render_thread_hang_tick` and
   `WindowInner::close`. Anything still retired at window close is destroyed by the OS with the
   parent, so there is no leak.

6. **Hoist `create_uniform` out of the per-draw loop in `WebGpuState::submit_frame`**
   `crates/onlyterm-gui/src/termwindow/webgpu.rs:841` builds a fresh uniform buffer and bind
   group inside the `for draw in &frame.draws` loop (line 815), even though `frame.uniform` is
   a `Copy` value identical for every draw in the frame. Build it once before the loop and
   reuse the resulting `wgpu::BindGroup`. Saves one `create_buffer_init` plus one
   `create_bind_group` per extra draw call per frame on the render thread; a three-line change
   with no behavioral difference.

7. **Instrument the per-frame WebGpu vertex-buffer churn**
   Add `metrics` histograms around the `recreate()`/`unmap()` step in
   `TermWindow::build_webgpu_frame` (`crates/onlyterm-gui/src/termwindow/render/draw.rs:65-82`):
   one latency histogram for the whole loop and one `*.size`-suffixed histogram (see the
   `.size` special case in `crates/onlyterm-gui/src/stats.rs`) for total bytes re-created per
   frame, i.e. the sum of each recreated buffer's capacity. This is the measurement that
   "221.9 pending measurement" was actually waiting on; it is readable via the existing
   `periodic_stat_logging` config knob with no new tooling. The decision rule: pursue the
   full buffer-pooling redesign only if per-frame bytes regularly exceed ~8 MB or the
   recreate+unmap step regularly exceeds ~2 ms on a real workload.

8. **Collapse the vestigial 3-slot vertex-buffer rotation in the WebGpu path**
   `TripleVertexBuffer::bufs` (`crates/onlyterm-gui/src/renderstate.rs:321-327`) keeps three
   buffers per sub-layer and `next_index()` rotates them once per frame
   (`crates/onlyterm-gui/src/termwindow/render/draw.rs:81`). That rotation is meaningful only for
   Glium; in the WebGpu path `recreate()` swaps a brand-new buffer into the current slot every
   frame and hands the filled one to the render thread, so a rotated-to slot never holds a
   buffer the GPU has seen and one slot behaves identically to three. Make the slot count
   backend-dependent (1 for WebGpu, 3 for Glium) to reclaim roughly two-thirds of the
   vertex-buffer upload-heap memory per window — on the order of 6 MB for a large window's
   glyph sub-layer alone. Verify by comparing the reported buffer footprint before/after; do
   not change the Glium path's behavior.

9. **Make the render-thread hang-check timer chain single-instance**
   `schedule_render_thread_hang_check`/`check_render_thread_hang_tick`
   (`crates/onlyterm-gui/src/termwindow/mod.rs:1110-1182`) rely on timing alone to stay
   single-chained: a rebuild started from `handle_render_error_recovery` (line 1209) resets
   `render_thread_hang_handled` to `false` and arms a fresh chain (lines 1698-1705) while the
   previous chain's timer may still be pending, so a tick landing after the reset re-arms and
   forks the chain. Add a `Cell<bool>` `hang_check_scheduled` guard — set when scheduling,
   cleared at the top of the tick, early-return if already set — so exactly one chain exists by
   construction rather than by coincidence. Purely wasted timers today, but the invariant should
   be structural.

10. **Comment and documentation accuracy fixes (no code behavior change)**
    Four independent, small corrections found by the first review round and re-verified as still
    live: (a) rewrite `WriterWrapper`'s unbounded-channel rationale at
    `crates/mux/src/domain.rs:558-566`, whose claim that "pty write throughput vastly exceeds
    realistic input rates" contradicts the very scenario the type exists for (a child not
    draining stdin), keeping only the correct argument that bounding the queue would reintroduce
    blocking or data loss; (b) fix "33x" to "34x" in
    `crates/mux/src/test/terminal_lock_contention.rs:550-551` and 570-571, which cite a minimum
    their own measurement table contradicts; (c) drop the "On Windows," qualifier from
    `docs/changelog.md:301` — commit `0ba6e9b68` removed both the `cfg(unix)` and `cfg(windows)`
    socketpair paths and `crates/mux/src/lib.rs:413` uses the crossbeam channel on all platforms;
    (d) change `no_serve_automatically = true` to `no_serve_automatically: true` at
    `docs/multiplexing.md:222`, which is inside the section explicitly declared to be written in
    rhai.
