//! Cache correctness and benchmark cases, extracted without changing behavior.
use super::*;
use config::ConfigHandle;
use lfucache::LfuCacheU64;
use lru::LruCache;
use std::mem::size_of;
use std::sync::Arc;
use std::time::Duration;

// Test helpers for LfuCache with simple capacity
fn test_cache_capacity(_config: &ConfigHandle) -> usize {
    1024
}

// Static capacity for use with fn pointer
static BENCH_CAPACITY: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(1000);

fn bench_capacity_func(_config: &ConfigHandle) -> usize {
    BENCH_CAPACITY.load(std::sync::atomic::Ordering::Relaxed)
}

/// Value type matching CachedLineState shape
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct TestCachedLineState {
    id: u64,
    seqno: SequenceNo,
    shape_hash: [u8; 16],
}

/// Create a test value matching CachedLineState structure
fn make_test_value(id: u64) -> Arc<TestCachedLineState> {
    Arc::new(TestCachedLineState {
        id,
        seqno: SequenceNo::from(id as usize),
        shape_hash: [id as u8; 16],
    })
}

/// Simulate miss-heavy burst: mostly new keys, few repeats.
/// During flood output, renderer sees mostly new line states
/// with very few cache hits.
fn miss_heavy_sequence(count: usize, capacity: usize) -> Vec<u64> {
    // Generate mostly unique keys with ~5% repeats to simulate rare hits
    let mut keys = Vec::with_capacity(count);
    for i in 0..count {
        if i % 20 == 0 && i > capacity {
            // Repeat a recent key ~5% of the time
            keys.push((i - capacity / 2) as u64);
        } else {
            keys.push(i as u64);
        }
    }
    keys
}

/// Simulate stable screen: small working set accessed repeatedly.
/// During static screen redraws, renderer repeatedly hits the same
/// small set of line states.
fn stable_screen_sequence(count: usize, working_set_size: usize) -> Vec<u64> {
    let mut keys = Vec::with_capacity(count);
    for i in 0..count {
        // Cycle through a small working set (e.g., 24 lines for 80x24 screen)
        keys.push((i % working_set_size) as u64);
    }
    keys
}

/// Measure LfuCacheU64 performance
fn benchmark_lfu(keys: &[u64], capacity: usize) -> (Duration, usize, usize, usize) {
    benchmarking::warm_up();

    // Set static capacity for fn pointer
    BENCH_CAPACITY.store(capacity, std::sync::atomic::Ordering::Relaxed);
    let config = ConfigHandle::default_config();
    let keys = keys.to_vec();
    let keys_for_bench = keys.clone();
    let bench_result = benchmarking::measure_function(move |measurer| {
        measurer.measure(|| {
            let config = ConfigHandle::default_config();
            let mut cache =
                LfuCacheU64::new("bench_hit", "bench_miss", bench_capacity_func, &config);
            let mut hits = 0;
            let mut misses = 0;

            for key in &keys_for_bench {
                if cache.get(key).is_some() {
                    hits += 1;
                } else {
                    misses += 1;
                    cache.put(*key, make_test_value(*key));
                }
            }

            std::hint::black_box(&mut cache);
            std::hint::black_box(hits);
            std::hint::black_box(misses);
        })
    })
    .unwrap();

    let mut cache = LfuCacheU64::new("bench_hit", "bench_miss", bench_capacity_func, &config);
    let mut hits = 0;
    let mut misses = 0;
    for key in &keys {
        if cache.get(key).is_some() {
            hits += 1;
        } else {
            misses += 1;
            cache.put(*key, make_test_value(*key));
        }
    }
    let final_len = cache.len();

    (bench_result.elapsed(), hits, misses, final_len)
}

