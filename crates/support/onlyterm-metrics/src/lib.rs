//! Cheap call-site caching for `metrics` handles.
//!
//! `metrics::histogram!`/`metrics::counter!` re-resolve the handle through
//! the installed `Recorder` on *every* invocation: the macro expands to
//! `with_recorder(|r| r.register_histogram(...))` (see `metrics` 0.23's
//! `macros.rs`). OnlyTerm's recorder (`onlyterm-gui`'s `Stats`) takes a lock
//! and does a hashmap lookup per registration, so re-resolving the handle on
//! a per-row/per-frame/per-task hot path is wasted, measurable work. This
//! crate lets a call site cache the resolved handle in a `static` after the
//! first successful resolution, so steady-state calls just clone an `Arc`.
//!
//! # Init-order hazard
//!
//! Before any global recorder is installed, `metrics`'s macros hand back a
//! no-op handle. Caching *that* handle forever would silently and
//! permanently disable the metric for the rest of the process, since
//! `metrics::set_global_recorder` can only succeed once. To avoid that,
//! caching is gated on [`mark_recorder_installed`]: call sites resolve (but
//! do not cache) a fresh handle on every call until the real recorder has
//! been installed, and only start caching from the first call made
//! afterwards.
//!
//! Whatever installs the global recorder (e.g. `onlyterm-gui`'s
//! `Stats::init`) must call [`mark_recorder_installed`] once, right after
//! `metrics::set_global_recorder` succeeds.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

pub mod named;

static RECORDER_INSTALLED: AtomicBool = AtomicBool::new(false);

/// Marks the global `metrics` recorder as installed. Call once, after
/// `metrics::set_global_recorder` succeeds. Idempotent; safe to call more
/// than once (later calls are no-ops).
pub fn mark_recorder_installed() {
    RECORDER_INSTALLED.store(true, Ordering::Release);
}

/// Whether [`mark_recorder_installed`] has been called yet in this process.
pub fn recorder_installed() -> bool {
    RECORDER_INSTALLED.load(Ordering::Acquire)
}

/// A single call site's cached `metrics` handle (typically a
/// `metrics::Histogram` or `metrics::Counter`).
///
/// Construct as a `static` at the call site (see [`cached_histogram!`] /
/// [`cached_counter!`]); [`CachedHandle::get_or_resolve`] handles the
/// init-order hazard described in the module docs.
pub struct CachedHandle<T> {
    cell: OnceLock<T>,
}

impl<T> CachedHandle<T> {
    pub const fn new() -> Self {
        Self {
            cell: OnceLock::new(),
        }
    }
}

impl<T> Default for CachedHandle<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Clone> CachedHandle<T> {
    /// Returns the cached handle, resolving (and caching) it via `resolve`
    /// the first time this is called after the recorder is known to be
    /// installed. Before that point, `resolve` runs -- uncached -- on every
    /// call, so a metric used before startup finishes is never permanently
    /// pinned to a no-op handle.
    pub fn get_or_resolve(&self, resolve: impl FnOnce() -> T) -> T {
        get_or_resolve_with_flag(&self.cell, recorder_installed(), resolve)
    }
}

/// Core caching logic with an injected "is the recorder installed" flag.
///
/// Exposed (rather than folded into [`CachedHandle::get_or_resolve`]) so
/// tests can exercise the init-order hazard deterministically against a
/// locally-owned flag, instead of the process-global one: like the real
/// `metrics` recorder, [`RECORDER_INSTALLED`] only ever flips `false ->
/// true`, once, for the lifetime of the process, so it can't be reset
/// between test cases sharing the same test binary.
pub fn get_or_resolve_with_flag<T: Clone>(
    cell: &OnceLock<T>,
    installed: bool,
    resolve: impl FnOnce() -> T,
) -> T {
    if !installed {
        return resolve();
    }
    cell.get_or_init(resolve).clone()
}

