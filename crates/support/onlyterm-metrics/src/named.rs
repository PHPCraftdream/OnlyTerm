//! Caching for call sites whose metric name is only known at runtime.
//!
//! [`crate::CachedHandle`] needs one `static` per call site, which works
//! when the metric name is a literal baked into the macro invocation. Some
//! call sites instead receive the name as a `&'static str` function
//! argument, shared by several distinct call sites with different constant
//! names (e.g. `lock_terminal_timed(terminal, "localpane.terminal_lock.wait.key_input", ...)`
//! called from half a dozen places in `onlyterm-mux`). [`NamedCache`] gives
//! that single function body one shared, keyed cache instead.
//!
//! The read path (the common case, once every distinct name has been seen
//! once) only takes a shared read lock plus a hashmap lookup; the exclusive
//! write lock is only taken to insert a name that hasn't been seen before.

use crate::recorder_installed;
use std::collections::HashMap;
use std::sync::{PoisonError, RwLock};

/// A cache of `metrics` handles keyed by a runtime (but `'static`) metric
/// name. See the module docs for when to use this instead of
/// [`crate::CachedHandle`].
pub struct NamedCache<T> {
    map: RwLock<HashMap<&'static str, T>>,
}

impl<T> NamedCache<T> {
    pub fn new() -> Self {
        Self {
            map: RwLock::new(HashMap::new()),
        }
    }
}

impl<T> Default for NamedCache<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Clone> NamedCache<T> {
    /// Returns the handle cached for `name`, resolving (and caching) it via
    /// `resolve` the first time `name` is seen after the recorder is known
    /// to be installed. Subject to the same init-order hazard as
    /// [`crate::CachedHandle`]: see the crate-level docs.
    pub fn get_or_resolve(&self, name: &'static str, resolve: impl FnOnce() -> T) -> T {
        Self::get_or_resolve_with_flag(&self.map, name, recorder_installed(), resolve)
    }

    /// Core logic with an injected "is the recorder installed" flag, for
    /// deterministic testing -- see
    /// [`crate::get_or_resolve_with_flag`] for why this is a parameter
    /// rather than always reading the process-global flag.
    fn get_or_resolve_with_flag(
        map: &RwLock<HashMap<&'static str, T>>,
        name: &'static str,
        installed: bool,
        resolve: impl FnOnce() -> T,
    ) -> T {
        if !installed {
            return resolve();
        }
        // A metrics cache must never take a caller down: a poisoned map
        // still holds only fully inserted handles.
        if let Some(existing) = map.read().unwrap_or_else(PoisonError::into_inner).get(name) {
            return existing.clone();
        }
        let mut guard = map.write().unwrap_or_else(PoisonError::into_inner);
        if let Some(existing) = guard.get(name) {
            return existing.clone();
        }
        let value = resolve();
        guard.insert(name, value.clone());
        value
    }

    #[cfg(test)]
    pub(crate) fn get_or_resolve_for_test(
        &self,
        name: &'static str,
        installed: bool,
        resolve: impl FnOnce() -> T,
    ) -> T {
        Self::get_or_resolve_with_flag(&self.map, name, installed, resolve)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use metrics::{Counter, CounterFn, Key, Metadata, Recorder};
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
    use std::sync::Arc;

    #[derive(Default)]
    struct CountingRecorder {
        registrations_by_call: Arc<AtomicUsize>,
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
            self.registrations_by_call.fetch_add(1, Ordering::Relaxed);
            Counter::from_arc(Arc::new(CountingCounter {
                value: Arc::new(AtomicU64::new(0)),
            }))
        }

        fn register_gauge(&self, _key: &Key, _metadata: &Metadata<'_>) -> metrics::Gauge {
            metrics::Gauge::noop()
        }

        fn register_histogram(&self, _key: &Key, _metadata: &Metadata<'_>) -> metrics::Histogram {
            metrics::Histogram::noop()
        }
    }

    #[test]
    fn distinct_names_get_distinct_handles_registered_once_each() {
        let recorder = CountingRecorder::default();
        let cache: NamedCache<Counter> = NamedCache::new();

        metrics::with_local_recorder(&recorder, || {
            for _ in 0..20 {
                let _ =
                    cache.get_or_resolve_for_test("name.a", true, || metrics::counter!("name.a"));
            }
            for _ in 0..7 {
                let _ =
                    cache.get_or_resolve_for_test("name.b", true, || metrics::counter!("name.b"));
            }
        });

        assert_eq!(recorder.registrations_by_call.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn pre_install_lookups_are_not_cached() {
        let recorder = CountingRecorder::default();
        let cache: NamedCache<Counter> = NamedCache::new();

        metrics::with_local_recorder(&recorder, || {
            for _ in 0..3 {
                let _ = cache
                    .get_or_resolve_for_test("name.pre", false, || metrics::counter!("name.pre"));
            }
            assert_eq!(recorder.registrations_by_call.load(Ordering::Relaxed), 3);

            let _ =
                cache.get_or_resolve_for_test("name.pre", true, || metrics::counter!("name.pre"));
            assert_eq!(recorder.registrations_by_call.load(Ordering::Relaxed), 4);

            for _ in 0..5 {
                let _ = cache
                    .get_or_resolve_for_test("name.pre", true, || metrics::counter!("name.pre"));
            }
            assert_eq!(recorder.registrations_by_call.load(Ordering::Relaxed), 4);
        });
    }

    #[test]
    fn concurrent_registration_from_several_threads_is_consistent() {
        let recorder = CountingRecorder::default();
        let cache: Arc<NamedCache<Counter>> = Arc::new(NamedCache::new());

        // `with_local_recorder` sets a thread-local, which spawned threads
        // do not inherit -- each thread below must install it itself.
        std::thread::scope(|scope| {
            for _ in 0..8 {
                let cache = Arc::clone(&cache);
                let recorder = &recorder;
                scope.spawn(move || {
                    metrics::with_local_recorder(recorder, || {
                        for _ in 0..200 {
                            let _ = cache.get_or_resolve_for_test("name.concurrent", true, || {
                                metrics::counter!("name.concurrent")
                            });
                        }
                    });
                });
            }
        });

        assert_eq!(
            recorder.registrations_by_call.load(Ordering::Relaxed),
            1,
            "concurrent first-touch registration for one name must happen exactly once"
        );
    }

    #[test]
    fn panicking_resolve_does_not_break_later_lookups() {
        let recorder = CountingRecorder::default();
        let cache: NamedCache<Counter> = NamedCache::new();

        // `resolve` runs under the write lock, so a panic there poisons it.
        let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            cache.get_or_resolve_for_test("name.poison", true, || panic!("resolve failed"))
        }));
        assert!(panicked.is_err());
        assert!(cache.map.is_poisoned());

        metrics::with_local_recorder(&recorder, || {
            for _ in 0..3 {
                let _ = cache.get_or_resolve_for_test("name.poison", true, || {
                    metrics::counter!("name.poison")
                });
            }
        });
        assert_eq!(recorder.registrations_by_call.load(Ordering::Relaxed), 1);
    }
}