/// Measure LruCache performance
fn benchmark_lru(keys: &[u64], capacity: usize) -> (Duration, usize, usize, usize) {
    benchmarking::warm_up();
    let capacity_nonzero = std::num::NonZeroUsize::new(capacity).unwrap();

    let keys = keys.to_vec();
    let keys_for_bench = keys.clone();
    let bench_result = benchmarking::measure_function(move |measurer| {
        measurer.measure(|| {
            let mut cache = LruCache::new(capacity_nonzero);
            let mut hits = 0;
            let mut misses = 0;

            for key in &keys_for_bench {
                if cache.get(key).is_some() {
                    hits += 1;
                } else {
                    misses += 1;
                    cache.put(*key, make_test_value(*key));
                }
            }

            std::hint::black_box(&mut cache);
            std::hint::black_box(hits);
            std::hint::black_box(misses);
        })
    })
    .unwrap();

    let mut cache = LruCache::new(capacity_nonzero);
    let mut hits = 0;
    let mut misses = 0;
    for key in &keys {
        if cache.get(key).is_some() {
            hits += 1;
        } else {
            misses += 1;
            cache.put(*key, make_test_value(*key));
        }
    }
    let final_len = cache.len();

    (bench_result.elapsed(), hits, misses, final_len)
}

#[test]
#[ignore = "benchmark, not a correctness test -- takes ~16s; run explicitly with \
            `cargo test -p onlyterm-gui --lib -- --ignored bench_cache_comparison`"]
fn bench_cache_comparison() {
    println!("\n=== LfuCacheU64 vs lru::LruCache Benchmark ===");
    println!(
        "Value size: {} bytes (Arc<TestCachedLineState>)",
        size_of::<Arc<TestCachedLineState>>()
    );

    // Size of internal node structures
    println!("\nApproximate entry sizes:");
    println!("  LfuCache Entry: estimated ~{}+ bytes (Rc<Entry<K,V>> with 3 links, 2 RefCells, key, value)",
        size_of::<Rc<()>>() + size_of::<Arc<TestCachedLineState>>() + 32 // approx for links+RefCells
    );
    println!(
        "  LruCache Node: estimated ~{}+ bytes (Key + Value + Node links)",
        size_of::<u64>() + size_of::<Arc<TestCachedLineState>>() + 32 // approx for Node overhead
    );

    // Test parameters
    let capacity = 1000; // Typical cache size
    let operations = 10000;

    // Pattern (a): Miss-heavy burst (flood output)
    println!("\n--- Pattern (a): Miss-heavy burst (flood output) ---");
    println!("Operations: {}, Cache capacity: {}", operations, capacity);
    let miss_heavy_keys = miss_heavy_sequence(operations, capacity);

    let (lfu_time, lfu_hits, lfu_misses, lfu_final) = benchmark_lfu(&miss_heavy_keys, capacity);
    let (lru_time, lru_hits, lru_misses, lru_final) = benchmark_lru(&miss_heavy_keys, capacity);

    let lfu_hit_ratio = (lfu_hits as f64) / (lfu_hits + lfu_misses) as f64;
    let lru_hit_ratio = (lru_hits as f64) / (lru_hits + lru_misses) as f64;

    println!("\nLfuCacheU64:");
    println!("  Time: {:?}", lfu_time);
    println!(
        "  Hits: {}, Misses: {}, Hit ratio: {:.2}%",
        lfu_hits,
        lfu_misses,
        lfu_hit_ratio * 100.0
    );
    println!("  Final cache size: {}", lfu_final);

    println!("\nlru::LruCache:");
    println!("  Time: {:?}", lru_time);
    println!(
        "  Hits: {}, Misses: {}, Hit ratio: {:.2}%",
        lru_hits,
        lru_misses,
        lru_hit_ratio * 100.0
    );
    println!("  Final cache size: {}", lru_final);

    // Show speedup/slowdown
    let lfu_ns = lfu_time.as_nanos();
    let lru_ns = lru_time.as_nanos();
    if lfu_ns > 0 {
        let ratio = (lru_ns as f64 / lfu_ns as f64) * 100.0;
        if ratio < 100.0 {
            println!("  → LruCache is {:.1}% faster", 100.0 - ratio);
        } else {
            println!("  → LruCache is {:.1}% slower", ratio - 100.0);
        }
    }

    // Pattern (b): Stable screen (small working set)
    println!("\n--- Pattern (b): Stable screen (small working set) ---");
    let working_set_size = 24; // Typical terminal height
    println!(
        "Operations: {}, Cache capacity: {}, Working set: {}",
        operations, capacity, working_set_size
    );
    let stable_keys = stable_screen_sequence(operations, working_set_size);

    let (lfu_time_stable, lfu_hits_stable, lfu_misses_stable, lfu_final_stable) =
        benchmark_lfu(&stable_keys, capacity);
    let (lru_time_stable, lru_hits_stable, lru_misses_stable, lru_final_stable) =
        benchmark_lru(&stable_keys, capacity);

    let lfu_hit_ratio_stable =
        (lfu_hits_stable as f64) / (lfu_hits_stable + lfu_misses_stable) as f64;
    let lru_hit_ratio_stable =
        (lru_hits_stable as f64) / (lru_hits_stable + lru_misses_stable) as f64;

    println!("\nLfuCacheU64:");
    println!("  Time: {:?}", lfu_time_stable);
    println!(
        "  Hits: {}, Misses: {}, Hit ratio: {:.2}%",
        lfu_hits_stable,
        lfu_misses_stable,
        lfu_hit_ratio_stable * 100.0
    );
    println!("  Final cache size: {}", lfu_final_stable);

    println!("\nlru::LruCache:");
    println!("  Time: {:?}", lru_time_stable);
    println!(
        "  Hits: {}, Misses: {}, Hit ratio: {:.2}%",
        lru_hits_stable,
        lru_misses_stable,
        lru_hit_ratio_stable * 100.0
    );
    println!("  Final cache size: {}", lru_final_stable);

    // Show speedup/slowdown
    let lfu_ns_stable = lfu_time_stable.as_nanos();
    let lru_ns_stable = lru_time_stable.as_nanos();
    if lfu_ns_stable > 0 {
        let ratio = (lru_ns_stable as f64 / lfu_ns_stable as f64) * 100.0;
        if ratio < 100.0 {
            println!("  → LruCache is {:.1}% faster", 100.0 - ratio);
        } else {
            println!("  → LruCache is {:.1}% slower", ratio - 100.0);
        }
    }

    // Summary and recommendation
    println!("\n=== Summary ===");
    println!("Miss-heavy pattern (flood):");
    println!(
        "  LfuCache: {:.2}%, LruCache: {:.2}%, Time: LruCache {:.1}% {}",
        lfu_hit_ratio * 100.0,
        lru_hit_ratio * 100.0,
        if lfu_ns > 0 {
            (lru_ns as f64 / lfu_ns as f64 * 100.0 - 100.0).abs()
        } else {
            0.0
        },
        if lru_ns < lfu_ns { "faster" } else { "slower" }
    );
    println!("Stable screen pattern:");
    println!(
        "  LfuCache: {:.2}%, LruCache: {:.2}%, Time: LruCache {:.1}% {}",
        lfu_hit_ratio_stable * 100.0,
        lru_hit_ratio_stable * 100.0,
        if lfu_ns_stable > 0 {
            (lru_ns_stable as f64 / lfu_ns_stable as f64 * 100.0 - 100.0).abs()
        } else {
            0.0
        },
        if lru_ns_stable < lfu_ns_stable {
            "faster"
        } else {
            "slower"
        }
    );
}

