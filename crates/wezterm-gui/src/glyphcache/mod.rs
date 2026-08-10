use super::utilsprites::RenderMetrics;
use crate::customglyph::*;
use ::window::bitmaps::atlas::{Atlas, Sprite};
use ahash::AHasher;
use config::TextStyle;
use lfucache::LfuCache;
use ordered_float::NotNan;
use std::collections::HashMap;
use std::hash::BuildHasherDefault;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use termwiz::color::RgbColor;
use termwiz::surface::CursorShape;
use wezterm_font::units::*;
use wezterm_font::{FontConfiguration, LoadedFontId};
use wezterm_term::Underline;

// AHashMap: HashMap with ahash's AHasher for faster hashing on internal keys
type AHashMap<K, V> = HashMap<K, V, BuildHasherDefault<AHasher>>;

static FRAME_ERROR_REPORTED: AtomicBool = AtomicBool::new(false);

/// We only want to report a frame error once at error level, because
/// if it is triggering it is likely in a animated image and will continue
/// to trigger multiple times per second as the frames are cycled.
fn report_frame_error<S: Into<String>>(message: S) {
    if FRAME_ERROR_REPORTED.load(Ordering::Relaxed) {
        log::debug!("{}", message.into());
    } else {
        log::error!("{}", message.into());
        FRAME_ERROR_REPORTED.store(true, Ordering::Relaxed);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadState {
    Loading,
    Loaded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CellMetricKey {
    pub pixel_width: u16,
    pub pixel_height: u16,
}

impl From<&RenderMetrics> for CellMetricKey {
    fn from(metrics: &RenderMetrics) -> CellMetricKey {
        CellMetricKey {
            pixel_width: metrics.cell_size.width as u16,
            pixel_height: metrics.cell_size.height as u16,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SizedBlockKey {
    pub block: BlockKey,
    pub size: CellMetricKey,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GlyphKey {
    pub font_idx: usize,
    pub glyph_pos: u32,
    pub num_cells: u8,
    pub style: TextStyle,
    pub followed_by_space: bool,
    pub metric: CellMetricKey,
    pub id: LoadedFontId,
}

/// We'd like to avoid allocating when resolving from the cache
/// so this is the borrowed version of GlyphKey.
/// It's a bit involved to make this work; more details can be
/// found in the excellent guide here:
/// <https://github.com/sunshowers/borrow-complex-key-example/blob/master/src/lib.rs>
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct BorrowedGlyphKey<'a> {
    pub font_idx: usize,
    pub glyph_pos: u32,
    pub num_cells: u8,
    pub style: &'a TextStyle,
    pub followed_by_space: bool,
    pub metric: CellMetricKey,
    pub id: LoadedFontId,
}

impl<'a> BorrowedGlyphKey<'a> {
    fn to_owned(self) -> GlyphKey {
        GlyphKey {
            font_idx: self.font_idx,
            glyph_pos: self.glyph_pos,
            num_cells: self.num_cells,
            style: self.style.clone(),
            followed_by_space: self.followed_by_space,
            metric: self.metric,
            id: self.id,
        }
    }
}

trait GlyphKeyTrait {
    fn key<'k>(&'k self) -> BorrowedGlyphKey<'k>;
}

impl GlyphKeyTrait for GlyphKey {
    fn key<'k>(&'k self) -> BorrowedGlyphKey<'k> {
        BorrowedGlyphKey {
            font_idx: self.font_idx,
            glyph_pos: self.glyph_pos,
            num_cells: self.num_cells,
            style: &self.style,
            followed_by_space: self.followed_by_space,
            metric: self.metric,
            id: self.id,
        }
    }
}

impl<'a> GlyphKeyTrait for BorrowedGlyphKey<'a> {
    fn key<'k>(&'k self) -> BorrowedGlyphKey<'k> {
        *self
    }
}

impl<'a> std::borrow::Borrow<dyn GlyphKeyTrait + 'a> for GlyphKey {
    fn borrow(&self) -> &(dyn GlyphKeyTrait + 'a) {
        self
    }
}

impl<'a> PartialEq for dyn GlyphKeyTrait + 'a {
    fn eq(&self, other: &Self) -> bool {
        self.key().eq(&other.key())
    }
}

impl<'a> Eq for dyn GlyphKeyTrait + 'a {}

impl<'a> std::hash::Hash for dyn GlyphKeyTrait + 'a {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.key().hash(state)
    }
}

