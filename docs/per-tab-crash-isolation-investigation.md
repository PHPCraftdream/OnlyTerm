# Per-tab GPU crash isolation — investigation and design options

Status: investigation + design proposal, not a plan approved for implementation.
Written 2026-08-21, in response to task #640 and the user's explicit pushback on
that task's conclusion. Every claim below is grounded in code actually read on
this checkout (`main`, HEAD `dac4fefa9`, plus uncommitted `c6a5dae52` — see
"Verified current state" for exact citations) or in web research actually
performed (cited inline). Where something could not be verified, that is stated
explicitly rather than smoothed over.

## 0. The question this document answers

The user, after being told crash #3 (PID 27376, raw SEH from inside DXGI/D3D12)
would take down the *entire* process, pushed back:

> мы долго сделали, чтобы каждая вкладка жила отдельно и падала сама - и тут ты
> говоришь, что этого нет. Нужно чтобы каждая вкладка жила сама и если внутри
> что-то упало - пусть корректно падает, а если поверх этого нужно через CPU
> вывести копируемый текст падения на экран и оставить вкладку с этой
> "эпитафией" - можно только скопировать текст и закрыть вкладку. Соседние
> вкладки должны спокойно жить

Two things need independent verification before any design can be trusted:

1. Is task #640 actually right that GPU rendering is process-wide and
   unisolated? (The user's memory of "we spent a long time on this" could be
   about a *different* kind of isolation that actually shipped — need to check
   which.)
2. What would it actually take to make one tab's render crash not affect its
   siblings, and what would the "epitaph" screen require?

## 1. Verified current state

### 1.1 Two isolation efforts exist in this codebase. They solve different problems.

**Effort A — `per_tab_process_isolation` (shell/pty isolation).** Real,
substantial, well-documented, shipped feature. Confirmed by reading
`docs/per-tab-hosting-architecture.md` (dated 2026-08-14) end to end, plus the
commits it cites:

- `7e8c763cf` Phase A — single-pane hosting process prototype
- `6aa69eaee` Phase B — per-tab single-pane process isolation for SpawnTab
- `66118a515` Phase C — elevated (admin) single-pane tabs
- `c2ba549e6` Phase D — hosting child's lifetime tied to the GUI's
- `94de066db` Phase E — security review (found nothing to harden; documented why)
- `f6773d023` Phase F — cost measurement + architecture doc

Config flag: `per_tab_process_isolation: bool`, `#[dynamic(default)]` →
**defaults to `false`** (`crates/config/src/config.rs`). Gate:
`crates/onlyterm-gui/src/spawn.rs`:
```rust
let use_single_pane =
    config.per_tab_process_isolation && !matches!(spawn_where, SpawnWhere::SplitPane(_));
```
When enabled, a regular tab spawns `onlyterm-mux-server.exe --single-pane` as a
genuinely separate OS process. That child:
- owns the PTY and the shell process for **that one pane**,
- talks back to the GUI over an inherited `socketpair()` (or, for elevated
  tabs, a loopback WebSocket rendezvous — `crates/onlyterm-elevated-transport`),
  relaying the mux **PDU protocol** (pane content, resize, input) — not pixels,
  not GPU state,
- is bound to the GUI's lifetime via a per-child Windows Job Object with
  `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` (`crates/onlyterm-client/src/client/windows_job.rs`)
  for ordinary children, or a `--supervise-pid` parent-watcher thread
  (`crates/onlyterm-mux-server/src/main.rs`, `spawn_parent_watcher`) for elevated
  children that Job Objects can't reach across the integrity boundary.

What crosses the process boundary here: **pty bytes / mux PDUs**, nothing
GPU-related. The terminal model (`Tab`, `Pane`, the `Terminal` state machine)
for a single-pane child lives in the child process, but **all GPU rendering of
that pane's content still happens in the GUI process**, via the normal
`ClientDomain` → mux protocol → `TermWindow` rendering path, identical to any
other pane. This is explicit in the architecture doc itself and matches what
task #640 claimed.

**Effort B — "wedged-pane isolation" (mutex/CPU isolation, not GPU).** This is
almost certainly what the user is remembering as "we spent a long time making
each tab live independently" — it's real, it's substantial (tasks #244–#256,
commit `e7ea99e86` adds `crates/mux/src/test/wedged_pane_isolation.rs`), and it
predates and is unrelated to the GPU crash work. It proves: if one pane's
`Terminal::lock()` is genuinely wedged (a stuck reader thread holding the
mutex), the GUI thread's per-pane accessors (`has_unseen_output`,
`is_unresponsive`, `get_title`, `pane.writer()`, etc.) use bounded fallbacks so
a wedged pane does not block input or rendering for *other panes in the same
window*. This is real isolation, but it is isolation of **CPU-side terminal
state access**, not of GPU rendering, and it does not involve a process
boundary — the healthy pane still shares the wedged pane's window, render
thread, and `RenderState`.