/// Task #439: Test that empirically demonstrates the clone-broken cache issue.
/// This test shows that the current Line::appdata-based cache never hits
/// because Line::clone copies the Weak reference, and set_appdata on the
/// clone doesn't propagate back to the original Line in the Screen.
#[test]
fn test_clone_broken_cache() {
    use onlyterm_surface::Line;
    use std::sync::Arc;

    // Create a line with some test content
    let original = Line::with_width_and_cell(80, onlyterm_term::Cell::default(), 1usize);

    // First call: compute hash, store in cache via appdata
    let state = Arc::new(CachedLineState {
        id: 42,
        seqno: 1,
        shape_hash: [1u8; 16],
    });
    original.set_appdata(Arc::clone(&state));

    // Verify it worked: original has the appdata
    let original_appdata = original.get_appdata();
    assert!(original_appdata.is_some(), "original should have appdata");
    if let Some(arc) = original_appdata {
        if let Some(line_state) = arc.downcast_ref::<CachedLineState>() {
            assert_eq!(line_state.id, 42, "original appdata should have id 42");
        } else {
            panic!("original appdata should be CachedLineState");
        }
    }

    // Simulate what get_lines() does: clone the line
    let clone1 = original.clone();

    // Clone initially has the same Weak reference, so it can upgrade
    let clone1_appdata = clone1.get_appdata();
    assert!(
        clone1_appdata.is_some(),
        "clone should be able to upgrade Weak initially"
    );
    if let Some(arc) = clone1_appdata {
        if let Some(line_state) = arc.downcast_ref::<CachedLineState>() {
            assert_eq!(line_state.id, 42, "clone should see original's appdata");
        } else {
            panic!("clone appdata should be CachedLineState");
        }
    }

    // This is what the render path does on cache miss: set appdata on the CLONE
    let new_state = Arc::new(CachedLineState {
        id: 43,
        seqno: 1,
        shape_hash: [2u8; 16],
    });
    clone1.set_appdata(Arc::clone(&new_state));

    // The clone now has the new appdata
    let clone1_appdata = clone1.get_appdata();
    assert!(clone1_appdata.is_some());
    if let Some(arc) = clone1_appdata {
        if let Some(line_state) = arc.downcast_ref::<CachedLineState>() {
            assert_eq!(line_state.id, 43, "clone should have new appdata");
        } else {
            panic!("clone appdata should be CachedLineState");
        }
    }

    // KEY BUG: The ORIGINAL Line still has the OLD appdata, not the new one
    let original_appdata_after = original.get_appdata();
    assert!(
        original_appdata_after.is_some(),
        "original should still have some appdata"
    );
    if let Some(arc) = original_appdata_after {
        if let Some(line_state) = arc.downcast_ref::<CachedLineState>() {
            assert_eq!(
                line_state.id, 42,
                "original should still have old appdata (BUG!)"
            );
        } else {
            panic!("original appdata should be CachedLineState");
        }
    }

    // Simulate the next frame: clone again
    let clone2 = original.clone();

    // This clone can still upgrade the ORIGINAL's Weak reference (id: 42)
    // NOT the clone's updated reference (id: 43) - it's completely lost
    let clone2_appdata = clone2.get_appdata();
    assert!(
        clone2_appdata.is_some(),
        "clone2 can upgrade original's Weak"
    );
    if let Some(arc) = clone2_appdata {
        if let Some(line_state) = arc.downcast_ref::<CachedLineState>() {
            assert_eq!(
                line_state.id, 42,
                "clone2 sees original's old appdata, not clone1's new appdata (BUG!)"
            );
        } else {
            panic!("clone2 appdata should be CachedLineState");
        }
    }

    // If we had a seqno bump on the original, the cache entry would be invalid anyway,
    // but for static screens (no seqno bumps), the cache SHOULD hit but DOESN'T.
    // The effective hit rate is 0% for any line that doesn't get modified between frames.

    println!("✓ Test confirmed: set_appdata on Line clones doesn't propagate back to original");
    println!("  - Original appdata id: 42 (unchanged)");
    println!("  - Clone1 appdata id: 43 (updated on clone, lost)");
    println!("  - Clone2 appdata id: 42 (saw original, not clone1's update)");
    println!("  → This is why shape_hash_for_line cache never hits on static screens");
}