/// Caches a rendered glyph.
/// The image data may be None for whitespace glyphs.
pub struct CachedGlyph {
    pub has_color: bool,
    pub brightness_adjust: f32,
    pub x_offset: PixelLength,
    pub y_offset: PixelLength,
    pub x_advance: PixelLength,
    pub bearing_x: PixelLength,
    pub bearing_y: PixelLength,
    pub texture: Option<Sprite>,
    pub scale: f64,
}

impl std::fmt::Debug for CachedGlyph {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::result::Result<(), std::fmt::Error> {
        fmt.debug_struct("CachedGlyph")
            .field("has_color", &self.has_color)
            .field("x_advance", &self.x_advance)
            .field("x_offset", &self.x_offset)
            .field("y_offset", &self.y_offset)
            .field("bearing_x", &self.bearing_x)
            .field("bearing_y", &self.bearing_y)
            .field("scale", &self.scale)
            .field("texture", &self.texture)
            .finish()
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Hash)]
struct LineKey {
    strike_through: bool,
    underline: Underline,
    overline: bool,
    size: CellMetricKey,
}

/// A number of items here are HashMaps rather than LfuCaches;
/// eviction is managed by recreating Self when the Atlas is filled
///
/// We use AHashMap (ahash::AHasher) for internal-only struct/numeric keys
/// where benchmarking shows 40-80% speedups: glyph_cache (GlyphKey),
/// line_glyphs (LineKey), block_glyphs (SizedBlockKey).
///
/// External string/input maps remain std::HashMap: frame_cache ([u8; 32]),
/// cursor_glyphs ((Option<CursorShape>, u8)), color ((RgbColor, NotNan<f32>)).
pub struct GlyphCache {
    glyph_cache: AHashMap<GlyphKey, Rc<CachedGlyph>>,
    // Resolved once rather than on every `cached_glyph()` call (once per
    // rendered glyph per frame): `metrics::histogram!(name)` re-resolves
    // through the recorder's `register_histogram` -- a global mutex plus a
    // string-keyed hashmap lookup -- on every invocation.
    glyph_cache_hit: metrics::Histogram,
    glyph_cache_miss: metrics::Histogram,
    pub atlas: Atlas,
    pub fonts: Rc<FontConfiguration>,
    pub image_cache: LfuCache<[u8; 32], DecodedImage>,
    frame_cache: HashMap<[u8; 32], Sprite>,
    line_glyphs: AHashMap<LineKey, Sprite>,
    pub block_glyphs: AHashMap<SizedBlockKey, Sprite>,
    pub cursor_glyphs: HashMap<(Option<CursorShape>, u8), Sprite>,
    pub color: HashMap<(RgbColor, NotNan<f32>), Sprite>,
    min_frame_duration: Duration,
}

mod glyph_cache_impl;
mod image_decode;

pub use image_decode::DecodedImage;
use image_decode::DecodedImageHandle;

#[cfg(test)]
mod hashmap_bench {
    use super::*;
    use ahash::AHasher;
    use std::collections::HashMap;
    use std::hash::BuildHasherDefault;
    use std::rc::Rc;
    use std::time::Duration;

    // Type alias for AHashMap using ahash's AHasher
    type AHashMap<K, V> = HashMap<K, V, BuildHasherDefault<AHasher>>;

    /// Create a realistic GlyphKey for benchmarking
    fn make_glyph_key(id: u32) -> GlyphKey {
        GlyphKey {
            font_idx: (id % 4) as usize, // Simulate 4 different fonts
            glyph_pos: id,
            num_cells: 1,
            style: TextStyle::default(),
            followed_by_space: id.is_multiple_of(2),
            metric: CellMetricKey {
                pixel_width: 8 + ((id % 3) as u16), // Typical cell widths: 8, 9, 10
                pixel_height: 16 + ((id % 2) as u16), // Typical cell heights: 16, 17
            },
            id: wezterm_font::LoadedFontId::from((id % 2) as usize), // 2 different loaded fonts
        }
    }

