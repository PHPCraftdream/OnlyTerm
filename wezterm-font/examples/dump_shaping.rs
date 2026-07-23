//! H0 tooling for the freetype+harfbuzz -> rustybuzz+swash migration
//! (see docs/plans/2026-07-23-freetype-harfbuzz-migration.md).
//!
//! This is a headless CLI that shapes a text string through:
//!   1. The current production shaping path: `wezterm_font::shaper::harfbuzz::HarfbuzzShaper`
//!      (unmodified; this is the existing `FontShaper` trait impl used by wezterm today).
//!   2. Raw `rustybuzz`, called directly (NOT integrated into the `FontShaper` trait -
//!      that integration is future work, tracked as H1).
//!
//! For each engine it prints (or saves to a JSON file) an array of per-glyph
//! records: `{glyph_id, x_advance, y_advance, x_offset, y_offset, cluster}`.
//!
//! This lets us compare shaping (which glyphs + which advances/positions/cluster
//! boundaries come out of a given input string) independently of rasterization,
//! which is already covered by the separate dump_glyph/diff_glyph tooling.
//!
//! Usage:
//!   cargo run -p wezterm-font --example dump_shaping -- \
//!       --font-file assets/fonts/JetBrainsMono-Regular.ttf \
//!       --text "Hello World" --size 12 --dpi 96
//!
//!   # Save both engines' output to JSON files instead of printing:
//!   cargo run -p wezterm-font --example dump_shaping -- \
//!       --font-file FONT --text TEXT --harfbuzz-out hb.json --rustybuzz-out rb.json
//!
//! Comparison mode (see also `--compare`):
//!   cargo run -p wezterm-font --example dump_shaping -- \
//!       --font-file FONT --text TEXT --compare
//!
//! which shapes with both engines and reports a diff with a non-zero exit
//! code if the glyph sequences disagree.
use serde::Serialize;
use std::path::PathBuf;
use wezterm_font::locator::{FontDataHandle, FontDataSource, FontOrigin};
use wezterm_font::parser::ParsedFont;
use wezterm_font::shaper::harfbuzz::HarfbuzzShaper;
use wezterm_font::shaper::{Direction as WeztermDirection, FontShaper};

#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
struct ShapedGlyph {
    glyph_id: u32,
    x_advance: f64,
    y_advance: f64,
    x_offset: f64,
    y_offset: f64,
    cluster: u32,
}

struct Args {
    font_file: PathBuf,
    face_index: u32,
    text: String,
    size: f64,
    dpi: u32,
    harfbuzz_out: Option<PathBuf>,
    rustybuzz_out: Option<PathBuf>,
    compare: bool,
    quiet: bool,
}

const HELP: &str = "\
dump_shaping - shape a text string with the production HarfbuzzShaper and
with raw rustybuzz, and print/save/compare the resulting glyph sequences.

USAGE:
    dump_shaping --font-file PATH --text TEXT [OPTIONS]

OPTIONS:
    --font-file PATH        Path to a font file (ttf/otf/ttc) [required]
    --face-index N          Face index within the font file [default: 0]
    --text TEXT             Text to shape (UTF-8, passed directly)          [required]
    --size PT               Font size in points                            [default: 12]
    --dpi DPI               DPI to shape at                                 [default: 96]
    --harfbuzz-out PATH     Save the HarfbuzzShaper result as JSON to PATH
    --rustybuzz-out PATH    Save the raw rustybuzz result as JSON to PATH
    --compare               Shape with both engines and diff the results;
                            exits non-zero if they disagree
    --quiet                 Suppress the pretty-printed JSON dump to stdout
                            (still writes --*-out files / runs --compare)
    -h, --help              Show this help
";

fn parse_args() -> anyhow::Result<Args> {
    let mut font_file = None;
    let mut face_index = 0u32;
    let mut text = None;
    let mut size = 12.0f64;
    let mut dpi = 96u32;
    let mut harfbuzz_out = None;
    let mut rustybuzz_out = None;
    let mut compare = false;
    let mut quiet = false;

    let mut argv = std::env::args().skip(1);
    while let Some(arg) = argv.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print!("{}", HELP);
                std::process::exit(0);
            }
            "--font-file" => {
                font_file = Some(PathBuf::from(
                    argv.next().ok_or_else(|| anyhow::anyhow!("--font-file needs a value"))?,
                ));
            }
            "--face-index" => {
                face_index = argv
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("--face-index needs a value"))?
                    .parse()?;
            }
            "--text" => {
                text = Some(argv.next().ok_or_else(|| anyhow::anyhow!("--text needs a value"))?);
            }
            "--size" => {
                size = argv
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("--size needs a value"))?
                    .parse()?;
            }
            "--dpi" => {
                dpi = argv
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("--dpi needs a value"))?
                    .parse()?;
            }
            "--harfbuzz-out" => {
                harfbuzz_out = Some(PathBuf::from(
                    argv.next()
                        .ok_or_else(|| anyhow::anyhow!("--harfbuzz-out needs a value"))?,
                ));
            }
            "--rustybuzz-out" => {
                rustybuzz_out = Some(PathBuf::from(
                    argv.next()
                        .ok_or_else(|| anyhow::anyhow!("--rustybuzz-out needs a value"))?,
                ));
            }
            "--compare" => compare = true,
            "--quiet" => quiet = true,
            other => anyhow::bail!("unrecognized argument: {other}\n\n{HELP}"),
        }
    }

    Ok(Args {
        font_file: font_file.ok_or_else(|| anyhow::anyhow!("--font-file is required\n\n{HELP}"))?,
        face_index,
        text: text.ok_or_else(|| anyhow::anyhow!("--text is required\n\n{HELP}"))?,
        size,
        dpi,
        harfbuzz_out,
        rustybuzz_out,
        compare,
        quiet,
    })
}