There is also **window-level hang supervision** for GPU work specifically
(execution-decoupling initiative, tasks #218–#226, commits `b027e5d2`…
`1fa8df741` and later `dac4fefa9`, `c6a5dae52`): each window with an active
render thread runs a self-rescheduling timer that polls
`render_thread_is_hung()` / `render_thread_has_died()`, and on a confirmed
hang/death/repeated-error episode, `attempt_renderer_rebuild_or_close`
(`crates/onlyterm-gui/src/termwindow/render_pipeline.rs:715`) either rebuilds
the renderer in place or, if a circuit breaker trips (`MAX_REBUILDS_PER_WINDOW`
within `REBUILD_WINDOW`), closes **the whole window**:
```rust
fn close_window_for_unrecoverable_render_hang(&mut self, window: &Window) {
    let mux = Mux::get();
    mux.kill_window(self.mux_window_id);   // kills every pane's process tree in this window
    window.close();
    front_end().forget_known_window(window);
    ...
}
```
(`render_pipeline.rs:819-825`). `mux.kill_window` kills every pane's process
tree in that window — every tab, every split, all of them — not just the one
that triggered the hang. The commit `47f059ab4` ("manual end-to-end
verification of GPU-hang window isolation", task #226) explicitly records:
*"не проверена изоляция вкладок ВНУТРИ одного окна (недостижима на этой
архитектуре)"* — tab-within-window isolation was not verified **because it is
not achievable on this architecture**. That line, written by an earlier
session, already independently agrees with task #640's conclusion.

**Correction/confirmation of task #640:** task #640 is correct. Neither
existing isolation effort protects GPU rendering at tab (or even
window-to-window... see below) granularity within the failure mode that
matters here. The user's memory of substantial deliberate work on "each tab
lives on its own" is accurate and refers to Effort A (shell/pty process
isolation) and the wedged-pane CPU isolation — real work, real discipline
(prototype → measure → security review → phased rollout, see §6) — but neither
one is a GPU isolation mechanism, and task #640 never claimed otherwise; it
specifically named `per_tab_process_isolation` and said what it does and
doesn't cover. There is no contradiction between the two; they're about
different subsystems.

### 1.2 GPU state is confirmed process-wide, not just window-wide

`crates/onlyterm-gui/src/termwindow/webgpu/context.rs:214-219`:
```rust
/// Process-wide GPU context shared by all windows.
/// Created once per process on first window creation and reused thereafter.
///
/// Contains resources that are expensive to initialize and don't depend on
/// any specific window: Instance, Adapter, Device, Queue, shader module,
/// bind group layouts, samplers, and a pipeline cache keyed by surface format.
pub struct ProcessGpuContext { ... }
```
`ProcessGpuContext::get_or_init` (`context.rs:279`) uses a single
process-global `static CONTEXT_LOCK: smol::lock::Mutex<Option<Arc<ProcessGpuContext>>>`
— one `Instance`/`Adapter`/`Device`/`Queue` for the whole `onlyterm-gui.exe`,
shared across every window. Per-window state is limited to `WindowGpuSurface`
(`context.rs:702`, just the `wgpu::Surface` + its config + dimensions) and
`RenderState`/`render_thread`, both fields of `TermWindow`
(`crates/onlyterm-gui/src/termwindow/mod.rs:315,481` —
`render_state: Option<RenderState>`, `render_thread: Option<crate::renderthread::RenderThreadHandle>`).
There is exactly one `RenderState` and one render thread **per window**, not
per tab: `mux::Tab` (`crates/mux/src/tab.rs`) holds multiple `Pane`s (splits)
and a window can hold multiple tabs, but all of them funnel through the same
`TermWindow`'s single `render_state`/`render_thread`. Confirmed by grep: only
one `render_state`/`render_thread` field declaration in `termwindow/mod.rs`,
and `Tab`/`TabInner` in `mux/src/tab.rs` has no rendering fields at all — it's
pane-tree bookkeeping only.

