use hdrhistogram::Histogram;
use metrics::{Counter, Gauge, Key, KeyName, Metadata, Recorder, SharedString, Unit};
use onlyterm_config::configuration;
use parking_lot::{Mutex, RwLock};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tabout::{tabulate_output, Alignment, Column};

static ENABLE_STAT_PRINT: AtomicBool = AtomicBool::new(true);
lazy_static::lazy_static! {
    // `Inner` has no outer lock: each map below owns its own `RwLock`, so a
    // lookup of an already-registered key (the steady-state case once every
    // metric used by the process has been hit once) only ever takes a
    // shared read lock, and concurrent lookups across the GUI thread, pane
    // parser threads and pty reader threads don't serialize against each
    // other. The exclusive write lock is only taken to insert a key that
    // hasn't been seen before.
    static ref INNER: Arc<Inner> = Arc::new(Inner {
        histograms: RwLock::new(HashMap::new()),
        throughput: RwLock::new(HashMap::new()),
        counters: RwLock::new(HashMap::new()),
    });
}

/// Looks up `key` in `map`'s read lock first; only takes the write lock (and
/// re-checks, in case another thread won the race) to insert a key that
/// wasn't there yet. Keeps the common case -- a metric that's already been
/// registered -- off the exclusive lock.
fn get_or_insert<V: Clone>(
    map: &RwLock<HashMap<Key, V>>,
    key: &Key,
    make: impl FnOnce() -> V,
) -> V {
    if let Some(existing) = map.read().get(key) {
        return existing.clone();
    }
    let mut guard = map.write();
    if let Some(existing) = guard.get(key) {
        return existing.clone();
    }
    let value = make();
    guard.insert(key.clone(), value.clone());
    value
}

struct ThroughputInner {
    hist: Histogram<u64>,
    last: Option<Instant>,
    count: u64,
}

struct Throughput {
    inner: Mutex<ThroughputInner>,
}

impl Throughput {
    fn new() -> Self {
        Self {
            inner: Mutex::new(ThroughputInner {
                hist: Histogram::new(2).expect("failed to create histogram"),
                last: None,
                count: 0,
            }),
        }
    }
    fn current(&self) -> u64 {
        self.inner.lock().current()
    }

    fn percentiles(&self) -> (u64, u64, u64) {
        let inner = self.inner.lock();
        let p50 = inner.hist.value_at_percentile(50.);
        let p75 = inner.hist.value_at_percentile(75.);
        let p95 = inner.hist.value_at_percentile(95.);
        (p50, p75, p95)
    }
}

impl ThroughputInner {
    fn add(&mut self, value: u64) {
        if let Some(ref last) = self.last {
            let elapsed = last.elapsed();
            if elapsed > Duration::from_secs(1) {
                self.hist.record(self.count).ok();
                self.count = 0;
                self.last = Some(Instant::now());
            }
        } else {
            // Start a new window
            self.last = Some(Instant::now());
        };
        self.count += value;
    }

    fn current(&mut self) -> u64 {
        if let Some(ref last) = self.last {
            let elapsed = last.elapsed();
            if elapsed > Duration::from_secs(1) {
                self.hist.record(self.count).ok();
                self.count = 0;
                self.last = Some(Instant::now());
            }
        }
        self.count
    }
}

impl metrics::HistogramFn for Throughput {
    fn record(&self, value: f64) {
        self.inner.lock().add(value as u64);
    }
}

struct ScaledHistogram {
    hist: Mutex<Histogram<u64>>,
    scale: f64,
}

impl ScaledHistogram {
    fn new(scale: f64) -> Arc<Self> {
        Arc::new(Self {
            hist: Mutex::new(Histogram::new(2).expect("failed to create new Histogram")),
            scale,
        })
    }
    fn percentiles(&self) -> (u64, u64, u64) {
        let hist = self.hist.lock();
        let p50 = hist.value_at_percentile(50.);
        let p75 = hist.value_at_percentile(75.);
        let p95 = hist.value_at_percentile(95.);
        (p50, p75, p95)
    }

    fn latency_percentiles(&self) -> (Duration, Duration, Duration) {
        let hist = self.hist.lock();
        let p50 = pctile_latency(&hist, 50.);
        let p75 = pctile_latency(&hist, 75.);
        let p95 = pctile_latency(&hist, 95.);
        (p50, p75, p95)
    }
}

impl metrics::HistogramFn for ScaledHistogram {
    fn record(&self, value: f64) {
        self.hist.lock().record((value * self.scale) as u64).ok();
    }
}

fn pctile_latency(histogram: &Histogram<u64>, p: f64) -> Duration {
    Duration::from_nanos(histogram.value_at_percentile(p))
}

struct MyCounter {
    value: AtomicUsize,
}

impl metrics::CounterFn for MyCounter {
    fn increment(&self, value: u64) {
        self.value.fetch_add(value as usize, Ordering::Relaxed);
    }

    fn absolute(&self, value: u64) {
        self.value.store(value as usize, Ordering::Relaxed);
    }
}

