//! Headless tool that shapes a single line of text through the real
//! shaping/bidi/clustering pipeline (the same `Line::cluster` +
//! `RustybuzzShaper` code the GUI uses) and rasterizes it to a PNG,
//! without needing to build/launch the full GUI binary.
//!
//! Mirrors `crates/onlyterm-gui/src/termwindow/render/screen_line.rs`'s
//! default (non-`experimental_pixel_positioning`) glyph layout: each
//! cluster advances by `num_cells * cell_width` (grid-cell mode), while
//! each glyph's actual drawn position within its cell is offset by
//! `x_offset + bearing_x` -- so this reproduces the "narrow glyph left
//! -aligned in its cell, gap on the right" artifact visible in production.
//!
//! Example:
//!
//! ```sh
//! cargo run -p onlyterm-font --example render_line -- \
//!     --text "שלום עולם" --out /tmp/line.png
//! ```

use clap::Parser;
use config::FontAttributes;
use onlyterm_font::db::FontDatabase;
use onlyterm_font::parser::ParsedFont;
use onlyterm_font::rasterizer::new_rasterizer;
use onlyterm_font::shaper::{new_shaper, PresentationWidth};

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Opt {
    /// Text to render. May include Hebrew/RTL and mixed scripts. Prefer
    /// `--text-file` for anything with mixed scripts/RTL: passing such
    /// text as a raw CLI argument through a shell is unreliable (the
    /// shell/console layer can reorder or mangle it before this program
    /// ever sees it).
    #[arg(long)]
    text: Option<String>,

    /// Read the text to render from a UTF-8 file instead of `--text`
    /// (trailing newline stripped). Use this for RTL/mixed-script input.
    #[arg(long)]
    text_file: Option<std::path::PathBuf>,

    /// Number of terminal columns to allocate for the canvas.
    #[arg(long, default_value_t = 60)]
    cols: usize,

    /// Font size in points.
    #[arg(long, default_value_t = 14.0)]
    size: f64,

    /// DPI to render at.
    #[arg(long, default_value_t = 96)]
    dpi: u32,

    /// Primary (base) font family name.
    #[arg(long, default_value = "JetBrains Mono")]
    primary_font: String,

    /// Fallback font family names, in order, appended after the primary.
    #[arg(long, default_value = "Cascadia Mono")]
    fallback_fonts: Vec<String>,

    /// Use pixel positioning (actual glyph x_advance) instead of the
    /// default grid-cell (`num_cells * cell_width`) advance mode.
    #[arg(long, default_value_t = false)]
    pixel_positioning: bool,

    /// Output PNG path.
    #[arg(long)]
    out: std::path::PathBuf,
}

fn resolve(db: &FontDatabase, family: &str, is_fallback: bool) -> anyhow::Result<ParsedFont> {
    db.resolve(
        &FontAttributes {
            family: family.to_string(),
            stretch: Default::default(),
            weight: Default::default(),
            is_fallback,
            is_synthetic: false,
            style: Default::default(),
            freetype_load_flags: None,
            freetype_load_target: None,
            freetype_render_target: None,
            harfbuzz_features: None,
            scale: None,
            assume_emoji_presentation: None,
        },
        14,
    )
    .cloned()
    .ok_or_else(|| anyhow::anyhow!("no font found for family {family:?}"))
}