/// Task #439: Regression test that the extracted shape_hash_lookup function
/// actually skips recompute on cache hits (proves production code works).
#[test]
fn test_shape_hash_lookup_skips_recompute_on_hit() {
    use lfucache::LfuCache;
    use std::cell::Cell;

    let _capacity = std::num::NonZeroUsize::new(100).unwrap();
    let mut cache = LfuCache::new(
        "test_hit",
        "test_miss",
        test_cache_capacity,
        &ConfigHandle::default_config(),
    );

    let pane_id: PaneId = 1;
    let stable_row: StableRowIndex = 0;
    let key = ShapeHashCacheKey {
        pane_id,
        stable_row,
    };
    let seqno: SequenceNo = 1;

    let compute_count = Cell::new(0);
    let expected_hash = [42u8; 16];

    // First call: miss, should compute
    let hash1 = shape_hash_lookup(&mut cache, key, seqno, || {
        compute_count.set(compute_count.get() + 1);
        expected_hash
    });

    assert_eq!(compute_count.get(), 1, "should compute on first miss");
    assert_eq!(hash1, expected_hash, "should return computed hash");

    // Second call: hit, should NOT recompute
    let hash2 = shape_hash_lookup(&mut cache, key, seqno, || {
        compute_count.set(compute_count.get() + 1);
        expected_hash
    });

    assert_eq!(compute_count.get(), 1, "should NOT recompute on cache hit");
    assert_eq!(hash2, expected_hash, "should return cached hash on hit");
}