/// Shape `text` through the current production `HarfbuzzShaper` (via the
/// `FontShaper` trait, unmodified) and return the flattened glyph sequence.
fn shape_with_harfbuzz(
    font_file: &PathBuf,
    face_index: u32,
    text: &str,
    size: f64,
    dpi: u32,
) -> anyhow::Result<Vec<ShapedGlyph>> {
    let handle = FontDataHandle {
        source: FontDataSource::OnDisk(font_file.clone()),
        index: face_index,
        variation: 0,
        origin: FontOrigin::FontDirs,
        coverage: None,
    };
    let parsed = ParsedFont::from_locator(&handle)?;
    let config = config::configuration();
    let shaper = HarfbuzzShaper::new(&config, &[parsed])?;

    let mut no_glyphs = vec![];
    let infos = shaper.shape(
        text,
        size,
        dpi,
        &mut no_glyphs,
        None,
        WeztermDirection::LeftToRight,
        None,
        None,
    )?;
    if !no_glyphs.is_empty() {
        log::warn!(
            "harfbuzz shaper reported {} unresolved codepoints: {:?}",
            no_glyphs.len(),
            no_glyphs
        );
    }

    Ok(infos
        .into_iter()
        .map(|g| ShapedGlyph {
            glyph_id: g.glyph_pos,
            x_advance: g.x_advance.get(),
            y_advance: g.y_advance.get(),
            x_offset: g.x_offset.get(),
            y_offset: g.y_offset.get(),
            cluster: g.cluster,
        })
        .collect())
}

/// Shape `text` through raw `rustybuzz`, with no integration into wezterm's
/// `FontShaper` trait (that's future work, H1). This mirrors, as closely as
/// reasonably possible, the buffer setup that `HarfbuzzShaper::do_shape` uses
/// (LTR direction, "en" language, monotone-graphemes cluster level, and no
/// explicit script so it is guessed from the buffer contents).
///
/// Important API difference from the `hbwrap.rs` C API: rustybuzz does not
/// have an equivalent of `hb_font_set_scale`/`hb_ft_font_set_load_flags` that
/// pre-scales advances/positions into 26.6 fixed-point pixel units. Instead,
/// rustybuzz always returns raw font design units (i.e. in the font's
/// `unitsPerEm` space), and the caller is responsible for scaling them to
/// pixels. We do that scaling here so the output is directly comparable to
/// the harfbuzz side (which already reports pixel-space advances).
fn shape_with_rustybuzz(
    font_file: &PathBuf,
    face_index: u32,
    text: &str,
    size: f64,
    dpi: u32,
) -> anyhow::Result<Vec<ShapedGlyph>> {
    let font_data = std::fs::read(font_file)?;
    let face = rustybuzz::Face::from_slice(&font_data, face_index)
        .ok_or_else(|| anyhow::anyhow!("rustybuzz failed to parse {font_file:?}"))?;

    let units_per_em = face.units_per_em() as f64;
    // Same pixel-size convention as HarfbuzzShaper/ftwrap: points -> pixels at DPI.
    let pixel_size = size * dpi as f64 / 72.0;
    let scale = pixel_size / units_per_em;

    let mut buffer = rustybuzz::UnicodeBuffer::new();
    buffer.push_str(text);
    buffer.set_direction(rustybuzz::Direction::LeftToRight);
    buffer.set_language("en".parse().expect("valid language"));
    buffer.set_cluster_level(rustybuzz::BufferClusterLevel::MonotoneGraphemes);
    // Deliberately do not set the script; let rustybuzz guess it from the
    // buffer contents (same reasoning as HarfbuzzShaper, see the comment in
    // wezterm-font/src/shaper/harfbuzz.rs referencing #1474/#1573).

    let glyph_buffer = rustybuzz::shape(&face, &[], buffer);

    let infos = glyph_buffer.glyph_infos();
    let positions = glyph_buffer.glyph_positions();

    Ok(infos
        .iter()
        .zip(positions.iter())
        .map(|(info, pos)| ShapedGlyph {
            glyph_id: info.glyph_id,
            x_advance: pos.x_advance as f64 * scale,
            y_advance: pos.y_advance as f64 * scale,
            x_offset: pos.x_offset as f64 * scale,
            y_offset: pos.y_offset as f64 * scale,
            cluster: info.cluster,
        })
        .collect())
}

