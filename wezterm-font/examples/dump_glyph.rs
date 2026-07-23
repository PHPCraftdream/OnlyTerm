//! Headless tool to rasterize a single glyph from a font file and dump it
//! to a PNG (plus a JSON metadata sidecar) for agent-driven, without-a-human
//! verification of glyph rendering (in particular COLR/COLRv1 color glyphs).
//!
//! This is part of the cairo -> tiny-skia migration effort for
//! `wezterm-font`'s color-glyph rasterizer: the same binary is used to
//! capture the "before" (cairo-based) baseline and the "after"
//! (tiny-skia-based) result, so that `diff_glyph` can compare them without
//! any human looking at a screen.
//!
//! Example:
//!
//! ```sh
//! cargo run -p wezterm-font --example dump_glyph -- \
//!     --font assets/fonts/NotoColorEmoji.ttf \
//!     --codepoint 1F600 \
//!     --size 32 --dpi 96 \
//!     --rasterizer both \
//!     --out /tmp/grinning
//! ```
//!
//! With `--rasterizer both`, output files are suffixed with the rasterizer
//! name: `/tmp/grinning-freetype.png` / `/tmp/grinning-freetype.json` and
//! `/tmp/grinning-harfbuzz.png` / `/tmp/grinning-harfbuzz.json`. With a
//! single rasterizer selected, the plain `--out` path (with `.png`/`.json`
//! appended if not already present) is used.
//!
//! `--rasterizer swash` (part of phase H3 of the freetype+harfbuzz ->
//! rustybuzz+swash migration, see
//! `docs/plans/2026-07-23-freetype-harfbuzz-migration.md`) exercises the
//! parallel, not-wired-up `wezterm_font::rasterizer::swash::SwashRasterizer`
//! instead of going through `new_rasterizer`/`FontRasterizerSelection`
//! (which doesn't know about it yet). This is the tool used to visually
//! compare swash's hinting/rasterization of plain (non-COLR) glyphs
//! against FreeType's, e.g.:
//!
//! ```sh
//! cargo run -p wezterm-font --example dump_glyph -- \
//!     --font assets/fonts/JetBrainsMono-Regular.ttf \
//!     --codepoint A --size 12 --dpi 96 \
//!     --rasterizer freetype-vs-swash \
//!     --out /tmp/cap-a
//! ```

use clap::{Parser, ValueEnum};
use rangeset::RangeSet;
use std::path::PathBuf;
use wezterm_font::ftwrap;
use wezterm_font::locator::{FontDataHandle, FontDataSource, FontOrigin};
use wezterm_font::parser::ParsedFont;
use wezterm_font::rasterizer::swash::SwashRasterizer;
use wezterm_font::rasterizer::{new_rasterizer, FontRasterizer, RasterizedGlyph};

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
enum RasterizerArg {
    Freetype,
    Harfbuzz,
    /// The parallel `swash::scale`-based rasterizer (H3, non-COLR glyphs
    /// only -- see the module doc comment on
    /// `wezterm_font::rasterizer::swash`).
    Swash,
    Both,
    /// FreeType and swash side by side (the pairing that actually matters
    /// for H3's hinting-parity verification; `Both` above is the
    /// pre-existing FreeType-vs-harfbuzz-paint-path pairing from the
    /// cairo->tiny-skia initiative, which is a different comparison).
    FreetypeVsSwash,
}

/// The non-`config::FontRasterizerSelection` case (`Swash`) is handled
/// separately in `main`/`dump_one_swash`, since `SwashRasterizer` isn't a
/// `FontRasterizerSelection` variant yet (H3 is explicitly a parallel,
/// not-wired-up implementation).
impl RasterizerArg {
    fn selections(self) -> Vec<config::FontRasterizerSelection> {
        match self {
            RasterizerArg::Freetype => vec![config::FontRasterizerSelection::FreeType],
            RasterizerArg::Harfbuzz => vec![config::FontRasterizerSelection::Harfbuzz],
            RasterizerArg::Swash => vec![],
            RasterizerArg::Both => vec![
                config::FontRasterizerSelection::FreeType,
                config::FontRasterizerSelection::Harfbuzz,
            ],
            RasterizerArg::FreetypeVsSwash => vec![config::FontRasterizerSelection::FreeType],
        }
    }

    fn wants_swash(self) -> bool {
        matches!(self, RasterizerArg::Swash | RasterizerArg::FreetypeVsSwash)
    }
}