/// Task #439: Regression test that seqno mismatch forces recompute even with cache hit.
#[test]
fn test_shape_hash_lookup_recomputes_on_seqno_mismatch() {
    use lfucache::LfuCache;
    use std::cell::Cell;

    let _capacity = std::num::NonZeroUsize::new(100).unwrap();
    let mut cache = LfuCache::new(
        "test_hit",
        "test_miss",
        test_cache_capacity,
        &ConfigHandle::default_config(),
    );

    let pane_id: PaneId = 1;
    let stable_row: StableRowIndex = 0;
    let key = ShapeHashCacheKey {
        pane_id,
        stable_row,
    };

    let compute_count = Cell::new(0);

    // First call with seqno=1
    let hash1 = shape_hash_lookup(&mut cache, key, 1, || {
        compute_count.set(compute_count.get() + 1);
        [1u8; 16]
    });

    assert_eq!(compute_count.get(), 1, "should compute on first miss");

    // Second call with same seqno=1: cache hit, no recompute
    let hash2 = shape_hash_lookup(&mut cache, key, 1, || {
        compute_count.set(compute_count.get() + 1);
        [2u8; 16]
    });

    assert_eq!(compute_count.get(), 1, "should NOT recompute on cache hit");
    assert_eq!(hash1, hash2, "should return same cached hash");

    // Third call with different seqno=2: seqno mismatch, must recompute
    let hash3 = shape_hash_lookup(&mut cache, key, 2, || {
        compute_count.set(compute_count.get() + 1);
        [3u8; 16]
    });

    assert_eq!(compute_count.get(), 2, "should recompute on seqno mismatch");
    assert_eq!(hash3, [3u8; 16], "should return newly computed hash");
}

/// Task #439: Empirical test measuring actual cache hit rate over multiple frames.
/// Simulates 50 frames over 50 unchanged lines (static screen).
/// Frame 1 is expected to be 0% hits (cache cold), frames 2..50 expected ~100% hits.
#[test]
fn test_shape_hash_cache_hit_rate_static_screen() {
    use lfucache::LfuCache;
    use std::cell::Cell;

    let num_lines = 50;
    let num_frames = 50;
    let _capacity = std::num::NonZeroUsize::new(1024).unwrap();
    let mut cache = LfuCache::new(
        "static_hit",
        "static_miss",
        test_cache_capacity,
        &ConfigHandle::default_config(),
    );

    let compute_count = Cell::new(0);
    let hit_count = Cell::new(0);
    let miss_count = Cell::new(0);

    // Simulate static screen: same 50 lines with same seqno across all frames
    for _frame in 0..num_frames {
        for line_idx in 0..num_lines {
            let pane_id: PaneId = 1;
            let stable_row: StableRowIndex = line_idx as StableRowIndex;
            let key = ShapeHashCacheKey {
                pane_id,
                stable_row,
            };
            let seqno: SequenceNo = 1; // Static screen: seqno never changes

            let frame_compute_count = compute_count.get();

            shape_hash_lookup(&mut cache, key, seqno, || {
                compute_count.set(compute_count.get() + 1);
                // Simulate expensive computation: hash based on line index
                let mut hash = [0u8; 16];
                hash[0] = line_idx as u8;
                hash
            });

            // Track hits vs misses
            if compute_count.get() > frame_compute_count {
                miss_count.set(miss_count.get() + 1);
            } else {
                hit_count.set(hit_count.get() + 1);
            }
        }
    }

    let total_lookups = hit_count.get() + miss_count.get();
    let overall_hit_rate = (hit_count.get() as f64) / (total_lookups as f64) * 100.0;

    // Frame 1: all misses (cache cold)
    assert_eq!(miss_count.get(), num_lines, "frame 1 should be all misses");

    // Frames 2..50: all hits (static screen, cache warm)
    // Total lookups: 50 frames * 50 lines = 2500
    // Misses: 50 (frame 1 only)
    // Hits: 2500 - 50 = 2450
    // Expected hit rate: 2450 / 2500 = 98%
    assert_eq!(total_lookups, num_frames * num_lines, "total lookups count");
    assert_eq!(
        hit_count.get(),
        (num_frames - 1) * num_lines,
        "frames 2..50 should be all hits"
    );

    println!(
        "✓ Static screen cache hit rate: {:.2}% ({}/{} hits, {} computes)",
        overall_hit_rate,
        hit_count.get(),
        total_lookups,
        compute_count.get()
    );
    println!("  Frame 1: {} misses (cache warmup)", num_lines);
    println!("  Frames 2..50: {} hits (cache warm)", hit_count.get());
}