fn print_or_save(label: &str, glyphs: &[ShapedGlyph], out: Option<&PathBuf>, quiet: bool) -> anyhow::Result<()> {
    let json = serde_json::to_string_pretty(glyphs)?;
    if let Some(path) = out {
        std::fs::write(path, &json)?;
        eprintln!("{label}: wrote {} glyph(s) to {}", glyphs.len(), path.display());
    }
    if !quiet {
        println!("=== {label} ({} glyph(s)) ===", glyphs.len());
        println!("{json}");
    }
    Ok(())
}

/// Compare two shaped glyph sequences field-by-field and report every
/// divergence. Returns `true` if they matched exactly.
fn compare_glyphs(harfbuzz: &[ShapedGlyph], rustybuzz: &[ShapedGlyph]) -> bool {
    let mut ok = true;

    if harfbuzz.len() != rustybuzz.len() {
        println!(
            "MISMATCH: glyph count differs: harfbuzz={} rustybuzz={}",
            harfbuzz.len(),
            rustybuzz.len()
        );
        ok = false;
    }

    let n = harfbuzz.len().min(rustybuzz.len());
    // Allow tiny floating point slop from the pixel-scaling multiply; real
    // fixed-point-vs-float rounding differences of a small fraction of a
    // pixel are not meaningful shaping divergences.
    const EPS: f64 = 0.01;

    for i in 0..n {
        let hb = &harfbuzz[i];
        let rb = &rustybuzz[i];
        let mut diffs = vec![];

        if hb.glyph_id != rb.glyph_id {
            diffs.push(format!("glyph_id: {} != {}", hb.glyph_id, rb.glyph_id));
        }
        if hb.cluster != rb.cluster {
            diffs.push(format!("cluster: {} != {}", hb.cluster, rb.cluster));
        }
        if (hb.x_advance - rb.x_advance).abs() > EPS {
            diffs.push(format!("x_advance: {} != {}", hb.x_advance, rb.x_advance));
        }
        if (hb.y_advance - rb.y_advance).abs() > EPS {
            diffs.push(format!("y_advance: {} != {}", hb.y_advance, rb.y_advance));
        }
        if (hb.x_offset - rb.x_offset).abs() > EPS {
            diffs.push(format!("x_offset: {} != {}", hb.x_offset, rb.x_offset));
        }
        if (hb.y_offset - rb.y_offset).abs() > EPS {
            diffs.push(format!("y_offset: {} != {}", hb.y_offset, rb.y_offset));
        }

        if !diffs.is_empty() {
            ok = false;
            println!("MISMATCH at glyph index {i}:");
            println!("    harfbuzz:  {hb:?}");
            println!("    rustybuzz: {rb:?}");
            for d in diffs {
                println!("    -> {d}");
            }
        }
    }

    if n < harfbuzz.len() {
        println!("harfbuzz has {} extra trailing glyph(s):", harfbuzz.len() - n);
        for g in &harfbuzz[n..] {
            println!("    {g:?}");
        }
    }
    if n < rustybuzz.len() {
        println!("rustybuzz has {} extra trailing glyph(s):", rustybuzz.len() - n);
        for g in &rustybuzz[n..] {
            println!("    {g:?}");
        }
    }

    ok
}

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_default_env().init();
    let args = parse_args()?;

    let harfbuzz_glyphs =
        shape_with_harfbuzz(&args.font_file, args.face_index, &args.text, args.size, args.dpi)?;
    let rustybuzz_glyphs =
        shape_with_rustybuzz(&args.font_file, args.face_index, &args.text, args.size, args.dpi)?;

    print_or_save("harfbuzz", &harfbuzz_glyphs, args.harfbuzz_out.as_ref(), args.quiet)?;
    print_or_save("rustybuzz", &rustybuzz_glyphs, args.rustybuzz_out.as_ref(), args.quiet)?;

    if args.compare {
        let matched = compare_glyphs(&harfbuzz_glyphs, &rustybuzz_glyphs);
        if matched {
            println!("OK: harfbuzz and rustybuzz shaping results match for {:?}", args.text);
            Ok(())
        } else {
            anyhow::bail!(
                "shaping mismatch between harfbuzz and rustybuzz for {:?} (font={:?}, size={}, dpi={})",
                args.text,
                args.font_file,
                args.size,
                args.dpi
            );
        }
    } else {
        Ok(())
    }
}