    /// Create a realistic CachedGlyph for benchmarking
    fn make_cached_glyph(id: u32) -> Rc<CachedGlyph> {
        Rc::new(CachedGlyph {
            has_color: id.is_multiple_of(3),
            brightness_adjust: (id % 11) as f32 / 10.0,
            x_offset: wezterm_font::units::PixelLength::new((((id % 5) as f32) / 10.0) as f64),
            y_offset: wezterm_font::units::PixelLength::new((((id % 5) as f32) / 10.0) as f64),
            x_advance: wezterm_font::units::PixelLength::new((8.0 + ((id % 3) as f32)) as f64),
            bearing_x: wezterm_font::units::PixelLength::new((((id % 5) as f32) / 10.0) as f64),
            bearing_y: wezterm_font::units::PixelLength::new((12.0 + ((id % 3) as f32)) as f64),
            texture: None, // Keep it simple for benchmarking
            scale: 1.0,
        })
    }

    /// Create a realistic LineKey for benchmarking
    fn make_line_key(id: u32) -> LineKey {
        LineKey {
            strike_through: id.is_multiple_of(3),
            underline: match id % 4 {
                0 => wezterm_term::Underline::None,
                1 => wezterm_term::Underline::Single,
                2 => wezterm_term::Underline::Double,
                _ => wezterm_term::Underline::Curly,
            },
            overline: id.is_multiple_of(5),
            size: CellMetricKey {
                pixel_width: 8 + ((id % 3) as u16),
                pixel_height: 16 + ((id % 2) as u16),
            },
        }
    }

    /// Create a realistic SizedBlockKey for benchmarking
    fn make_sized_block_key(id: u32) -> SizedBlockKey {
        // Use a simple Spinner pattern since it's easy to construct
        SizedBlockKey {
            block: BlockKey::Spinner((id % 8) as u8),
            size: CellMetricKey {
                pixel_width: 8 + ((id % 3) as u16),
                pixel_height: 16 + ((id % 2) as u16),
            },
        }
    }

    /// Simulate miss-heavy burst: mostly new glyphs, few repeats.
    /// During flood output, renderer sees mostly new glyphs with very few cache hits.
    fn miss_heavy_glyph_sequence(count: usize) -> Vec<GlyphKey> {
        let mut keys = Vec::with_capacity(count);
        for i in 0..count {
            if i % 20 == 0 && i > 100 {
                // Repeat a recent key ~5% of the time
                keys.push(make_glyph_key((i - 50) as u32));
            } else {
                keys.push(make_glyph_key(i as u32));
            }
        }
        keys
    }

    /// Simulate stable screen: small working set accessed repeatedly.
    /// During static screen redraws, renderer repeatedly hits the same small set of glyphs.
    fn stable_screen_glyph_sequence(count: usize, working_set_size: usize) -> Vec<GlyphKey> {
        let mut keys = Vec::with_capacity(count);
        for i in 0..count {
            // Cycle through a small working set (e.g., 200 common glyphs for 80x24 screen)
            keys.push(make_glyph_key((i % working_set_size) as u32));
        }
        keys
    }

    /// Benchmark std::HashMap performance for glyph cache
    fn benchmark_std_hashmap(keys: &[GlyphKey]) -> (Duration, usize, usize) {
        benchmarking::warm_up();

        let keys = keys.to_vec();
        let keys_for_bench = keys.clone();
        let bench_result = benchmarking::measure_function(move |measurer| {
            measurer.measure(|| {
                let mut cache: HashMap<GlyphKey, Rc<CachedGlyph>> = HashMap::new();
                let mut hits = 0;
                let mut misses = 0;

                for key in &keys_for_bench {
                    if cache.contains_key(key) {
                        hits += 1;
                    } else {
                        misses += 1;
                        cache.insert(key.clone(), make_cached_glyph(key.glyph_pos));
                    }
                }

                std::hint::black_box(&mut cache);
                std::hint::black_box(hits);
                std::hint::black_box(misses);
            })
        })
        .unwrap();

        let mut cache: HashMap<GlyphKey, Rc<CachedGlyph>> = HashMap::new();
        let mut hits = 0;
        let mut misses = 0;
        for key in &keys {
            if cache.contains_key(key) {
                hits += 1;
            } else {
                misses += 1;
                cache.insert(key.clone(), make_cached_glyph(key.glyph_pos));
            }
        }

        (bench_result.elapsed(), hits, misses)
    }