/// Registers (and caches) a `metrics::Histogram` for a compile-time metric
/// name, with an optional compile-time label. Expands to a per-call-site
/// `static` cache cell, so each macro invocation site gets its own handle;
/// a labelled call site with more than one possible label *value* needs one
/// invocation per value (see `crates/graphics/window/src/spawn.rs` for an
/// example with `pri = high|low`). For metric names or labels that are only
/// known at runtime, use [`named::NamedCache`] instead.
#[macro_export]
macro_rules! cached_histogram {
    ($name:expr) => {{
        static CACHE: $crate::CachedHandle<metrics::Histogram> = $crate::CachedHandle::new();
        CACHE.get_or_resolve(|| metrics::histogram!($name))
    }};
    ($name:expr, $label_key:expr => $label_value:expr) => {{
        static CACHE: $crate::CachedHandle<metrics::Histogram> = $crate::CachedHandle::new();
        CACHE.get_or_resolve(|| metrics::histogram!($name, $label_key => $label_value))
    }};
}

/// Registers (and caches) a `metrics::Counter` for a compile-time metric
/// name. See [`cached_histogram!`] for details shared with the histogram
/// variant.
#[macro_export]
macro_rules! cached_counter {
    ($name:expr) => {{
        static CACHE: $crate::CachedHandle<metrics::Counter> = $crate::CachedHandle::new();
        CACHE.get_or_resolve(|| metrics::counter!($name))
    }};
}

#[cfg(test)]
mod tests {
    use super::*;
    use metrics::{Counter, CounterFn, Histogram, HistogramFn, Key, Metadata, Recorder};
    use std::sync::atomic::{AtomicU64, AtomicUsize};
    use std::sync::Arc;

    /// A local recorder that counts registrations per key and records every
    /// value handed to a histogram/counter, so tests can assert both "did we
    /// register exactly once" and "did every value still arrive".
    #[derive(Default)]
    struct CountingRecorder {
        histogram_registrations: Arc<AtomicUsize>,
        counter_registrations: Arc<AtomicUsize>,
        histogram_values: Arc<AtomicUsize>,
    }

    struct CountingHistogram {
        values: Arc<AtomicUsize>,
    }

    impl HistogramFn for CountingHistogram {
        fn record(&self, _value: f64) {
            self.values.fetch_add(1, Ordering::Relaxed);
        }
    }

    struct CountingCounter {
        value: Arc<AtomicU64>,
    }

    impl CounterFn for CountingCounter {
        fn increment(&self, value: u64) {
            self.value.fetch_add(value, Ordering::Relaxed);
        }
        fn absolute(&self, value: u64) {
            self.value.store(value, Ordering::Relaxed);
        }
    }

    impl Recorder for CountingRecorder {
        fn describe_counter(
            &self,
            _: metrics::KeyName,
            _: Option<metrics::Unit>,
            _: metrics::SharedString,
        ) {
        }
        fn describe_gauge(
            &self,
            _: metrics::KeyName,
            _: Option<metrics::Unit>,
            _: metrics::SharedString,
        ) {
        }
        fn describe_histogram(
            &self,
            _: metrics::KeyName,
            _: Option<metrics::Unit>,
            _: metrics::SharedString,
        ) {
        }

        fn register_counter(&self, _key: &Key, _metadata: &Metadata<'_>) -> Counter {
            self.counter_registrations.fetch_add(1, Ordering::Relaxed);
            Counter::from_arc(Arc::new(CountingCounter {
                value: Arc::new(AtomicU64::new(0)),
            }))
        }

        fn register_gauge(&self, _key: &Key, _metadata: &Metadata<'_>) -> metrics::Gauge {
            metrics::Gauge::noop()
        }

        fn register_histogram(&self, _key: &Key, _metadata: &Metadata<'_>) -> Histogram {
            self.histogram_registrations.fetch_add(1, Ordering::Relaxed);
            Histogram::from_arc(Arc::new(CountingHistogram {
                values: Arc::clone(&self.histogram_values),
            }))
        }
    }

    // These tests drive `get_or_resolve_with_flag` directly with a
    // function-local `OnceLock` and a function-local `installed` bool,
    // rather than the process-global `CachedHandle`/`RECORDER_INSTALLED`.
    // Both are shared, monotonic (`false -> true`, once) process state, so
    // sharing them across `#[test]` fns that run concurrently in the same
    // binary would make outcomes depend on test execution order. Exercising
    // the same logic through the flag parameter instead keeps every test
    // below fully isolated and deterministic under the parallel runner.