struct Inner {
    histograms: RwLock<HashMap<Key, Arc<ScaledHistogram>>>,
    throughput: RwLock<HashMap<Key, Arc<Throughput>>>,
    counters: RwLock<HashMap<Key, Arc<MyCounter>>>,
}

impl Inner {
    fn run(inner: Arc<Inner>) {
        let mut last_print = Instant::now();

        let rate_cols = vec![
            Column {
                name: "STAT".to_string(),
                alignment: Alignment::Left,
            },
            Column {
                name: "current".to_string(),
                alignment: Alignment::Left,
            },
            Column {
                name: "p50".to_string(),
                alignment: Alignment::Left,
            },
            Column {
                name: "p75".to_string(),
                alignment: Alignment::Left,
            },
            Column {
                name: "p95".to_string(),
                alignment: Alignment::Left,
            },
        ];
        let cols = vec![
            Column {
                name: "STAT".to_string(),
                alignment: Alignment::Left,
            },
            Column {
                name: "p50".to_string(),
                alignment: Alignment::Left,
            },
            Column {
                name: "p75".to_string(),
                alignment: Alignment::Left,
            },
            Column {
                name: "p95".to_string(),
                alignment: Alignment::Left,
            },
        ];
        let count_cols = vec![
            Column {
                name: "STAT".to_string(),
                alignment: Alignment::Left,
            },
            Column {
                name: "COUNT".to_string(),
                alignment: Alignment::Left,
            },
        ];

        loop {
            std::thread::sleep(Duration::from_secs(1));

            if !ENABLE_STAT_PRINT.load(Ordering::Acquire) {
                break;
            }

            let seconds = configuration().periodic_stat_logging;
            if seconds == 0 {
                continue;
            }
            if last_print.elapsed() >= Duration::from_secs(seconds) {
                let mut data = vec![];

                for (key, tput) in inner.throughput.read().iter() {
                    let current = tput.current();
                    let (p50, p75, p95) = tput.percentiles();
                    data.push(vec![
                        key.to_string(),
                        format!("{:.2?}", current),
                        format!("{:.2?}", p50),
                        format!("{:.2?}", p75),
                        format!("{:.2?}", p95),
                    ]);
                }
                data.sort_by(|a, b| a[0].cmp(&b[0]));
                eprintln!();
                tabulate_output(&rate_cols, &data, &mut std::io::stderr().lock()).ok();

                data.clear();
                for (key, histogram) in inner.histograms.read().iter() {
                    if key.name().ends_with(".size") {
                        let (p50, p75, p95) = histogram.percentiles();
                        data.push(vec![
                            key.to_string(),
                            format!("{:.2?}", p50),
                            format!("{:.2?}", p75),
                            format!("{:.2?}", p95),
                        ]);
                    } else {
                        let (p50, p75, p95) = histogram.latency_percentiles();
                        data.push(vec![
                            key.to_string(),
                            format!("{:.2?}", p50),
                            format!("{:.2?}", p75),
                            format!("{:.2?}", p95),
                        ]);
                    }
                }
                data.sort_by(|a, b| a[0].cmp(&b[0]));
                eprintln!();
                tabulate_output(&cols, &data, &mut std::io::stderr().lock()).ok();

                data.clear();
                for (key, count) in inner.counters.read().iter() {
                    data.push(vec![
                        key.to_string(),
                        count.value.load(Ordering::Relaxed).to_string(),
                    ]);
                }
                data.sort_by(|a, b| a[0].cmp(&b[0]));
                eprintln!();
                tabulate_output(&count_cols, &data, &mut std::io::stderr().lock()).ok();

                last_print = Instant::now();
            }
        }
    }
}

pub struct Stats {
    inner: Arc<Inner>,
}

impl Stats {
    pub fn new() -> Self {
        Self {
            inner: Arc::clone(&INNER),
        }
    }

    pub fn init() -> anyhow::Result<()> {
        let stats = Self::new();
        let inner = Arc::clone(&stats.inner);
        std::thread::spawn(move || Inner::run(inner));
        metrics::set_global_recorder(stats)
            .map_err(|e| anyhow::anyhow!("Failed to set metrics recorder:{}", e))?;
        // Call-sites using `onlyterm_metrics::cached_histogram!`/`cached_counter!`
        // gate their caching on this: a handle resolved before the recorder
        // above is installed is a no-op, and caching it forever would
        // silently disable that metric for the rest of the process. See
        // `onlyterm_metrics`'s crate docs for the full hazard.
        onlyterm_metrics::mark_recorder_installed();
        Ok(())
    }
}

impl Recorder for Stats {
    fn describe_counter(&self, _key: KeyName, _unit: Option<Unit>, _description: SharedString) {}

    fn describe_gauge(&self, _key: KeyName, _unit: Option<Unit>, _description: SharedString) {}

    fn describe_histogram(&self, _key: KeyName, _unit: Option<Unit>, _description: SharedString) {}

    fn register_counter(&self, key: &Key, _metadata: &Metadata) -> Counter {
        let counter = get_or_insert(&self.inner.counters, key, || {
            Arc::new(MyCounter {
                value: AtomicUsize::new(0),
            })
        });
        Counter::from_arc(counter)
    }