fn selection_name(sel: config::FontRasterizerSelection) -> &'static str {
    match sel {
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

    /// Which rasterizer implementation(s) to exercise.
    #[arg(long, value_enum, default_value_t = RasterizerArg::Both)]
    rasterizer: RasterizerArg,

    /// Output path (without extension when used with --rasterizer both,
    /// where it is used as a shared prefix). A ".png" is written, plus a
    /// ".json" metadata sidecar.
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

fn load_parsed_font(path: &PathBuf, index: u32) -> anyhow::Result<ParsedFont> {
    let path = path.canonicalize().unwrap_or_else(|_| path.clone());
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

    let lib = ftwrap::Library::new()?;
    let face = lib.face_from_locator(&parsed.handle)?;
    let glyph_id = unsafe { ftwrap::FT_Get_Char_Index(face.face, c as _) };
    if glyph_id == 0 {
        anyhow::bail!(
            "font {:?} has no glyph for codepoint {:?} (U+{:04X})",
            opt.font,
            c,
            c as u32
        );
    }
    log::info!("codepoint {c:?} (U+{:04X}) -> glyph id {glyph_id}", c as u32);
    Ok(glyph_id)
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
    let rasterizer = new_rasterizer(selection, parsed, config::DisplayPixelGeometry::RGB)?;
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
    log::info!("[{}] wrote {}", selection_name(selection), out_png.display());

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
    log::info!("[{}] wrote {}", selection_name(selection), out_json.display());

    Ok(())
}

/// Like `dump_one`, but for the `swash`-based rasterizer, which isn't a
/// `config::FontRasterizerSelection` variant and so can't go through
/// `new_rasterizer`.
fn dump_one_swash(
    parsed: &ParsedFont,
    glyph_id: u32,
    opt: &Opt,
    out_png: &PathBuf,
    out_json: &PathBuf,
) -> anyhow::Result<()> {
    let rasterizer = SwashRasterizer::from_locator(parsed)?;
    let glyph: RasterizedGlyph = rasterizer.rasterize_glyph(glyph_id, opt.size, opt.dpi)?;

    log::info!(
        "[swash] {}x{} bearing=({}, {}) has_color={} is_scaled={} bytes={}",
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
            "[swash] rasterized glyph {} has zero size (empty ink extents) -- \
             is this the right glyph id/codepoint for this font?",
            glyph_id
        );
    }

    let expected_len = glyph.width * glyph.height * 4;
    if glyph.data.len() != expected_len {
        anyhow::bail!(
            "[swash] glyph data length {} does not match width*height*4={}",
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
    log::info!("[swash] wrote {}", out_png.display());

    let meta = GlyphMetadata {
        rasterizer: "swash",
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
    log::info!("[swash] wrote {}", out_json.display());

    Ok(())
}

fn with_extension_ensured(path: &PathBuf, ext: &str) -> PathBuf {
    if path.extension().map(|e| e == ext).unwrap_or(false) {
        path.clone()
    } else {
        let mut p = path.clone();
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

    // Ensure a coverage set is not required; RangeSet import kept for
    // documentation/parity with FontDataHandle's `coverage` field type.
    let _unused: Option<RangeSet<u32>> = None;

    let parsed = load_parsed_font(&opt.font, opt.font_index)?;
    let glyph_id = resolve_glyph_id(&parsed, &opt)?;

    let selections = opt.rasterizer.selections();
    let wants_swash = opt.rasterizer.wants_swash();
    let multi = selections.len() + if wants_swash { 1 } else { 0 } > 1;

    let paths_for = |name: &str| -> (PathBuf, PathBuf) {
        if multi {
            let stem = opt
                .out
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "glyph".to_string());
            let mut png = opt.out.clone();
            png.set_file_name(format!("{stem}-{name}.png"));
            let mut json = opt.out.clone();
            json.set_file_name(format!("{stem}-{name}.json"));
            (png, json)
        } else {
            (
                with_extension_ensured(&opt.out, "png"),
                with_extension_ensured(&opt.out, "json"),
            )
        }
    };

    for selection in selections {
        let (out_png, out_json) = paths_for(selection_name(selection));
        dump_one(&parsed, glyph_id, &opt, selection, &out_png, &out_json)?;
    }

    if wants_swash {
        let (out_png, out_json) = paths_for("swash");
        dump_one_swash(&parsed, glyph_id, &opt, &out_png, &out_json)?;
    }

    Ok(())
}