New window creation (Ctrl+Shift+N, `SpawnWindow`) is confirmed
same-process: `crates/onlyterm-gui/src/termwindow/actions.rs:1159-1161`
```rust
SpawnWindow => {
    self.spawn_command(&SpawnCommand::default(), SpawnWhere::NewWindow);
}
```
`self.spawn_command` (`crates/onlyterm-gui/src/termwindow/spawn.rs:7`) runs on
the existing `TermWindow`/GUI process; it does not launch a new
`onlyterm-gui.exe`. So even **window**-level isolation, let alone tab-level,
does not currently exist for GPU rendering — every window in the process
shares `ProcessGpuContext`, and a `device_lost`/`uncaptured_error` on that
shared `Device` notifies *every* window's `DeviceLostSubscriber`
(`context.rs:41-101`), not just one.

## 2. Blast radius of the three known crashes, re-derived

| # | Failure | Where caught today | What actually dies |
|---|---|---|---|
| 1 | OOM → renderer rebuild → AV in `igd10iumd64.dll` (PID 33104) | Fixed: `surface_error_needs_renderer_rebuild` no longer rebuilds on `OutOfMemory`; `rebuild_backoff_for_attempt` spaces retries (`dac4fefa9`) | Before the fix: the whole process (unhandled AV inside a driver call reached from the render thread is not contained by anything — no VEH installed for this, no process boundary). After the fix: nothing, the failure mode is avoided rather than caught. |
| 2a | Panic inside `Surface::present` (PID 39192) | `catch_unwind` around `submit_frame` in `submit_one_frame` (`renderthread.rs:638`, `dac4fefa9`) | Before: silently killed the render thread only, but the window's message loop kept running and painting nothing forever (looked "responsive" to Windows, was actually dead) — a **hang**, not a crash, and it affected the whole window (all its tabs stopped repainting), not just one pane. After: caught, logged, turned into `SurfaceError::Other`, render thread survives. |
| 2b | GUI thread frozen during driver reinstall (`begin_renderer_rebuild` synchronously dropping `RenderState`) | Fixed via `mem::forget` + background-thread drop of `WebGpuState` (`dac4fefa9`) | Before: the entire GUI thread — every window in the process, since the GUI thread is process-wide — could hang forever with no supervisor able to reach it (the hang supervisor is a render-thread mechanism; a frozen GUI thread can't even run it). After: GUI thread never blocks on this drop. |
| 3 | Raw SEH (`0xC0000002`, `STATUS_NOT_IMPLEMENTED`) from inside DXGI/D3D12 during `Surface::configure` (resize), via `wgpu_hal::dx12::impl$74::configure` (PID 27376) | **Not caught by anything.** `catch_unwind` was added around the `resize` call by `c6a5dae52` for the *Rust-panic* case, but its own comment (`renderthread.rs:500-513`) states plainly that a foreign SEH exception "propagates straight through `catch_unwind` by design." | The entire process. Confirmed structurally: an unhandled SEH exception terminates the process by default on Windows (no per-thread granularity exists for an *unhandled* one — `UnhandledExceptionFilter` runs once, for the whole process, and its default action is process termination). Since `ProcessGpuContext` is one Instance/Device/Queue for the whole process and every window/tab draws through it, and since resize is a per-window operation reached from any window's render thread, **any window's resize can kill every window's every tab** — this is not "the tab that resized dies," it's "the process dies." |

Crash #3's blast radius is the whole process, confirmed both by the general
Windows SEH-termination model and by the fact that no code anywhere in this
repository intercepts a non-debug-string vectored exception (see §3) or runs
GPU work in a separate process (see §1). No existing boundary — not
`per_tab_process_isolation`, not wedged-pane isolation, not window-level hang
supervision — would have contained it, because none of them are a GPU-crash
process boundary. Window-level hang supervision (§1.1) reacts to Rust panics
and device-lost/uncaptured-error *callbacks*; a raw SEH from inside the driver
during `configure` never reaches any of those recovery paths, because the
thread it's on is already gone by the time anything could react.

## 3. SEH-interception feasibility research

### 3.1 What's already in this codebase

`crates/wgpu-hal-vendored/src/auxil/dxgi/exception.rs` installs exactly one
Vectored Exception Handler process-wide
(`AddVectoredExceptionHandler(0, Some(output_debug_string_handler))`,
reference-counted via `EXCEPTION_HANDLER_COUNT` so there's never more than
one). Read in full: it only recognizes `DBG_PRINTEXCEPTION_C` /
`DBG_PRINTEXCEPTION_WIDE_C` — the codes `OutputDebugString`-based D3D12 debug
layer messages use to smuggle text through the debugger — and for those it
parses a `"D3D12 <LEVEL>: ..."` prefixed string out of
`ExceptionRecord.ExceptionInformation`, logs it, and returns
`EXCEPTION_CONTINUE_EXECUTION`. **Every other exception code, including any
real D3D12/DXGI fault, falls through to `return Debug::EXCEPTION_CONTINUE_SEARCH`**
— i.e. "not my problem, let the next handler (or the default unhandled-exception
path) deal with it." This handler was never designed to, and does not, recover
from real access violations or `STATUS_NOT_IMPLEMENTED`-class faults. It's a
debug-string sniffer, not a crash barrier.

