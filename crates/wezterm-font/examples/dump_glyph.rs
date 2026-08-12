//! Headless tool to rasterize a single glyph from a font file and dump it
//! to a PNG (plus a JSON metadata sidecar) for agent-driven, without-a-human
//! verification of glyph rendering (in particular COLR/COLRv1 color glyphs).
//!
//! This is part of the freetype+harfbuzz -> rustybuzz+swash migration for
//! `wezterm-font`: it exercises the production
//! `wezterm_font::rasterizer::swash::SwashRasterizer` (selected via
//! `wezterm_font::rasterizer::new_rasterizer` /
//! `config::FontRasterizerSelection::Swash`, the crate's default), which
//! handles both ordinary glyph outlines and COLR/COLRv1/CBDT/sbix color
//! glyphs (the latter via its internal, pure-Rust
//! `wezterm_font::rasterizer::colr_paint::ColrRasterizer` fallback) in a
//! single path. The FreeType- and HarfBuzz-backed rasterizers this tool
//! used to also support have been removed from the workspace; see
//! `docs/plans/2026-07-23-freetype-harfbuzz-migration.md`.
//!
//! The companion `diff_glyph` example compares two PNGs (e.g. a pre-migration
//! baseline vs. a freshly-dumped result) on a per-pixel basis so that visual
//! regressions can be caught without a human looking at the images. Baseline
//! PNGs/JSON live under `wezterm-font/testdata/baseline/` (some filenames
//! still mention "freetype"/"harfbuzz" as historical artifacts from when
//! this tool supported those rasterizers; that's expected and left as-is).
//!
//! Example:
//!
//! ```sh
//! cargo run -p wezterm-font --example dump_glyph -- \
//!     --font assets/fonts/NotoColorEmoji.ttf \
//!     --codepoint 1F600 \
//!     --size 32 --dpi 96 \
//!     --rasterizer swash \
//!     --out /tmp/grinning
//! ```
//!
//! This writes `/tmp/grinning.png` and `/tmp/grinning.json`.

use clap::{Parser, ValueEnum};
use std::path::{Path, PathBuf};
use wezterm_font::locator::{FontDataHandle, FontDataSource, FontOrigin};
use wezterm_font::parser::ParsedFont;
use wezterm_font::rasterizer::{new_rasterizer, RasterizedGlyph};
use wezterm_font::swash_metrics::SwashFontInfo;

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
enum RasterizerArg {
    /// The production, pure-Rust `swash`-based rasterizer (see
    /// `wezterm_font::rasterizer::swash::SwashRasterizer`). This is the
    /// only rasterizer left in the workspace: it is selected via
    /// `wezterm_font::rasterizer::new_rasterizer` /
    /// `config::FontRasterizerSelection::Swash`, the crate's default.
    Swash,
}

impl RasterizerArg {
    fn selection(self) -> config::FontRasterizerSelection {
        match self {
            RasterizerArg::Swash => config::FontRasterizerSelection::Swash,
        }
    }
}

fn selection_name(sel: config::FontRasterizerSelection) -> &'static str {
    match sel {
        config::FontRasterizerSelection::Swash => "swash",
        config::FontRasterizerSelection::FreeType => "freetype",
        config::FontRasterizerSelection::Harfbuzz => "harfbuzz",
    }
}

/// Headless glyph rasterization dumper: renders a single glyph to a PNG,
/// with a JSON sidecar holding width/height/bearing/has_color metadata,
/// so that results can be compared programmatically (see `diff_glyph`).
#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Opt {
    /// Path to the font file (ttf/otf/ttc) to load.
    #[arg(long)]
    font: PathBuf,

    /// Font collection index, for ttc files with multiple faces.
    #[arg(long, default_value_t = 0)]
    font_index: u32,

    /// Codepoint to rasterize, expressed as a hex string (e.g. "1F600"
    /// or "U+1F600"), decimal, or a literal single character. Mutually
    /// exclusive with --glyph-id.
    #[arg(long)]
    codepoint: Option<String>,

    /// Explicit glyph id to rasterize, bypassing cmap lookup. Mutually
    /// exclusive with --codepoint.
    #[arg(long)]
    glyph_id: Option<u32>,

    /// Font size in points.
    #[arg(long, default_value_t = 32.0)]
    size: f64,

    /// DPI to render at.
    #[arg(long, default_value_t = 96)]
    dpi: u32,

    /// Which rasterizer implementation to exercise. `swash` (the default
    /// and only supported value) is the production
    /// `wezterm_font::rasterizer::swash::SwashRasterizer`.
    #[arg(long, value_enum, default_value_t = RasterizerArg::Swash)]
    rasterizer: RasterizerArg,

    /// Output path. A ".png" is written, plus a ".json" metadata sidecar
    /// (extensions are appended if not already present).
    #[arg(long)]
    out: PathBuf,
}

fn parse_codepoint(input: &str) -> anyhow::Result<char> {
    let trimmed = input
        .trim()
        .trim_start_matches("U+")
        .trim_start_matches("u+");

    // A single literal character, e.g. --codepoint 😀
    if input.chars().count() == 1 {
        return Ok(input.chars().next().unwrap());
    }

    let value = if let Ok(v) = u32::from_str_radix(trimmed, 16) {
        v
    } else if let Ok(v) = input.parse::<u32>() {
        v
    } else {
        anyhow::bail!("unable to parse codepoint from `{input}`");
    };

    char::from_u32(value).ok_or_else(|| anyhow::anyhow!("{value:#x} is not a valid codepoint"))
}

