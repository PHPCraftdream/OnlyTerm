# GPU rendering tests

CPU-side buffer-planning tests run without a graphics adapter. The real-buffer
reuse and mirrored-atlas routing tests request an adapter and run whenever one
is available. Without an adapter they emit an explicit `SKIP` diagnostic;
device creation and validation failures still fail the tests.

For a GPU-equipped validation job, require these tests instead of permitting
the headless path:

```powershell
$env:ONLYTERM_REQUIRE_GPU_TESTS = '1'
cargo test -p onlyterm-gpu-render
```

Unit tests exercise both the optional and mandatory missing-adapter paths with
an instance that has no graphics backends enabled. Windows CI already invokes
the test suite through `cargo nextest run --all --no-fail-fast`.