/// Task #439: Regression test that different panes don't share cache entries.
#[test]
fn test_shape_hash_cache_key_includes_pane_id() {
    use lfucache::LfuCache;
    use std::cell::Cell;

    let _capacity = std::num::NonZeroUsize::new(100).unwrap();
    let mut cache = LfuCache::new(
        "pane_hit",
        "pane_miss",
        test_cache_capacity,
        &ConfigHandle::default_config(),
    );

    let pane_id_1: PaneId = 1;
    let pane_id_2: PaneId = 2;
    let stable_row: StableRowIndex = 0;

    let key1 = ShapeHashCacheKey {
        pane_id: pane_id_1,
        stable_row,
    };
    let key2 = ShapeHashCacheKey {
        pane_id: pane_id_2,
        stable_row,
    };

    // These should be different keys
    assert_ne!(
        key1, key2,
        "keys with different pane_id should be different"
    );

    let compute_count = Cell::new(0);

    // Store for pane 1
    let hash1 = shape_hash_lookup(&mut cache, key1, 1, || {
        compute_count.set(compute_count.get() + 1);
        [1u8; 16]
    });

    assert_eq!(compute_count.get(), 1, "should compute for pane 1");

    // Pane 2 should not see pane 1's entry
    let hash2 = shape_hash_lookup(&mut cache, key2, 1, || {
        compute_count.set(compute_count.get() + 1);
        [2u8; 16]
    });

    assert_eq!(
        compute_count.get(),
        2,
        "pane 2 should not hit pane 1's cache entry"
    );
    assert_ne!(hash1, hash2, "different panes should have different hashes");

    // Both should now have entries
    let cached_hash1 = shape_hash_lookup(&mut cache, key1, 1, || {
        panic!("should hit cache for pane 1")
    });
    let cached_hash2 = shape_hash_lookup(&mut cache, key2, 1, || {
        panic!("should hit cache for pane 2")
    });

    assert_eq!(cached_hash1, hash1, "pane 1 should have its own entry");
    assert_eq!(cached_hash2, hash2, "pane 2 should have its own entry");

    println!("✓ Test passed: cache key correctly includes pane_id");
}