### 3.2 Is `EXCEPTION_CONTINUE_EXECUTION` viable for a real driver fault?

Researched via web search (see Sources). The consistent, authoritative
position — including Raymond Chen's ("The Old New Thing," Microsoft) writing on
this exact question — is **no, not in general, and specifically not for an
access violation or an unexpected status code raised from inside someone
else's code (here: a closed-source Intel/Microsoft driver stack you don't
control)**:

- Resuming execution after an exception via `EXCEPTION_CONTINUE_EXECUTION`
  requires the handler to know precisely what state the faulting instruction
  left behind and either fix that state or safely skip forward — this is only
  reliably done for a small, deliberately engineered class of cases (guard
  pages for stack probing, JIT self-modifying-code page faults, structured
  null-check elision patterns). For an arbitrary fault at an arbitrary point
  inside opaque driver code, there is no way to know what partial writes,
  partially-acquired locks, or partially-updated GPU command-buffer state
  exist at the fault point.
- Chen's writing (Old New Thing posts on exception handling and resumability)
  states plainly that generic Win32 code is not exception-safe across frames
  you don't control, and that "if someone unwisely tries to handle this
  exception, the result is that the thing you tried to protect against ended
  up happening anyway" — i.e. swallowing the exception doesn't undo the
  corruption, it just defers or hides its consequences.
- `STATUS_NOT_IMPLEMENTED` (`0xC0000002`) specifically, raised from *inside*
  `dxgi.dll`/`d3d12.dll`/the vendor driver during `configure`, is not a
  documented, recoverable wgpu/D3D12 error path — it's the kind of fault a
  raw VEH would see only after something already went wrong at the API/driver
  boundary. Attempting `EXCEPTION_CONTINUE_EXECUTION` here does not "skip the
  bad call" — execution resumes at the faulting instruction, so without also
  rewriting the instruction pointer/registers, it does not even avoid
  re-faulting; and rewriting them safely requires understanding of the fault
  that a generic handler cannot have.
- A VEH can safely intercept, log, and continue only when the handler
  authored that exact recovery for that exact fault site with full knowledge
  of pre/post state (as this codebase's own `exception.rs` does — but only
  for a debug *print* message, which is not a fault at all, just an
  `OutputDebugString`-style side channel abusing the exception mechanism for
  IPC, and is explicitly documented as such in Microsoft's own guidance for
  handling debug-layer output).
- What a VEH *can* do safely and is architecturally different from "recover
  and continue": act as a **last-chance crash reporter** — capture context
  (call stack, exception record) and then let the process die anyway (return
  `EXCEPTION_CONTINUE_SEARCH` or explicitly terminate), which is a legitimate,
  common use (minidump writers, crash telemetry) but does **not** achieve "the
  rest of the process keeps running."

### 3.3 How Chromium actually solves this

Fetched `chromium.org`'s GPU-accelerated-compositing design doc directly
(see Sources). Confirmed explicitly: Chromium's GPU isolation is a **genuine
separate OS process**, not an in-process recovery mechanism. The renderer
process is the client; the GPU process is the server; they communicate over a
shared-memory command buffer (an IPC boundary, not a shared address space).
"A GPU process crash (e.g. due to faulty drivers) doesn't bring down the
browser" because the browser process supervises the GPU process as an
external, monitored child — when it dies (by any means, including a raw SEH
the GPU process itself never tries to catch), the browser process observes
process termination through the OS (broken pipe / process handle signaled) and
relaunches a fresh GPU process. **The crash is not intercepted mid-flight; the
crashed process is simply allowed to die, and a new one takes over.** This is
architecturally the same pattern this codebase already uses for
`per_tab_process_isolation`'s hosting children (Job Object / `--supervise-pid`
lifetime binding, §1.1) — just applied to GPU work instead of PTY/shell work.

### 3.4 Honest confidence level

