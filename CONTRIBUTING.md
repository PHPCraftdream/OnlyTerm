# Contributing to OnlyTerm

Thanks for considering donating your time and energy!  I value any contribution,
even if it is just to highlight a typo.

Included here are some guidelines that can help streamline taking in your contribution.
They are just guidelines and not hard-and-fast rules. Things will probably go faster
and smoother if you have the time and energy to read and follow these suggestions.

## Contributing Documentation

There's never enough!  Pretty much anything is fair game to improve here.

### Running the doc build yourself

To check your documentation additions, you can optionally build the docs yourself and see how the changes will look on the webpage. 

To serve them up, and then automatically rebuild and refresh the docs in your browser, run:
```console
$ ci/build-docs.sh serve
```
And then click on the URL that it prints out after it has performed the first build.

Any arguments passed to `build-docs.sh` are passed down to the underlying `mkdocs` utility.

Look at [mkdocs serve](https://www.mkdocs.org/user-guide/cli/#mkdocs-serve) for more information on additional parameters.

### Platform-specific installation instructions?

OnlyTerm is a Windows-only fork. Installation instructions should be tailored to Windows development environments.

## Contributing code

Yes please!

If you are new to the Rust language check out <https://doc.rust-lang.org/rust-by-example/>.

### Building from source

To build OnlyTerm from source, you will need a local Rust toolchain, and a few Windows-specific dependencies.
Follow the [Install from Source](../install/source.md) guide to get started!

Some platforms like Windows have a few specific steps, make sure to check the dedicated sections in the guide.

### Where to find things?

The `crates/term` directory holds the core terminal model code. This is agnostic
of any windowing system. If you want to add support for terminal escape
sequences and that sort of thing, you probably want to be in the `crates/term` directory.
Keep in mind that for maximal compatibility and utility the terminal model aims to
be compatible with the `xterm` behavior.
https://invisible-island.net/xterm/ctlseqs/ctlseqs.html is a useful resource!

The `crates/wezterm-gui` directory holds the code for the GUI renderer for the
terminal model.  If you want to change something about the GUI you want to be
in that directory.

### Iterating

I tend to iterate and sanity check as I develop using `cargo check`; it
will type-check your code without generating code which is much faster
than building everything in release mode:

```console
$ cargo check
```

Likewise, if you want to quickly check that something works, you can run it
in debug mode using:

```console
$ cargo run
```

This will produce a debug-instrumented binary with poor optimization. This will
give you more detail in the backtrace produced if you run `RUST_BACKTRACE=1 cargo run`.

Starting OnlyTerm with `onlyterm-gui start --always-new-process` is useful to ensure Mux logs are not
hidden in a background process started in an earlier test.

Start OnlyTerm with `onlyterm-gui --config-file ./test-conf.rhai ……` to test a custom config file.


### Benchmarking with `bench-scale-tool`

The [`bench-scale-tool`](https://github.com/PHPCraftdream/bench-scale-tool) crate is
wired up as a workspace dependency (`bench-scale-tool` in `[workspace.dependencies]`)
for writing fixed-iteration micro-benchmarks. It calibrates an iteration count once
per benchmark, then re-runs that static count on every subsequent `cargo bench`, so
wall-time becomes a directly comparable speed signal across runs.

All benchmarks in this workspace use this harness — `rangeset` (`crates/rangeset/benches/rangeset.rs`),
`termwiz`'s `cell` bench (`crates/termwiz/benches/cell.rs`), `wezterm-char-props`'s
`wcwidth` bench (`crates/wezterm-char-props/benches/wcwidth.rs`), and the
`mux`/`placeholder` demo (`crates/mux/benches/placeholder.rs`). Criterion is no
longer used anywhere in the project; it has been fully replaced by `bench-scale-tool`.

See `crates/mux/benches/placeholder.rs` and `crates/mux/Cargo.toml` for a minimal,
working example of the plumbing (`[dev-dependencies]` entry + `[[bench]] harness = false`
target + a `main()` built around `bench_scale_tool::Harness`).

To add your own benchmark to a crate:
1. Add `bench-scale-tool.workspace = true` to that crate's `[dev-dependencies]`.
2. Add a `[[bench]] name = "<bin>" harness = false` entry and a matching file under
   `benches/<bin>.rs` that builds a `Harness::new("<bin>", env!("CARGO_MANIFEST_DIR"))`,
   registers workloads with `.bench("group/case", || { black_box(...) })`, and calls
   `.run()`.
3. Calibrate it once: `cargo bench -p <crate> --bench <bin> -- --calibrate <secs>`.
   This writes/updates `bench-iters.txt` at the workspace root — **commit this file**,
   it stores the calibrated iteration counts so results stay comparable over time.
4. Afterwards, plain `cargo bench -p <crate> --bench <bin>` reuses the stored counts,
   e.g. `cargo bench -p rangeset --bench rangeset` re-runs the `contig/*` and
   `sparse/*` cases at their pinned iteration counts.

`bench-run.log`, `bench-history.log`, and `bench-run-baselines.txt` are machine-local
run artifacts the tool generates and are gitignored — do not commit them.

### Collection-capacity telemetry with `captrack`

The [`captrack`](https://github.com/PHPCraftdream/captrack) crate is wired up as a
workspace dependency (`captrack` in `[workspace.dependencies]`, currently `0.1.1`) and
added as a regular (non-dev) dependency of `crates/mux`. It provides `t*!` macros
(`tvec!`, `tfxmap!`, `tbtreemap!`, and friends) that are drop-in replacements for the
usual collection constructors (`Vec::with_capacity`, `HashMap::new`, etc.).

This is integration/setup only (task #151) — **no production call site has been
migrated to a `t*!` macro yet**. A single demo test,
`crates/mux/src/lib.rs::captrack_integration::tvec_demo_resolves_and_behaves_like_vec`,
exists purely to prove the dependency resolves and compiles; it does not touch any
real code path.

- **Default (no `telemetry` feature):** `t*!` macros compile straight down to the bare
  constructor (e.g. `tvec!(label, n)` becomes exactly `Vec::with_capacity(n)`) — zero
  runtime overhead, and the `label` argument is discarded at compile time.
- **With `telemetry` enabled:** the macros return `Tracked*` wrapper types that record
  real capacity/len into a lock-free global registry (`scc::HashMap`) on construction.
  Enable it with `cargo build -p mux --features telemetry` (or
  `cargo build --workspace --features mux/telemetry`).
- **Dumping stats:** once telemetry is enabled and the instrumented code has run, call
  `captrack::dump_capacity_stats("path.json")` to write out what was recorded.

`crates/mux/Cargo.toml` proxies the feature with `telemetry = ["captrack/telemetry"]`.

### Please include tests to cover your changes!

This will help ensure that your contributions keep working as things change.

You can run the existing tests using:

```console
$ cargo test --all
```

There are some helper classes for writing tests for terminal behavior.
Here's [an example of a test to verify that the terminal contents
match expectations](https://github.com/wezterm/wezterm/blob/fd532a8c2fb3b56593597cf8be1775da1feda0a3/term/src/test/mod.rs#L314).

Please also make a point of adding comments to your tests to help
clarify the intent of the test!

### Please also include documentation if you are adding or changing behavior

This helps to keep things well-understood and working in the long term.
Don't worry if you're not a wordsmith or English isn't your first language as
I can help with that. It is more important to capture the intent of the
feature and having this written out in English also helps when it comes
to reviewing the code.

## Submitting a Pull Request

After considering all of the above, and once you've prepared your contribution
and are ready to submit it, you'll need to create a pull request.

If you're new to GitHub Pull Requests, read through
https://help.github.com/articles/creating-a-pull-request/ to understand
how the process works.

### Before you submit your code

Make sure that the tests are working and that the code is correctly
formatted otherwise the continuous integration system will fail your build:

```console
$ rustup component add rustfmt-preview          # you only need to do this once
$ cargo test --all
$ cargo fmt --all
```