fn load_parsed_font(path: &Path, index: u32) -> anyhow::Result<ParsedFont> {
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let handle = FontDataHandle {
        source: FontDataSource::OnDisk(path),
        index,
        variation: 0,
        origin: FontOrigin::FontDirs,
        coverage: None,
    };
    ParsedFont::from_locator(&handle)
}

fn resolve_glyph_id(parsed: &ParsedFont, opt: &Opt) -> anyhow::Result<u32> {
    if let Some(id) = opt.glyph_id {
        return Ok(id);
    }
    let text = opt
        .codepoint
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("either --codepoint or --glyph-id must be specified"))?;
    let c = parse_codepoint(text)?;

    let font_info = SwashFontInfo::from_locator(&parsed.handle.source, parsed.handle.index)?;
    let glyph_id = font_info.glyph_id_for_char(c);
    if glyph_id == 0 {
        anyhow::bail!(
            "font {:?} has no glyph for codepoint {:?} (U+{:04X})",
            opt.font,
            c,
            c as u32
        );
    }
    log::info!(
        "codepoint {c:?} (U+{:04X}) -> glyph id {glyph_id}",
        c as u32
    );
    Ok(glyph_id as u32)
}

#[derive(serde::Serialize)]
struct GlyphMetadata {
    rasterizer: &'static str,
    font: String,
    glyph_id: u32,
    size: f64,
    dpi: u32,
    width: usize,
    height: usize,
    bearing_x: f64,
    bearing_y: f64,
    has_color: bool,
    is_scaled: bool,
}

fn dump_one(
    parsed: &ParsedFont,
    glyph_id: u32,
    opt: &Opt,
    selection: config::FontRasterizerSelection,
    out_png: &PathBuf,
    out_json: &PathBuf,
) -> anyhow::Result<()> {
    let rasterizer = new_rasterizer(selection, parsed)?;
    let glyph: RasterizedGlyph = rasterizer.rasterize_glyph(glyph_id, opt.size, opt.dpi)?;

    log::info!(
        "[{}] {}x{} bearing=({}, {}) has_color={} is_scaled={} bytes={}",
        selection_name(selection),
        glyph.width,
        glyph.height,
        glyph.bearing_x.get(),
        glyph.bearing_y.get(),
        glyph.has_color,
        glyph.is_scaled,
        glyph.data.len()
    );

    if glyph.width == 0 || glyph.height == 0 {
        anyhow::bail!(
            "[{}] rasterized glyph {} has zero size (empty ink extents) -- \
             is this the right glyph id/codepoint for this font?",
            selection_name(selection),
            glyph_id
        );
    }

    let expected_len = glyph.width * glyph.height * 4;
    if glyph.data.len() != expected_len {
        anyhow::bail!(
            "[{}] glyph data length {} does not match width*height*4={}",
            selection_name(selection),
            glyph.data.len(),
            expected_len
        );
    }

    let img: image::RgbaImage =
        image::ImageBuffer::from_raw(glyph.width as u32, glyph.height as u32, glyph.data.clone())
            .ok_or_else(|| anyhow::anyhow!("failed to build image buffer from glyph data"))?;

    if let Some(parent) = out_png.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    img.save(out_png)?;
    log::info!(
        "[{}] wrote {}",
        selection_name(selection),
        out_png.display()
    );

    let meta = GlyphMetadata {
        rasterizer: selection_name(selection),
        font: opt.font.display().to_string(),
        glyph_id,
        size: opt.size,
        dpi: opt.dpi,
        width: glyph.width,
        height: glyph.height,
        bearing_x: glyph.bearing_x.get(),
        bearing_y: glyph.bearing_y.get(),
        has_color: glyph.has_color,
        is_scaled: glyph.is_scaled,
    };
    let json = serde_json::to_string_pretty(&meta)?;
    std::fs::write(out_json, json)?;
    log::info!(
        "[{}] wrote {}",
        selection_name(selection),
        out_json.display()
    );

    Ok(())
}

fn with_extension_ensured(path: &Path, ext: &str) -> PathBuf {
    if path.extension().map(|e| e == ext).unwrap_or(false) {
        path.to_path_buf()
    } else {
        let mut p = path.to_path_buf();
        let file_name = p
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_default();
        p.set_file_name(format!("{file_name}.{ext}"));
        p
    }
}

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let opt = Opt::parse();

    if opt.codepoint.is_some() && opt.glyph_id.is_some() {
        anyhow::bail!("specify only one of --codepoint or --glyph-id, not both");
    }

    let parsed = load_parsed_font(&opt.font, opt.font_index)?;
    let glyph_id = resolve_glyph_id(&parsed, &opt)?;

    let selection = opt.rasterizer.selection();
    let out_png = with_extension_ensured(&opt.out, "png");
    let out_json = with_extension_ensured(&opt.out, "json");

    dump_one(&parsed, glyph_id, &opt, selection, &out_png, &out_json)?;

    Ok(())
}