High confidence that **in-process SEH interception cannot be made
generally safe** for this fault class, based on: (a) this codebase's own VEH
already only ever recovers a non-fault debug-string case and explicitly
`CONTINUE_SEARCH`s everything else, (b) authoritative Microsoft-adjacent
guidance (Raymond Chen) stating resumability is not generally available for
arbitrary faults in code you don't control, and (c) Chromium — the most
scrutinized GPU-crash-hardened multi-process application that exists — solving
this with a real process boundary, not exception interception. This is not
asserted from first principles alone; it is corroborated by both a real
precedent codebase's own design choice (`exception.rs`'s narrow scope) and
external authoritative sources. Residual uncertainty: it is *conceivable* that
some subset of DXGI/D3D12 faults are safely resumable in narrow, specifically-
engineered circumstances (e.g. certain debug-layer-only paths) — but
`STATUS_NOT_IMPLEMENTED` from inside `configure`'s actual driver call is not
such a case, and building/maintaining a safe-subset classifier would itself be
a large, driver-version-fragile undertaking with no existing precedent in this
codebase or (as far as this research found) in Chromium's approach either.

## 4. Design options

### (a) True per-window (or per-tab, or per-pane) OS-process isolation for GPU rendering

**What it would look like:** a new "GPU host" child process type
(`onlyterm-gui.exe --gpu-host` or similar, reusing the existing subprocess/
supervision plumbing pattern), owning its own `wgpu::Instance`/`Device`/
`Surface`, rendering into a texture, and sharing that texture back to the
parent's window surface via `IDXGIResource1::CreateSharedHandle` +
`DXGI_SHARED_RESOURCE_READ` (cross-process shared D3D11/D3D12 resource
handles — a real, long-standing DXGI feature for exactly this purpose:
compositing surfaces produced in one process into a swapchain owned by
another). **Caveat, stated honestly:** the claim that this is "already
confirmed technically viable by earlier research" comes from the task prompt,
not from anything I could find committed in this repository — I grepped the
whole tree for `CreateSharedHandle`/`DXGI_SHARED_RESOURCE`/shared-swapchain
usage and found **zero matches**. If that research happened, it isn't
persisted anywhere I could read (possibly still in an uncheckpointed
conversation, like the PID 27376 crash analysis itself). Treat the technical
viability of the shared-handle approach as **plausible but unverified by this
investigation** — worth a real prototype before committing to it, not a
foregone conclusion.

**Granularity: per-window, not per-tab, not per-pane.** Justification:
`RenderState`/render thread/`WindowGpuSurface` are already scoped per-window
today (§1.2) — a tab and its splits inside one window already share one
surface, one swapchain, one set of render calls per frame (the renderer draws
the whole window's tab bar + active tab's pane tree in a single frame; tabs
are a *content* concept, not a *surface* concept). Isolating at tab granularity
would require either (i) one GPU child process *per tab*, each owning its own
swapchain, compositing multiple swapchains into the one HWND — expensive
(many device/swapchain instances, texture-sharing overhead scales with tab
count) and architecturally foreign to how `TermWindow` currently draws — or
(ii) one child per *window* that still draws all its tabs in one frame, same
as today, just relocated across a process boundary. Option (ii) is far
cheaper, and it already satisfies "a crash in one tab's rendering doesn't kill
other *windows*" — but it does **not** satisfy "a crash in one tab's rendering
doesn't kill other *tabs in the same window*," because all tabs in a window
still share that window's one GPU child process and one swapchain. Making
sibling tabs in the *same* window survive a resize-triggered SEH would need
per-tab child processes (option i) — the user's literal ask (each **tab**
independent) requires the more expensive granularity, not the cheaper one.

**Tradeoffs:**
- Cost: `docs/per-tab-hosting-architecture.md`'s own measurements for the
  *shell*-hosting children (a much lighter workload than a GPU device) were
  ~46 MB working set and ~750 ms cold-start per child, debug build, Idle
  priority (explicitly flagged as a lower-bound, not a user-facing number). A
  GPU-hosting child would carry a full `wgpu::Device`/`Adapter` — materially
  heavier than a PTY-hosting child; expect the "GPU Instance/Adapter/Device
  ready" startup cost (already logged and known to be significant enough that
  a whole separate investigation, `docs/investigations/2026-08-11-dxgi-adapter-
  selection-plan.md`, exists to shave it down) to be paid **per tab** instead
  of once per process. At even a handful of tabs this is a large regression
  unless adapter/device creation itself can be shared (it likely cannot be,
  across a process boundary, without its own IPC design).
- Risk: cross-process shared-texture compositing on Windows has known sharp
  edges (synchronization between producer/consumer, format/alpha-mode
  matching, DXGI_SHARED_RESOURCE flag support varies by driver) — this is new
  surface area, not a reuse of anything currently working in this codebase.