    /// Benchmark AHashMap performance for glyph cache
    fn benchmark_ahashmap(keys: &[GlyphKey]) -> (Duration, usize, usize) {
        benchmarking::warm_up();

        let keys = keys.to_vec();
        let keys_for_bench = keys.clone();
        let bench_result = benchmarking::measure_function(move |measurer| {
            measurer.measure(|| {
                let mut cache: AHashMap<GlyphKey, Rc<CachedGlyph>> =
                    HashMap::with_hasher(BuildHasherDefault::default());
                let mut hits = 0;
                let mut misses = 0;

                for key in &keys_for_bench {
                    if cache.contains_key(key) {
                        hits += 1;
                    } else {
                        misses += 1;
                        cache.insert(key.clone(), make_cached_glyph(key.glyph_pos));
                    }
                }

                std::hint::black_box(&mut cache);
                std::hint::black_box(hits);
                std::hint::black_box(misses);
            })
        })
        .unwrap();

        let mut cache: AHashMap<GlyphKey, Rc<CachedGlyph>> =
            HashMap::with_hasher(BuildHasherDefault::default());
        let mut hits = 0;
        let mut misses = 0;
        for key in &keys {
            if cache.contains_key(key) {
                hits += 1;
            } else {
                misses += 1;
                cache.insert(key.clone(), make_cached_glyph(key.glyph_pos));
            }
        }

        (bench_result.elapsed(), hits, misses)
    }

    /// Measure only lookup latency (cache already populated)
    fn benchmark_lookup_std_hashmap(keys: &[GlyphKey]) -> Duration {
        benchmarking::warm_up();

        // Pre-populate cache
        let mut cache: HashMap<GlyphKey, Rc<CachedGlyph>> = HashMap::new();
        for key in keys {
            if !cache.contains_key(key) {
                cache.insert(key.clone(), make_cached_glyph(key.glyph_pos));
            }
        }

        let keys_for_bench = keys.to_vec();
        let bench_result = benchmarking::measure_function(move |measurer| {
            measurer.measure(|| {
                let cache = &cache;
                for key in &keys_for_bench {
                    std::hint::black_box(cache.get(key));
                }
            })
        })
        .unwrap();

        bench_result.elapsed()
    }

    /// Measure only lookup latency (cache already populated)
    fn benchmark_lookup_ahashmap(keys: &[GlyphKey]) -> Duration {
        benchmarking::warm_up();

        // Pre-populate cache
        let mut cache: AHashMap<GlyphKey, Rc<CachedGlyph>> =
            HashMap::with_hasher(BuildHasherDefault::default());
        for key in keys {
            if !cache.contains_key(key) {
                cache.insert(key.clone(), make_cached_glyph(key.glyph_pos));
            }
        }

        let keys_for_bench = keys.to_vec();
        let bench_result = benchmarking::measure_function(move |measurer| {
            measurer.measure(|| {
                let cache = &cache;
                for key in &keys_for_bench {
                    std::hint::black_box(cache.get(key));
                }
            })
        })
        .unwrap();

        bench_result.elapsed()
    }

    /// Measure only insert latency (fresh cache)
    fn benchmark_insert_std_hashmap(keys: &[GlyphKey]) -> Duration {
        benchmarking::warm_up();

        let keys_for_bench = keys.to_vec();
        let bench_result = benchmarking::measure_function(move |measurer| {
            measurer.measure(|| {
                let mut cache: HashMap<GlyphKey, Rc<CachedGlyph>> = HashMap::new();
                for key in &keys_for_bench {
                    cache.insert(key.clone(), make_cached_glyph(key.glyph_pos));
                }
                std::hint::black_box(&mut cache);
            })
        })
        .unwrap();

        bench_result.elapsed()
    }

    /// Measure only insert latency (fresh cache)
    fn benchmark_insert_ahashmap(keys: &[GlyphKey]) -> Duration {
        benchmarking::warm_up();

        let keys_for_bench = keys.to_vec();
        let bench_result = benchmarking::measure_function(move |measurer| {
            measurer.measure(|| {
                let mut cache: AHashMap<GlyphKey, Rc<CachedGlyph>> =
                    HashMap::with_hasher(BuildHasherDefault::default());
                for key in &keys_for_bench {
                    cache.insert(key.clone(), make_cached_glyph(key.glyph_pos));
                }
                std::hint::black_box(&mut cache);
            })
        })
        .unwrap();

        bench_result.elapsed()
    }