fn main() -> anyhow::Result<()> {
    env_logger::init();
    let opt = Opt::parse();

    let text = match (&opt.text, &opt.text_file) {
        (_, Some(path)) => std::fs::read_to_string(path)?
            .trim_end_matches('\n')
            .to_string(),
        (Some(text), None) => text.clone(),
        (None, None) => anyhow::bail!("either --text or --text-file must be given"),
    };

    let db = FontDatabase::with_built_in()?;
    let mut handles = vec![resolve(&db, &opt.primary_font, false)?];
    for fam in &opt.fallback_fonts {
        handles.push(resolve(&db, fam, true)?);
    }

    let config = config::configuration();
    let shaper = new_shaper(&config, &handles)?;
    let metrics = shaper.metrics(opt.size, opt.dpi)?;
    let cell_width = metrics.cell_width.get() as f32;
    let cell_height = metrics.cell_height.get() as f32;
    let descender = metrics.descender.get() as f32;

    use onlyterm_bidi::ParagraphDirectionHint;
    use termwiz::cell::CellAttributes;
    use termwiz::surface::Line;

    let line = Line::from_text(&text, &CellAttributes::default(), 0, None);
    let clusters = line.cluster(Some(ParagraphDirectionHint::AutoLeftToRight));

    let canvas_width = (opt.cols as f32 * cell_width).ceil() as u32;
    let canvas_height = cell_height.ceil() as u32;
    let mut canvas = image::RgbaImage::from_pixel(
        canvas_width.max(1),
        canvas_height.max(1),
        image::Rgba([255, 255, 255, 255]),
    );

    let mut rasterizers = std::collections::HashMap::new();

    // Default (non-experimental) line direction: our config always
    // resolves to `AutoLeftToRight`, whose `.direction()` is `LeftToRight`
    // regardless of the paragraph's actual bidi content -- see
    // `onlyterm_bidi::ParagraphDirectionHint::direction`. `Line::cluster`
    // already returns clusters in visual (screen) left-to-right order, and
    // each cluster's own glyphs come back from the shaper pre-ordered for
    // left-to-right drawing (harfbuzz/rustybuzz convention for RTL runs),
    // so we can just walk forward through both lists unconditionally --
    // exactly what `screen_line.rs` does for this (default) config.
    let mut cluster_x_pos: f32 = 0.;

    for cluster in &clusters {
        let presentation_width = PresentationWidth::with_cluster(cluster);
        let mut no_glyphs = vec![];
        let infos = shaper.shape(
            &cluster.text,
            opt.size,
            opt.dpi,
            &mut no_glyphs,
            Some(cluster.presentation),
            cluster.direction,
            None,
            Some(&presentation_width),
        )?;
        if !no_glyphs.is_empty() {
            eprintln!("cluster {:?}: no glyph for {:?}", cluster.text, no_glyphs);
        }

        for info in &infos {
            let rasterizer = rasterizers.entry(info.font_idx).or_insert_with(|| {
                new_rasterizer(config.font_rasterizer, &handles[info.font_idx])
                    .expect("new_rasterizer")
            });

            if info.glyph_pos != 0 {
                if let Ok(glyph) = rasterizer.rasterize_glyph(info.glyph_pos, opt.size, opt.dpi) {
                    if glyph.width > 0 && glyph.height > 0 {
                        // Mirrors screen_line.rs: `pos_x = cluster_x_pos +
                        // (x_offset + bearing_x)`, `top = cell_height +
                        // (descender - (y_offset + bearing_y))`.
                        let px = cluster_x_pos
                            + info.x_offset.get() as f32
                            + glyph.bearing_x.get() as f32;
                        let top = cell_height
                            + (descender
                                - (info.y_offset.get() as f32 + glyph.bearing_y.get() as f32));
                        blit(&mut canvas, &glyph, px.round() as i32, top.round() as i32);
                    }
                }
            }

            eprintln!(
                "cluster_x_pos={:.1} only_char={:?} num_cells={} x_advance={:.2} glyph_pos={} \
                 font_idx={} byte_cluster={}",
                cluster_x_pos,
                info.only_char,
                info.num_cells,
                info.x_advance.get(),
                info.glyph_pos,
                info.font_idx,
                info.cluster
            );

            cluster_x_pos += if opt.pixel_positioning {
                info.x_advance.get() as f32
            } else {
                info.num_cells as f32 * cell_width
            };
        }
    }

    canvas.save(&opt.out)?;
    eprintln!("wrote {}", opt.out.display());
    Ok(())
}

fn blit(
    canvas: &mut image::RgbaImage,
    glyph: &onlyterm_font::rasterizer::RasterizedGlyph,
    x: i32,
    y: i32,
) {
    for gy in 0..glyph.height {
        let dy = y + gy as i32;
        if dy < 0 || dy as u32 >= canvas.height() {
            continue;
        }
        for gx in 0..glyph.width {
            let dx = x + gx as i32;
            if dx < 0 || dx as u32 >= canvas.width() {
                continue;
            }
            let idx = (gy * glyph.width + gx) * 4;
            let a = glyph.data[idx + 3] as f32 / 255.0;
            if a <= 0.0 {
                continue;
            }
            let src = [
                glyph.data[idx] as f32,
                glyph.data[idx + 1] as f32,
                glyph.data[idx + 2] as f32,
            ];
            let dst = canvas.get_pixel_mut(dx as u32, dy as u32);
            for (c, &src_val) in src.iter().enumerate() {
                dst.0[c] = ((src_val * a) + (dst.0[c] as f32 * (1.0 - a))).round() as u8;
            }
        }
    }
}
