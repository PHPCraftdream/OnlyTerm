# GUI pipeline profiling on Windows

The opt-in profiler separates PTY parsing, terminal-lock waits/holds, render
snapshot, shaping/paint, frame construction/IPC, GPU-worker CPU submission and
`ResizePseudoConsole`. It does not measure physical keyboard-to-displayed-pixel
latency or GPU execution time.

## Run

Build serially, without the inherited shared target directory or a compiler
wrapper left in the environment:

```sh
env -u CARGO_TARGET_DIR -u RUSTC_WRAPPER -u CARGO_BUILD_RUSTC_WRAPPER cargo build --profile dev-install -p onlyterm -p onlyterm-gui -j 2
```

From the repository root:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File ci/profile-gui/run.ps1 `
  -GuiExe target/dev-install/onlyterm-gui.exe `
  -CliExe target/dev-install/onlyterm.exe -Scenario multi-pane
```

Scenarios: `ascii`, `cyrillic`, `sgr`, `tui`, `history`, `bulk-printstring`,
`bulk-lines`, `multi-pane`, `resize`, `input-probe`. Use a fresh output directory
for each invocation; the default supplies a timestamped directory. Run scenarios
and builds sequentially when collecting performance results.

`input-probe -ConcurrentBulk` keeps a second application thread writing 64-KiB
SGR text while the main application thread receives 100 F13 key events. All 100
receipts must occur while that writer is alive. The writer is bounded and stopped
before the final model-completion marker. The receiving thread does not print
per-key echoes in bulk mode: doing so would contend on `Console.Out` and measure
application output serialization rather than input delivery alone.

`-DisableInstrumentation` sets the profiling interval to zero. Compare the
resulting `pane-N.txt` files against an instrumented invocation of the same
scenario; this checks terminal output, not equal timing overhead.

## Process isolation and safety

- The harness launches its own GUI with `--skip-config`, a unique window class,
  and a recorded PID. It does not load or change the user's `.onlyterm.ktav`.
- Inherited `ONLYTERM_UNIX_SOCKET` and `ONLYTERM_PANE` are cleared. CLI commands
  are pinned to the socket belonging to the launched GUI PID; auto-start is off.
- Keyboard messages and resize messages target that owned HWND. The HWND's PID
  is checked before key enqueue. No global `SendKeys` or foreground activation
  is used.
- On Windows, native known-folder lookup ignores a changed `USERPROFILE`/`HOME`
  for the runtime directory. Log/socket storage still uses the real known-folder
  location. The harness copies only its GUI and identified GPU-worker PID logs;
  the environment override is not a filesystem sandbox.
- Cleanup closes only the GUI launched by this invocation. A forced fallback is
  restricted to the exact PID captured from its `Process.Start`. No process is
  selected for termination by image name. Never terminate a user's OnlyTerm to
  unlock a build output.

## Artifacts and endpoint definitions

- `timeline.csv`: startup, workload, completion and a two-interval settle phase.
- `metrics-intervals.csv`: cumulative histogram reports per PID and metric.
- `metrics-latest.csv`: last cumulative report for each PID/metric pair.
- `processes.csv`, `owned-logs/`: provenance for GUI, GPU worker and CLI calls.
- `pane-N.txt`: final terminal-model text captured through the CLI.
- `input-receipts.csv`: application-side F13 receipts and shared-QPC timestamps.
  Readiness is a nonce-matched record, not a transient screen line that a bulk
  writer could immediately scroll away. Shared-read access avoids blocking the
  receipt writer; incomplete records are not accepted.
- `input-app-receipt-summary.csv`: owned-HWND key enqueue to the native ConPTY
  application's `Console.ReadKey`, using shared Windows QPC. Its end timestamp
  precedes receipt-file I/O. Observer polling is excluded. This is delivery to
  the application, not a hardware-key timestamp or a presented-pixel timestamp.
- `input-probe-summary.csv`: observer latency. Without bulk it includes receipt
  polling, CLI startup and terminal-model echo polling. With bulk it ends at the
  receipt-file observer; it is explicitly not model/pixel latency.
- `gui.input_to_next_paint`: GUI key-handler start to the next paint proxy. It
  excludes time spent queued before the key handler and does not prove GPU
  presentation. Its opt-in pending queue is bounded at 256 samples.
- `gui.host_process.inflight_duration`: an upper bound ending at a subsequent
  host paint. It can include idle time, event-loop delay and unsuccessful work;
  do not report it as GPU execution or an exact frame-ack round trip.
- `worker_submit`: worker CPU-side submission/present-call wall time, not GPU
  execution time. Worker ack-write/flush is reported separately.

Histograms include startup/warmup and are cumulative, not interval deltas.
Small frame sample counts cannot support a reliable tail estimate. HDR values
are quantized. `estimated_total = samples * mean` is an approximate stage total.
Nested timers overlap: for example, paint includes collection, and collection
includes snapshot work. Do not sum every metric into a frame critical path.

Only metrics with the appropriate semantic unit belong in duration tables.
`.size` metrics are raw counts/bytes. Existing count-like metrics without that
suffix may still be printed with `ns`; queue depth and rows-per-sweep are not
latencies and must not be ranked as duration hotspots.

## Observed output-equivalence check

The native SGR fixture produced the same `pane-0.txt` with profiling enabled and
disabled. Both SHA-256 values were
`bbf6f65f0a3ccdc92e488fdf5e4dcbb8c22cabfbd8439fa8685bdd094dea94e4`.
This establishes the exercised SGR output equivalence only; it does not claim
zero profiling overhead or physical display equivalence.