    #[test]
    fn bench_glyph_cache_hashmap_vs_ahashmap() {
        benchmarking::warm_up();

        println!("\n=== GlyphCache HashMap vs AHashMap Benchmark ===");
        println!("GlyphKey size: {} bytes", std::mem::size_of::<GlyphKey>());
        println!(
            "CachedGlyph size: {} bytes (Rc<CachedGlyph>)",
            std::mem::size_of::<Rc<CachedGlyph>>()
        );

        // Test different cache sizes representing realistic working sets
        for &cache_size in &[200, 500, 1000, 2000] {
            println!("\n--- Cache size: {} glyphs ---", cache_size);

            // Test 1: Stable screen pattern (mostly cache hits)
            println!(
                "\nStable screen pattern ({} lookups, mostly hits):",
                cache_size * 10
            );
            let stable_keys = stable_screen_glyph_sequence(cache_size * 10, cache_size);

            let (std_time, std_hits, std_misses) = benchmark_std_hashmap(&stable_keys);
            println!(
                "  std::HashMap: {:?} ({:.2} ns/op), {} hits, {} misses",
                std_time,
                std_time.as_nanos() as f64 / (stable_keys.len() as f64),
                std_hits,
                std_misses
            );

            let (ahash_time, ahash_hits, ahash_misses) = benchmark_ahashmap(&stable_keys);
            println!(
                "  AHashMap:      {:?} ({:.2} ns/op), {} hits, {} misses",
                ahash_time,
                ahash_time.as_nanos() as f64 / (stable_keys.len() as f64),
                ahash_hits,
                ahash_misses
            );

            if std_time.as_nanos() > 0 {
                let ratio = (ahash_time.as_nanos() as f64 / std_time.as_nanos() as f64) * 100.0;
                if ratio < 100.0 {
                    println!("  → AHashMap is {:.1}% faster", 100.0 - ratio);
                } else {
                    println!("  → AHashMap is {:.1}% slower", ratio - 100.0);
                }
            }

            // Test 2: Miss-heavy pattern (flood output)
            println!(
                "\nMiss-heavy pattern ({} lookups, mostly misses):",
                cache_size * 10
            );
            let miss_heavy_keys = miss_heavy_glyph_sequence(cache_size * 10);

            let (std_time, std_hits, std_misses) = benchmark_std_hashmap(&miss_heavy_keys);
            println!(
                "  std::HashMap: {:?} ({:.2} ns/op), {} hits, {} misses",
                std_time,
                std_time.as_nanos() as f64 / (miss_heavy_keys.len() as f64),
                std_hits,
                std_misses
            );

            let (ahash_time, ahash_hits, ahash_misses) = benchmark_ahashmap(&miss_heavy_keys);
            println!(
                "  AHashMap:      {:?} ({:.2} ns/op), {} hits, {} misses",
                ahash_time,
                ahash_time.as_nanos() as f64 / (miss_heavy_keys.len() as f64),
                ahash_hits,
                ahash_misses
            );

            if std_time.as_nanos() > 0 {
                let ratio = (ahash_time.as_nanos() as f64 / std_time.as_nanos() as f64) * 100.0;
                if ratio < 100.0 {
                    println!("  → AHashMap is {:.1}% faster", 100.0 - ratio);
                } else {
                    println!("  → AHashMap is {:.1}% slower", ratio - 100.0);
                }
            }

            // Test 3: Pure lookup latency (cache already warm)
            println!(
                "\nPure lookup latency (cache pre-populated with {} unique glyphs):",
                cache_size
            );
            let unique_keys: Vec<_> = (0..cache_size).map(|i| make_glyph_key(i as u32)).collect();

            let std_lookup_time = benchmark_lookup_std_hashmap(&unique_keys);
            println!(
                "  std::HashMap: {:?} ({:.2} ns/op)",
                std_lookup_time,
                std_lookup_time.as_nanos() as f64 / (unique_keys.len() as f64)
            );

            let ahash_lookup_time = benchmark_lookup_ahashmap(&unique_keys);
            println!(
                "  AHashMap:      {:?} ({:.2} ns/op)",
                ahash_lookup_time,
                ahash_lookup_time.as_nanos() as f64 / (unique_keys.len() as f64)
            );

            if std_lookup_time.as_nanos() > 0 {
                let ratio = (ahash_lookup_time.as_nanos() as f64
                    / std_lookup_time.as_nanos() as f64)
                    * 100.0;
                if ratio < 100.0 {
                    println!("  → AHashMap is {:.1}% faster", 100.0 - ratio);
                } else {
                    println!("  → AHashMap is {:.1}% slower", ratio - 100.0);
                }
            }

            // Test 4: Pure insert latency (fresh cache)
            println!(
                "\nPure insert latency (fresh cache, {} inserts):",
                cache_size
            );

            let std_insert_time = benchmark_insert_std_hashmap(&unique_keys);
            println!(
                "  std::HashMap: {:?} ({:.2} ns/op)",
                std_insert_time,
                std_insert_time.as_nanos() as f64 / (unique_keys.len() as f64)
            );

            let ahash_insert_time = benchmark_insert_ahashmap(&unique_keys);
            println!(
                "  AHashMap:      {:?} ({:.2} ns/op)",
                ahash_insert_time,
                ahash_insert_time.as_nanos() as f64 / (unique_keys.len() as f64)
            );

            if std_insert_time.as_nanos() > 0 {
                let ratio = (ahash_insert_time.as_nanos() as f64
                    / std_insert_time.as_nanos() as f64)
                    * 100.0;
                if ratio < 100.0 {
                    println!("  → AHashMap is {:.1}% faster", 100.0 - ratio);
                } else {
                    println!("  → AHashMap is {:.1}% slower", ratio - 100.0);
                }
            }
        }

        println!("\n=== LineKey and SizedBlockKey Benchmarks ===");
        println!("LineKey size: {} bytes", std::mem::size_of::<LineKey>());
        println!(
            "SizedBlockKey size: {} bytes",
            std::mem::size_of::<SizedBlockKey>()
        );

        // Test LineKey performance
        for &cache_size in &[100, 500] {
            println!("\n--- LineKey cache size: {} ---", cache_size);
            let line_keys: Vec<_> = (0..cache_size).map(|i| make_line_key(i as u32)).collect();

            let (std_time, std_hits, std_misses) = benchmark_std_hashmap_line(&line_keys);
            println!(
                "  std::HashMap: {:?} ({:.2} ns/op), {} hits, {} misses",
                std_time,
                std_time.as_nanos() as f64 / (line_keys.len() as f64),
                std_hits,
                std_misses
            );

            let (ahash_time, ahash_hits, ahash_misses) = benchmark_ahashmap_line(&line_keys);
            println!(
                "  AHashMap:      {:?} ({:.2} ns/op), {} hits, {} misses",
                ahash_time,
                ahash_time.as_nanos() as f64 / (line_keys.len() as f64),
                ahash_hits,
                ahash_misses
            );

            if std_time.as_nanos() > 0 {
                let ratio = (ahash_time.as_nanos() as f64 / std_time.as_nanos() as f64) * 100.0;
                if ratio < 100.0 {
                    println!("  → AHashMap is {:.1}% faster", 100.0 - ratio);
                } else {
                    println!("  → AHashMap is {:.1}% slower", ratio - 100.0);
                }
            }
        }

        // Test SizedBlockKey performance
        for &cache_size in &[50, 200] {
            println!("\n--- SizedBlockKey cache size: {} ---", cache_size);
            let block_keys: Vec<_> = (0..cache_size)
                .map(|i| make_sized_block_key(i as u32))
                .collect();

            let (std_time, std_hits, std_misses) = benchmark_std_hashmap_block(&block_keys);
            println!(
                "  std::HashMap: {:?} ({:.2} ns/op), {} hits, {} misses",
                std_time,
                std_time.as_nanos() as f64 / (block_keys.len() as f64),
                std_hits,
                std_misses
            );

            let (ahash_time, ahash_hits, ahash_misses) = benchmark_ahashmap_block(&block_keys);
            println!(
                "  AHashMap:      {:?} ({:.2} ns/op), {} hits, {} misses",
                ahash_time,
                ahash_time.as_nanos() as f64 / (block_keys.len() as f64),
                ahash_hits,
                ahash_misses
            );

            if std_time.as_nanos() > 0 {
                let ratio = (ahash_time.as_nanos() as f64 / std_time.as_nanos() as f64) * 100.0;
                if ratio < 100.0 {
                    println!("  → AHashMap is {:.1}% faster", 100.0 - ratio);
                } else {
                    println!("  → AHashMap is {:.1}% slower", ratio - 100.0);
                }
            }
        }

        println!("\n=== Conclusion ===");
        println!("If AHashMap shows consistent speedup (>5-10%) across realistic patterns,");
        println!("then swapping glyph_cache/line_glyphs/block_glyphs to AHashMap is justified.");
        println!("Otherwise, stick with std::HashMap.");
    }

