//! Placeholder benchmark demonstrating how `bench-scale-tool` is wired up
//! in this crate (task #150).
//!
//! This intentionally benchmarks trivial, side-effect-free work — the goal
//! is a working, compiling example of the harness plumbing, not a real
//! performance measurement. Add real benchmarks alongside this file (or in
//! new files under `benches/`) as separate `[[bench]]` targets, following
//! the same `harness = false` + `bench_scale_tool::Harness` pattern.
//!
//! See `docs/benchmarking.md` for how to add a bench and how calibration
//! works.

use bench_scale_tool::Harness;
use std::hint::black_box;

fn main() {
    let mut h = Harness::new("placeholder", env!("CARGO_MANIFEST_DIR"));

    h.bench("placeholder/noop", || {
        black_box(());
    });

    h.bench("placeholder/add", || {
        let a = black_box(2u64);
        let b = black_box(2u64);
        black_box(a + b);
    });

    h.run();
}