    fn register_gauge(&self, _key: &Key, _metadata: &Metadata) -> Gauge {
        Gauge::noop()
    }

    fn register_histogram(&self, key: &Key, _metadata: &Metadata) -> metrics::Histogram {
        if key.name().ends_with(".rate") {
            let tput = get_or_insert(&self.inner.throughput, key, || Arc::new(Throughput::new()));
            metrics::Histogram::from_arc(tput)
        } else {
            let scale = if key.name().ends_with(".size") {
                1.0
            } else {
                // Assume seconds; convert to nanoseconds
                1_000_000_000.0
            };
            let histogram =
                get_or_insert(&self.inner.histograms, key, || ScaledHistogram::new(scale));
            metrics::Histogram::from_arc(histogram)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU64;

    // `Stats::new()` clones an `Arc` to the single process-wide `INNER`
    // static, shared with every other test in this binary (and, outside of
    // tests, with the real recorder). Every test below therefore uses a
    // metric name unique to itself, so tests running concurrently can never
    // observe each other's registrations.
    // `label` is appended last (after the uniquifying id), not first, so
    // that a caller relying on a trailing suffix like `.size`/`.rate` (to
    // match `Stats`'s own suffix-based routing) still gets it at the very
    // end of the generated name.
    fn unique_key(label: &str) -> Key {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        Key::from_name(format!("stats::tests::unique_{}{}", id, label))
    }

    fn test_metadata() -> Metadata<'static> {
        Metadata::new("stats::tests", metrics::Level::INFO, None)
    }

    #[test]
    fn registering_same_counter_key_twice_returns_same_underlying_counter() {
        let stats = Stats::new();
        let key = unique_key("_counter");
        let md = test_metadata();

        let first = stats.register_counter(&key, &md);
        first.increment(3);
        let second = stats.register_counter(&key, &md);
        second.increment(4);

        // If the two handles were backed by different `MyCounter`s, this
        // would read back 3 or 4, not the sum of both.
        let stored = stats
            .inner
            .counters
            .read()
            .get(&key)
            .expect("counter must be registered")
            .clone();
        assert_eq!(stored.value.load(Ordering::Relaxed), 7);
    }

    #[test]
    fn registering_same_histogram_key_twice_returns_same_underlying_histogram() {
        let stats = Stats::new();
        let key = unique_key("_histogram.size");
        let md = test_metadata();

        let first = stats.register_histogram(&key, &md);
        first.record(10.0);
        let second = stats.register_histogram(&key, &md);
        second.record(20.0);

        let stored = stats
            .inner
            .histograms
            .read()
            .get(&key)
            .expect("histogram must be registered")
            .clone();
        let (p50, _, _) = stored.percentiles();
        // Both values must have landed in the one shared histogram.
        assert!(p50 == 10 || p50 == 20, "unexpected p50: {}", p50);
        assert_eq!(stored.hist.lock().len(), 2);
    }

    #[test]
    fn rate_suffix_routes_to_throughput_not_histograms() {
        let stats = Stats::new();
        let key = unique_key("_some_stat.rate");
        let md = test_metadata();

        let _ = stats.register_histogram(&key, &md);

        assert!(stats.inner.throughput.read().contains_key(&key));
        assert!(!stats.inner.histograms.read().contains_key(&key));
    }

    #[test]
    fn size_suffix_is_unscaled_other_names_scale_seconds_to_nanoseconds() {
        let stats = Stats::new();
        let md = test_metadata();

        let size_key = unique_key("_some_stat.size");
        let _ = stats.register_histogram(&size_key, &md);
        let size_scale = stats.inner.histograms.read().get(&size_key).unwrap().scale;
        assert_eq!(size_scale, 1.0);

        let latency_key = unique_key("_some_stat.latency");
        let _ = stats.register_histogram(&latency_key, &md);
        let latency_scale = stats
            .inner
            .histograms
            .read()
            .get(&latency_key)
            .unwrap()
            .scale;
        assert_eq!(latency_scale, 1_000_000_000.0);
    }

    #[test]
    fn concurrent_registration_of_the_same_new_key_is_consistent() {
        let stats = Arc::new(Stats::new());
        let key = unique_key("_concurrent_counter");
        let md = test_metadata();

        const THREADS: u64 = 16;
        std::thread::scope(|scope| {
            for _ in 0..THREADS {
                let stats = Arc::clone(&stats);
                let key = key.clone();
                let md = md.clone();
                scope.spawn(move || {
                    stats.register_counter(&key, &md).increment(1);
                });
            }
        });

        // If two threads had each raced past the read-lock miss and built
        // their own `MyCounter` before the write lock serialized them, one
        // of those two objects would end up orphaned (not the one stored in
        // the map), and its increment would never show up here -- so
        // anything less than `THREADS` would indicate a lost update.
        let stored = stats
            .inner
            .counters
            .read()
            .get(&key)
            .expect("counter must be registered")
            .clone();
        assert_eq!(stored.value.load(Ordering::Relaxed) as u64, THREADS);
    }
}
