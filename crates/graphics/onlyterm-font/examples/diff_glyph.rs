//! Headless PNG diff tool for agent-driven, without-a-human verification
//! of glyph rendering. Companion to `dump_glyph`: compares two PNGs
//! (typically a cairo-based baseline vs. a tiny-skia-based result) on a
//! per-pixel/per-channel basis and reports whether they match within a
//! configurable tolerance.
//!
//! Exits with a non-zero status code if the images differ by more than the
//! configured threshold, so it can be used directly in scripts/CI without a
//! human looking at the images.
//!
//! Example:
//!
//! ```sh
//! cargo run -p onlyterm-font --example diff_glyph -- \
//!     --a /tmp/grinning-freetype.png --b /tmp/grinning-harfbuzz.png \
//!     --meta-a /tmp/grinning-freetype.json --meta-b /tmp/grinning-harfbuzz.json
//! ```

use clap::Parser;
use image::RgbaImage;
use std::path::PathBuf;

/// Compare two glyph PNGs (as produced by `dump_glyph`) and report whether
/// they match within tolerance.
#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Opt {
    /// First (e.g. baseline/reference) PNG.
    #[arg(long)]
    a: PathBuf,

    /// Second (e.g. candidate/new) PNG.
    #[arg(long)]
    b: PathBuf,

    /// Optional JSON metadata sidecar for `a`, as produced by `dump_glyph`.
    #[arg(long)]
    meta_a: Option<PathBuf>,

    /// Optional JSON metadata sidecar for `b`, as produced by `dump_glyph`.
    #[arg(long)]
    meta_b: Option<PathBuf>,

    /// Maximum allowed per-channel absolute value deviation (0-255) for a
    /// pixel to still be considered "matching".
    #[arg(long, default_value_t = 24)]
    channel_threshold: u8,

    /// Maximum allowed fraction (0.0-1.0) of pixels that may exceed
    /// `channel-threshold` before the images are considered different.
    #[arg(long, default_value_t = 0.02)]
    max_diff_fraction: f64,

    /// Allow image dimensions to differ; if set, comparison is done over
    /// the overlapping region and the size mismatch itself does not cause
    /// a failure by itself (still reported).
    #[arg(long, default_value_t = false)]
    allow_size_mismatch: bool,
}

fn load_rgba(path: &PathBuf) -> anyhow::Result<RgbaImage> {
    let img =
        image::open(path).map_err(|e| anyhow::anyhow!("failed to open {}: {e}", path.display()))?;
    Ok(img.to_rgba8())
}

fn load_meta(path: &Option<PathBuf>) -> anyhow::Result<Option<serde_json::Value>> {
    match path {
        None => Ok(None),
        Some(p) => {
            let text = std::fs::read_to_string(p)
                .map_err(|e| anyhow::anyhow!("failed to read {}: {e}", p.display()))?;
            Ok(Some(serde_json::from_str(&text)?))
        }
    }
}

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let opt = Opt::parse();

    let img_a = load_rgba(&opt.a)?;
    let img_b = load_rgba(&opt.b)?;

    println!(
        "a: {} ({}x{})",
        opt.a.display(),
        img_a.width(),
        img_a.height()
    );
    println!(
        "b: {} ({}x{})",
        opt.b.display(),
        img_b.width(),
        img_b.height()
    );

    let mut ok = true;

    if img_a.width() != img_b.width() || img_a.height() != img_b.height() {
        println!(
            "DIMENSION MISMATCH: a={}x{} b={}x{}",
            img_a.width(),
            img_a.height(),
            img_b.width(),
            img_b.height()
        );
        if !opt.allow_size_mismatch {
            ok = false;
        }
    }

    let width = img_a.width().min(img_b.width());
    let height = img_a.height().min(img_b.height());

    let mut max_channel_diff: u32 = 0;
    let mut diff_pixel_count: u64 = 0;
    let mut total_pixels: u64 = 0;
    let mut sum_abs_diff: u64 = 0;

    for y in 0..height {
        for x in 0..width {
            let pa = img_a.get_pixel(x, y);
            let pb = img_b.get_pixel(x, y);
            total_pixels += 1;

            let mut pixel_max_diff: u32 = 0;
            for c in 0..4 {
                let da = pa[c] as i32;
                let db = pb[c] as i32;
                let diff = (da - db).unsigned_abs();
                sum_abs_diff += diff as u64;
                pixel_max_diff = pixel_max_diff.max(diff);
            }
            max_channel_diff = max_channel_diff.max(pixel_max_diff);

            if pixel_max_diff > opt.channel_threshold as u32 {
                diff_pixel_count += 1;
            }
        }
    }

    let diff_fraction = if total_pixels == 0 {
        0.0
    } else {
        diff_pixel_count as f64 / total_pixels as f64
    };
    let mean_abs_diff = if total_pixels == 0 {
        0.0
    } else {
        sum_abs_diff as f64 / (total_pixels as f64 * 4.0)
    };

    println!("compared region: {width}x{height}");
    println!("max per-channel abs diff: {max_channel_diff}");
    println!("mean per-channel abs diff: {mean_abs_diff:.3}");
    println!(
        "pixels exceeding channel-threshold={}: {diff_pixel_count}/{total_pixels} ({:.4}%)",
        opt.channel_threshold,
        diff_fraction * 100.0
    );
    println!(
        "max-diff-fraction allowed: {:.4}%",
        opt.max_diff_fraction * 100.0
    );

    if diff_fraction > opt.max_diff_fraction {
        println!("RESULT: FAIL (diff fraction exceeds threshold)");
        ok = false;
    } else {
        println!("RESULT: {}", if ok { "PASS" } else { "FAIL" });
    }

    if let (Some(meta_a_path), Some(meta_b_path)) = (&opt.meta_a, &opt.meta_b) {
        let meta_a = load_meta(&Some(meta_a_path.clone()))?;
        let meta_b = load_meta(&Some(meta_b_path.clone()))?;
        if let (Some(meta_a), Some(meta_b)) = (meta_a, meta_b) {
            println!("meta a: {meta_a}");
            println!("meta b: {meta_b}");

            for field in ["width", "height", "bearing_x", "bearing_y", "has_color"] {
                let va = meta_a.get(field);
                let vb = meta_b.get(field);
                if va != vb {
                    println!("METADATA MISMATCH on `{field}`: a={:?} b={:?}", va, vb);
                    ok = false;
                }
            }
        }
    }

    if !ok {
        std::process::exit(1);
    }

    Ok(())
}