/// Task #476 regression: end-to-end check that the *production*
/// `shape_hash_lookup` never serves a stale hash for a row of a real
/// `Terminal`.
///
/// `ShapeHashCacheKey{pane_id, stable_row}` validated by
/// `entry.seqno == line.current_seqno()` is only sound if the terminal
/// model guarantees that `(StableRowIndex, seqno)` identifies a unique
/// line content. `Screen::scroll_up` used to break that guarantee: with
/// a top-anchored scroll region that stops short of the bottom of the
/// screen (`CSI 1;Nr`, N < rows) and a full scrollback, every scroll
/// advanced `stable_row_index_offset` for the whole screen while the
/// rows *below* the region stayed physically put and were never
/// dirtied. Their StableRowIndex therefore slid by one per scroll with
/// no seqno change, so this cache would serve one row's shaping for a
/// different row -- a line duplicated onto a neighbouring row that no
/// amount of further scrolling could clear.
///
/// This drives a real `Terminal` with real escape sequences, feeds the
/// real `Line`s and their real seqnos through the real
/// `shape_hash_lookup`, and asserts the answer always matches a freshly
/// computed `Line::compute_shape_hash` for the line actually being
/// rendered. It fails (many rows, every frame) without the
/// `Screen::scroll_up` fix.
#[test]
fn test_shape_hash_lookup_never_stale_under_top_anchored_scroll_region() {
    use lfucache::LfuCache;
    use onlyterm_term::color::ColorPalette;
    use onlyterm_term::{Terminal, TerminalConfiguration, TerminalSize};

    #[derive(Debug)]
    struct Cfg;
    impl TerminalConfiguration for Cfg {
        fn scrollback_size(&self) -> usize {
            10
        }
        fn color_palette(&self) -> ColorPalette {
            ColorPalette::default()
        }
    }

    const ROWS: usize = 10;
    const PANE_ID: PaneId = 7;

    /// One frame of the renderer's per-row work: for every visible row,
    /// ask the production cache for that row's shape hash and check it
    /// against the hash of the line that row actually holds right now.
    fn render_frame(
        term: &Terminal,
        cache: &mut LfuCache<ShapeHashCacheKey, ShapeHashEntry>,
        label: &str,
    ) {
        let screen = term.screen();
        let top = screen.visible_row_to_stable_row(0);
        for i in 0..ROWS {
            let stable_row = top + i as StableRowIndex;
            let phys = match screen.stable_row_to_phys(stable_row) {
                Some(phys) => phys,
                None => continue,
            };
            let line = screen.lines_in_phys_range(phys..phys + 1).remove(0);
            let truth = line.compute_shape_hash();
            let served = shape_hash_lookup(
                cache,
                ShapeHashCacheKey {
                    pane_id: PANE_ID,
                    stable_row,
                },
                line.current_seqno(),
                || line.compute_shape_hash(),
            );
            assert_eq!(
                served,
                truth,
                "[{}] stable_row={} (seqno={}) was served a stale shape hash; \
                 the row actually contains {:?}",
                label,
                stable_row,
                line.current_seqno(),
                line.as_str(),
            );
        }
    }

    let mut term = Terminal::new(
        TerminalSize {
            rows: ROWS,
            cols: 20,
            pixel_width: 160,
            pixel_height: 160,
            dpi: 0,
        },
        Arc::new(Cfg),
        "OnlyTerm",
        "0",
        Box::new(Vec::new()),
    );

    let mut cache = LfuCache::new(
        "t476_hit",
        "t476_miss",
        test_cache_capacity,
        &ConfigHandle::default_config(),
    );

    // Fill the screen and overflow the scrollback so that subsequent
    // scrolls have to recycle lines off the front of the buffer.
    for i in 0..25 {
        term.advance_bytes(format!("row{:02}\r\n", i));
    }
    render_frame(&term, &mut cache, "filled");

    // Top-anchored scroll region covering only the upper half of the
    // screen, leaving rows 5..10 below it untouched.
    term.advance_bytes("\x1b[1;5r");

    for step in 0..12 {
        // Newline on the last row of the region scrolls the region.
        term.advance_bytes(format!("\x1b[5;1Hnew{:02}\n", step));
        render_frame(&term, &mut cache, &format!("region scroll {}", step));
    }
}

#[test]
fn fallback_fingerprint_reuses_seqno_and_epoch_cache() {
    let mut cache = lfucache::LfuCache::new(
        "fallback_fingerprint_hit",
        "fallback_fingerprint_miss",
        test_cache_capacity,
        &ConfigHandle::default_config(),
    );
    let key = ShapeHashCacheKey {
        pane_id: 7,
        stable_row: 3,
    };
    let mut computes = 0;
    let first = shape_hash_and_fallback_lookup(&mut cache, key, 1, 0, || {
        computes += 1;
        ([1; 16], 9)
    });
    let second = shape_hash_and_fallback_lookup(&mut cache, key, 1, 0, || {
        computes += 1;
        ([2; 16], 10)
    });
    assert_eq!(first.shape_hash, second.shape_hash);
    assert_eq!(first.fallback_fingerprint, second.fallback_fingerprint);
    assert_eq!(computes, 1);

    let third = shape_hash_and_fallback_lookup(&mut cache, key, 1, 1, || {
        computes += 1;
        ([3; 16], 11)
    });
    assert_eq!(third.fallback_fingerprint, 11);
    assert_eq!(computes, 2);
}