    #[test]
    fn caches_after_n_calls_and_every_value_arrives() {
        let recorder = CountingRecorder::default();
        let cell: OnceLock<Histogram> = OnceLock::new();

        const N: usize = 50;
        for i in 0..N {
            let histogram = metrics::with_local_recorder(&recorder, || {
                get_or_resolve_with_flag(&cell, true, || metrics::histogram!("test.hist"))
            });
            histogram.record(i as f64);
        }

        assert_eq!(
            recorder.histogram_registrations.load(Ordering::Relaxed),
            1,
            "expected exactly one registration across N cached calls"
        );
        assert_eq!(
            recorder.histogram_values.load(Ordering::Relaxed),
            N,
            "every recorded value must still reach the underlying histogram"
        );
    }

    #[test]
    fn labelled_variants_resolve_to_distinct_handles() {
        let recorder = CountingRecorder::default();
        let high_cell: OnceLock<Histogram> = OnceLock::new();
        let low_cell: OnceLock<Histogram> = OnceLock::new();

        metrics::with_local_recorder(&recorder, || {
            for i in 0..10 {
                let histogram = get_or_resolve_with_flag(
                    &high_cell,
                    true,
                    || metrics::histogram!("test.labelled", "pri" => "high"),
                );
                histogram.record(i as f64);
            }
            for i in 0..5 {
                let histogram = get_or_resolve_with_flag(
                    &low_cell,
                    true,
                    || metrics::histogram!("test.labelled", "pri" => "low"),
                );
                histogram.record(i as f64);
            }
        });

        // One registration per distinct label value, not per call.
        assert_eq!(recorder.histogram_registrations.load(Ordering::Relaxed), 2);
        assert_eq!(recorder.histogram_values.load(Ordering::Relaxed), 15);
    }

    #[test]
    fn pre_install_calls_do_not_permanently_disable_the_metric() {
        let recorder = CountingRecorder::default();
        let cell: OnceLock<Histogram> = OnceLock::new();

        metrics::with_local_recorder(&recorder, || {
            // Simulate calls made before the recorder is known to be
            // installed: nothing gets cached, but the underlying recorder
            // is still (redundantly) asked to register the metric each time.
            for _ in 0..3 {
                let histogram = get_or_resolve_with_flag(&cell, false, || {
                    metrics::histogram!("test.pre_install")
                });
                histogram.record(1.0);
            }
            assert!(
                cell.get().is_none(),
                "pre-install resolutions must not populate the cache"
            );
            assert_eq!(recorder.histogram_registrations.load(Ordering::Relaxed), 3);

            // Now the recorder becomes installed: the very next call must
            // still reach the real recorder (not a stuck no-op) and, from
            // then on, cache it.
            let histogram =
                get_or_resolve_with_flag(&cell, true, || metrics::histogram!("test.pre_install"));
            histogram.record(1.0);
            assert!(
                cell.get().is_some(),
                "post-install call must populate the cache"
            );
            assert_eq!(recorder.histogram_registrations.load(Ordering::Relaxed), 4);

            for _ in 0..10 {
                let histogram = get_or_resolve_with_flag(&cell, true, || {
                    metrics::histogram!("test.pre_install")
                });
                histogram.record(1.0);
            }
            assert_eq!(
                recorder.histogram_registrations.load(Ordering::Relaxed),
                4,
                "post-install calls after the first must be served from cache"
            );
            assert_eq!(recorder.histogram_values.load(Ordering::Relaxed), 14);
        });
    }

    #[test]
    fn recorder_installed_flips_true_and_stays_true() {
        // `RECORDER_INSTALLED` is process-global and monotonic, so this only
        // asserts the post-condition, not a particular before-state (another
        // test in this binary may have already flipped it).
        mark_recorder_installed();
        assert!(recorder_installed());
        mark_recorder_installed();
        assert!(recorder_installed());
    }
}