- What it covers: genuinely contains crash #3's failure class — if the SEH
  happens in the child, the child dies, the parent observes process death
  (same supervision pattern as `per_tab_process_isolation`) and can put up the
  epitaph screen for that tab's area without the parent's own GPU context
  (used by every *other* tab/window) ever being touched.
- What it doesn't cover for free: needs its own security review (Phase E
  equivalent) since a new IPC surface for texture handles is being added;
  needs its own cost-measurement Phase (F equivalent) before any decision on
  defaults.

### (b) In-process recovery via SEH interception at the render thread's GPU call boundary

Per §3: **not recommended as the answer to crash #3.** A VEH could be
installed at the render thread's driver-call sites to *observe and report*
(crash telemetry, "this window's renderer died with exception code X at
address Y") before allowing the process to terminate — genuinely useful for
diagnostics, and cheap to add — but `EXCEPTION_CONTINUE_EXECUTION` to keep the
*process* alive after an arbitrary DXGI/D3D12 fault is not something this
investigation found any reliable basis for, in this codebase's own precedent
(`exception.rs`'s narrow, non-fault-only scope), in authoritative external
guidance (Chen), or in the industry's own reference solution (Chromium, which
uses a process boundary specifically because in-process recovery from this
fault class isn't trusted). This option should not be pursued as a
crash-containment mechanism. It remains reasonable as a **diagnostics-only**
addition (log-and-let-die, not log-and-continue) — separate, smaller, and
much lower-risk than either (a) or (c).

### (c) Extend `per_tab_process_isolation`'s hosting process to also host that tab's rendering

**Attractive on paper — reuse a process that's already spawned and supervised
— but the analysis above (§1.1, §1.2) shows why it doesn't actually reduce the
work much:**

- `per_tab_process_isolation` hosting children exist **only when the flag is
  on**, which defaults to `false` — most users, most of the time, run with
  zero hosting children. Any design that only protects tabs when this
  unrelated, off-by-default flag is also enabled would leave the reported bug
  (crash #3) unaddressed for the default configuration, which is presumably
  the configuration it was hit in.
- Hosting children today carry the **pty/mux protocol layer only** — they have
  no `wgpu`/`Instance`/`Device` at all, and critically, per §1.2, GPU state in
  this codebase is currently designed around a single process-wide
  `ProcessGpuContext`. Moving rendering into the hosting child means the
  hosting child would need its *own* `Instance`/`Adapter`/`Device` (since it's
  a separate process, it cannot share the GUI's `ProcessGpuContext` — that's
  an in-process `Arc`, not something `CreateSharedHandle`-able by itself) —
  which is the same GPU-device-per-tab cost problem as option (a), just
  arrived at from a different starting point. It is not meaningfully cheaper
  than (a); it inherits (a)'s device-per-tab cost concern and *adds* the
  complexity of two different transports (mux PDU for pty content, a new
  shared-texture channel for pixels) inside what's already a fairly intricate
  hosting-child protocol (`docs/per-tab-hosting-security-model.md` describes
  the existing PDU allow-list and rendezvous-token hardening — a new
  texture-sharing channel would need its own equivalent review).
- The one real thing (c) buys over a from-scratch (a): the **supervision
  primitives** (Job Object binding, `--supervise-pid` watcher,
  `pending_single_pane_spawns` dedup-guard pattern) are proven and can be
  copied/adapted rather than re-invented. That's a template win, not an
  architecture win — a new "GPU host" process type in option (a) should still
  reuse those same primitives; it doesn't need to literally *be* the existing
  hosting child to do so.

### Recommendation

Given the user's explicit ask (**tab**-level independence, not window-level),
option (a) at **per-tab granularity** is the only option that actually
satisfies the requirement — but it is also the most expensive, least-verified
(no shared-texture prototype exists in this repo despite the earlier-research
claim), and highest-risk option on the table, and it wasn't reachable by
reusing the existing hosting-child machinery at low cost (option (c) doesn't
meaningfully cheapen it). Option (b), the cheapest-looking option, is
disqualified for the actual crash-containment goal by §3's findings — it can
stay in scope only as a diagnostics addition.

Recommendation: **do not commit to full per-tab GPU-process isolation yet.**
Before any implementation:

1. A real, small, throwaway prototype of the `CreateSharedHandle` cross-process
   compositing path (a single quad rendered in a child process, shared into a
   parent's swapchain) is needed to convert "plausible but unverified" into a
   measured yes/no — this is exactly the kind of question `per_tab_process_
   isolation`'s own Phase A ("prototype") answered before anything else was
   built on top of it, and it should be answered the same way here rather than
   assumed.
2. Given the cost profile (§4a), per-**tab** GPU-device isolation may be
   unaffordable at any default-on setting even if technically viable — worth
   deciding, with real numbers in hand, whether the honest target is "one GPU
   child per **window**" (cheaper, matches the existing render-thread
   granularity, contains crash #3 across windows but not across sibling tabs
   in the same window) versus "one per **tab**" (matches the literal request,
   markedly more expensive). This is a tradeoff the user should make
   explicitly once real numbers exist, not one this document should presume.
3. In parallel, and regardless of which granularity is chosen or how long (a)
   takes: add option (b) as a **diagnostics-only** VEH (log full context, then
   let the process die) so that if crash #3 recurs before isolation ships,
   there's a proper crash report instead of a bare Windows Error Reporting
   entry — this is small, low-risk, and valuable independent of the larger
   decision.

## 5. The epitaph screen — concrete design sketch

Scoped to: once a tab's (or window's, depending on the granularity landed on
in §4) GPU host process is confirmed dead (via the same "child process
terminated" signal the existing Job Object / `--supervise-pid` supervision
already produces for pty-hosting children — §1.1 — not via any in-process SEH
interception), replace that tab's content area with a static, non-GPU
epitaph.

- **Rendering path:** must not depend on the GPU pipeline that just crashed —
  in a per-tab-GPU-process world this is automatic (the epitaph is drawn by
  the parent GUI process, which never touched the crashed child's device), but
  it should also not depend on the parent's own `ProcessGpuContext` being
  healthy, since a resize-triggered SEH inside a shared device (today's
  architecture, before any isolation ships) could plausibly have degraded
  process-wide GPU state even if the process itself survived via some other
  mechanism. A GDI-based (or Direct2D/plain raster-to-HBITMAP) text blit,
  drawn directly onto the tab's screen rectangle via ordinary Win32 painting,
  sidesteps `wgpu`/DXGI entirely and is the same category of fallback this
  codebase already leans on elsewhere (the "GDI placeholder" mentioned in
  `ec757f2ad`'s commit message, cleared only after the render thread's first
  real present — there is already a precedent GDI fallback path to study and
  possibly extend, not invent from nothing).
- **Content:** exception code, faulting module (if resolvable — DXGI/D3D12
  module name is already known from crash #3's own analysis), a short
  human-readable reason, and — if available — a lightweight backtrace of the
  render thread at time of death, since this process already has working
  `cdb.exe`-based crash-dump reading tooling built out this session (tasks
  #626, #627, #637); consider whether a minidump-on-this-specific-death could
  feed the epitaph's own displayed text automatically, rather than needing a
  human to run `cdb.exe` after the fact.
- **Selection/copy without live-terminal machinery:** the epitaph text is
  static and known at render time (not a live PTY stream), so it does not need
  the terminal's screen-buffer-backed selection model at all — a plain
  fixed-string text control (an actual Win32 edit control in read-only mode,
  or a GDI-drawn text region with a hand-rolled click-drag-to-highlight +
  `Ctrl+C`-to-clipboard handler) is sufficient and far simpler than adapting
  `term`'s selection engine, which assumes a live, healthy `Pane`/`Terminal`.
  An `EDIT` control with `ES_READONLY | ES_MULTILINE` is the pragmatic choice
  — it gets standard Windows text selection, Ctrl+C, and even Ctrl+A "for
  free" from `user32`, at the cost of not matching the terminal's own font
  rendering exactly (acceptable for a crash screen, arguably desirable so it's
  visually distinct from live content).
- **"Close this tab" affordance:** must not touch any GPU state that's
  potentially still bad. Closing one dead tab should reduce to: (1) if the
  tab's rendering was isolated in its own child process (§4a), the child is
  already dead — closing just needs to release the parent-side supervision
  handle/`OwnedHandle` and any shared-texture resources for that tab, tearing
  down bookkeeping that's process-owned, not shared-device-owned; (2) if
  isolation is *not* yet in place (interim state, or if the epitaph is only
  reached via a same-process death like a caught Rust panic), closing the tab
  is the same `Mux`-level pane/tab removal path that already exists
  (`ClientDomain::perform_detach`, `mux.remove_domain`,
  `mux.domain_was_detached` — §"Cleanup домена" in
  `docs/per-tab-hosting-architecture.md`) and does not need to touch
  `ProcessGpuContext` or any *other* tab's `RenderState` at all, since those
  are already scoped independently per §1.2. The risky case is exactly the one
  §4's granularity question is about: if the crash was in the **shared**
  process-wide `ProcessGpuContext` (today's architecture) rather than an
  isolated per-tab child, then "close this tab" cannot actually undo whatever
  state the SEH left the shared `Device` in — the epitaph would need to cover
  the *whole window* (or process) in that case, not just one tab, which is
  precisely why §4 recommends deciding the isolation granularity question
  before designing this affordance in full generality.

## 6. Phased plan (if the recommendation is accepted)

Mirroring this project's own established discipline for exactly this class of
decision — `per_tab_process_isolation` went prototype (Phase A) → build out
(Phases B/C) → lifecycle hardening (Phase D) → security review (Phase E) →
cost measurement + architecture doc (Phase F), each phase a separate commit,
each with its own verification, none skipped:

1. **Phase 0 (low-risk, do first, independent of everything else):**
   diagnostics-only VEH at the render thread's DXGI/D3D12 call sites — log
   exception code/address/module and any recoverable context, then let the
   process die (do not attempt `CONTINUE_EXECUTION`). Cheap, safe, immediately
   useful for the *next* occurrence of crash #3's failure class, and doesn't
   presuppose any answer to the bigger isolation question.
2. **Phase A (prototype):** throwaway `CreateSharedHandle` cross-process
   compositing spike — one child process rendering a quad, shared into a
   parent swapchain. Answers: does this actually work reliably on this
   machine's driver stack, what's the per-instance `wgpu::Device` creation
   cost in a child process, what's the texture-sharing latency/overhead. This
   is the step that should have existed before task #640's prompt asserted
   viability as settled fact — right now it isn't, and shouldn't be treated as
   though it were.
3. **Phase B (measure):** with real Phase A numbers, decide per-window vs.
   per-tab granularity (§4a) — this is a decision with a real cost tradeoff
   that needs the user's explicit sign-off (§7), not something to default into.
4. **Phase C onward:** build-out, lifecycle (reuse Job
   Object/`--supervise-pid` supervision pattern), security review (new IPC
   surface = new attack surface, same as Phase E did for the pty transport),
   epitaph screen (§5), staged rollout behind its own default-off config flag
   (matching `per_tab_process_isolation`'s own precedent of shipping
   default-off until proven).

## 7. Open questions requiring the user's explicit decision

1. **Granularity vs. cost.** Per-tab GPU isolation (matches the literal
   request) is markedly more expensive than per-window (cheaper, but does not
   make sibling tabs in the same window survive each other's GPU crashes).
   Is per-window an acceptable compromise pending real prototype numbers, or
   is per-tab a hard requirement regardless of cost?
2. **Scope of "first low-risk step."** Is Phase 0 (diagnostics-only VEH,
   §6.1) worth doing immediately on its own, independent of any decision about
   the larger isolation work, or should it wait until the isolation direction
   is decided so it can be designed together with whatever crash-reporting the
   final epitaph screen needs?
3. **Acceptable startup/memory cost per tab or per window**, once Phase A
   produces real numbers — no such budget currently exists to check the
   prototype's results against.
4. **Timeline / priority relative to other open work** — `#634` (steer
   rendering off the failing Intel driver via `webgpu_power_preference`) is
   still pending and is a much cheaper, narrower mitigation for the *specific*
   machine that hit these three crashes; worth deciding whether that ships
   first as a stopgap while the larger isolation design in this document is
   evaluated, or whether they proceed independently.
5. **Whether the `CreateSharedHandle` viability claim from earlier
   conversation context should be re-derived from scratch or whether that
   earlier research (if it exists) can be located/recovered** — this
   investigation could not find it committed anywhere in the repository.

---

## Sources (external research, §3)

- [Chromium: Multi-process Architecture](https://www.chromium.org/developers/design-documents/multi-process-architecture/) — process isolation rationale, "a crash in one application generally does not impair other applications."
- [Chromium: GPU Accelerated Compositing in Chrome](https://www.chromium.org/developers/design-documents/gpu-accelerated-compositing-in-chrome/) — fetched directly; confirms GPU process is a genuine separate OS process (client/server over a command-buffer IPC channel), and that GPU-process crash recovery is browser-process-level relaunch of a dead child, not in-process exception interception.
- [Vectored Exception Handling — Win32 apps, Microsoft Learn](https://learn.microsoft.com/en-us/windows/win32/debug/vectored-exception-handling) — `AddVectoredExceptionHandler`/`EXCEPTION_CONTINUE_EXECUTION` semantics.
- The Old New Thing (Raymond Chen, Microsoft), various posts on SEH resumability and cross-frame exception safety — searched and summarized; consistent position that generic Win32/driver code is not safely resumable after an arbitrary exception, and that attempting to "handle" such an exception typically defers rather than prevents the underlying corruption's consequences.