    // Helper benchmarks for LineKey and SizedBlockKey
    fn benchmark_std_hashmap_line(keys: &[LineKey]) -> (Duration, usize, usize) {
        benchmarking::warm_up();

        // Create a fake sprite for benchmarking (just need a placeholder value)
        let fake_sprite = Sprite {
            texture: Rc::new(::window::bitmaps::ImageTexture::new(1, 1)),
            coords: euclid::rect(0, 0, 1, 1),
        };

        let keys = keys.to_vec();
        let keys_for_bench = keys.clone();
        let bench_result = benchmarking::measure_function(move |measurer| {
            measurer.measure(|| {
                let mut cache: HashMap<LineKey, Sprite> = HashMap::new();
                let mut hits = 0;
                let mut misses = 0;

                for key in &keys_for_bench {
                    if cache.contains_key(key) {
                        hits += 1;
                    } else {
                        misses += 1;
                        cache.insert(*key, fake_sprite.clone());
                    }
                }

                std::hint::black_box(&mut cache);
                std::hint::black_box(hits);
                std::hint::black_box(misses);
            })
        })
        .unwrap();

        (bench_result.elapsed(), keys.len(), 0)
    }

    fn benchmark_ahashmap_line(keys: &[LineKey]) -> (Duration, usize, usize) {
        benchmarking::warm_up();

        let fake_sprite = Sprite {
            texture: Rc::new(::window::bitmaps::ImageTexture::new(1, 1)),
            coords: euclid::rect(0, 0, 1, 1),
        };

        let keys = keys.to_vec();
        let keys_for_bench = keys.clone();
        let bench_result = benchmarking::measure_function(move |measurer| {
            measurer.measure(|| {
                let mut cache: AHashMap<LineKey, Sprite> =
                    HashMap::with_hasher(BuildHasherDefault::default());
                let mut hits = 0;
                let mut misses = 0;

                for key in &keys_for_bench {
                    if cache.contains_key(key) {
                        hits += 1;
                    } else {
                        misses += 1;
                        cache.insert(*key, fake_sprite.clone());
                    }
                }

                std::hint::black_box(&mut cache);
                std::hint::black_box(hits);
                std::hint::black_box(misses);
            })
        })
        .unwrap();

        (bench_result.elapsed(), keys.len(), 0)
    }

    fn benchmark_std_hashmap_block(keys: &[SizedBlockKey]) -> (Duration, usize, usize) {
        benchmarking::warm_up();

        let fake_sprite = Sprite {
            texture: Rc::new(::window::bitmaps::ImageTexture::new(1, 1)),
            coords: euclid::rect(0, 0, 1, 1),
        };

        let keys = keys.to_vec();
        let keys_for_bench = keys.clone();
        let bench_result = benchmarking::measure_function(move |measurer| {
            measurer.measure(|| {
                let mut cache: HashMap<SizedBlockKey, Sprite> = HashMap::new();
                let mut hits = 0;
                let mut misses = 0;

                for key in &keys_for_bench {
                    if cache.contains_key(key) {
                        hits += 1;
                    } else {
                        misses += 1;
                        cache.insert(*key, fake_sprite.clone());
                    }
                }

                std::hint::black_box(&mut cache);
                std::hint::black_box(hits);
                std::hint::black_box(misses);
            })
        })
        .unwrap();

        (bench_result.elapsed(), keys.len(), 0)
    }

    fn benchmark_ahashmap_block(keys: &[SizedBlockKey]) -> (Duration, usize, usize) {
        benchmarking::warm_up();

        let fake_sprite = Sprite {
            texture: Rc::new(::window::bitmaps::ImageTexture::new(1, 1)),
            coords: euclid::rect(0, 0, 1, 1),
        };

        let keys = keys.to_vec();
        let keys_for_bench = keys.clone();
        let bench_result = benchmarking::measure_function(move |measurer| {
            measurer.measure(|| {
                let mut cache: AHashMap<SizedBlockKey, Sprite> =
                    HashMap::with_hasher(BuildHasherDefault::default());
                let mut hits = 0;
                let mut misses = 0;

                for key in &keys_for_bench {
                    if cache.contains_key(key) {
                        hits += 1;
                    } else {
                        misses += 1;
                        cache.insert(*key, fake_sprite.clone());
                    }
                }

                std::hint::black_box(&mut cache);
                std::hint::black_box(hits);
                std::hint::black_box(misses);
            })
        })
        .unwrap();

        (bench_result.elapsed(), keys.len(), 0)
    }
}
