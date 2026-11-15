//! Per-line costs the renderer pays for every row whose content changed:
//! splitting the row into clusters (with and without bidi), hashing it for
//! the shape cache, and turning it into a string; plus blanking a scrolled-off
//! line when the pen has a colour.
//!
//! ```text
//! cargo run --release -p onlyterm-surface --features std --example line_bench
//! ```

use onlyterm_bidi::ParagraphDirectionHint;
use onlyterm_cell::color::ColorAttribute;
use onlyterm_cell::{Cell, CellAttributes};
use onlyterm_surface::{Line, SEQ_ZERO};
use std::time::Instant;

fn median_us(iterations: usize, mut f: impl FnMut()) -> f64 {
    f();
    let mut samples: Vec<f64> = (0..iterations)
        .map(|_| {
            let start = Instant::now();
            f();
            start.elapsed().as_secs_f64() * 1e6
        })
        .collect();
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    samples[samples.len() / 2]
}

fn bench(name: &str, text: &str) {
    let lines: Vec<Line> = (0..40)
        .map(|row| {
            Line::from_text(
                &format!("{:03} {}", row, text),
                &CellAttributes::default(),
                SEQ_ZERO,
                None,
            )
        })
        .collect();
    let cells: usize = lines.iter().map(|line| line.len()).sum();
    let clusters: usize = lines.iter().map(|line| line.cluster(None).len()).sum();

    let no_bidi = median_us(300, || {
        for line in &lines {
            std::hint::black_box(line.cluster(None));
        }
    });
    let bidi = median_us(300, || {
        for line in &lines {
            std::hint::black_box(line.cluster(Some(ParagraphDirectionHint::LeftToRight)));
        }
    });
    let hash = median_us(300, || {
        for line in &lines {
            std::hint::black_box(line.compute_shape_hash());
        }
    });
    let as_str = median_us(300, || {
        for line in &lines {
            std::hint::black_box(line.as_str());
        }
    });
    println!(
        "{} (40 lines, {} cells, {} clusters)",
        name, cells, clusters
    );
    for (what, us) in [
        ("cluster, bidi off", no_bidi),
        ("cluster, bidi on (default)", bidi),
        ("compute_shape_hash", hash),
        ("as_str", as_str),
    ] {
        println!(
            "  {:<28} {:>8.1} us   {:>6.2} us/line",
            what,
            us,
            us / lines.len() as f64
        );
    }
}

/// `Screen::scroll_up` blanks the line that scrolled off the top with
/// `resize_and_clear` when the pen is not the default one. Compares it with
/// expanding the line and overwriting every cell (what it used to do),
/// alternating the two so that load on the machine hits both equally.
fn bench_resize_and_clear(width: usize, clustered: bool) {
    const LINES: usize = 40;
    let mut pen = CellAttributes::default();
    pen.set_foreground(ColorAttribute::PaletteIndex(15));
    pen.set_background(ColorAttribute::PaletteIndex(0));
    let text: String = "status=busy worker=042 "
        .chars()
        .cycle()
        .take(width)
        .collect();
    let fresh = || -> Vec<Line> {
        (0..LINES)
            .map(|_| {
                let mut line = Line::from_text(&text, &CellAttributes::default(), SEQ_ZERO, None);
                if clustered {
                    line.compress_for_scrollback();
                }
                line
            })
            .collect()
    };

    let (mut old, mut new) = (Vec::new(), Vec::new());
    for round in 0..600 {
        let mut lines = fresh();
        let start = Instant::now();
        if round % 2 == 0 {
            for line in &mut lines {
                for cell in line.cells_mut() {
                    *cell = Cell::blank_with_attrs(pen.clone());
                }
            }
            old.push(start.elapsed().as_secs_f64() * 1e6);
        } else {
            for line in &mut lines {
                line.resize_and_clear(width, SEQ_ZERO, pen.clone());
            }
            new.push(start.elapsed().as_secs_f64() * 1e6);
        }
        std::hint::black_box(&lines);
    }
    let stat = |v: &mut Vec<f64>| {
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        (v[0] / LINES as f64, v[v.len() / 2] / LINES as f64)
    };
    let (old_min, old_med) = stat(&mut old);
    let (new_min, new_med) = stat(&mut new);
    println!(
        "blank a scrolled-off line, coloured pen, {} columns, {} storage",
        width,
        if clustered {
            "clustered"
        } else {
            "cell vector"
        }
    );
    println!(
        "  expand + overwrite (old)     min {:>6.2} us/line   median {:>6.2} us/line",
        old_min, old_med
    );
    println!(
        "  resize_and_clear (new)       min {:>6.2} us/line   median {:>6.2} us/line",
        new_min, new_med
    );
}

/// A line kept as cells (a coloured pen blanks the row at the bottom, and
/// cursor addressing edits it) is packed into clusters when it scrolls off the
/// top of the screen.
fn bench_compress_for_scrollback(name: &str, build: impl Fn() -> Line) {
    const LINES: usize = 40;
    let mut samples: Vec<f64> = (0..400)
        .map(|_| {
            let mut lines: Vec<Line> = (0..LINES).map(|_| build()).collect();
            let start = Instant::now();
            for line in &mut lines {
                line.compress_for_scrollback();
            }
            let us = start.elapsed().as_secs_f64() * 1e6 / LINES as f64;
            std::hint::black_box(&lines);
            us
        })
        .collect();
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    println!(
        "compress_for_scrollback, {}: min {:.2} us/line   median {:.2} us/line",
        name,
        samples[0],
        samples[samples.len() / 2]
    );
}

fn main() {
    let mut pen = CellAttributes::default();
    pen.set_foreground(ColorAttribute::PaletteIndex(15));
    pen.set_background(ColorAttribute::PaletteIndex(0));
    let text = "The quick brown fox jumps over the lazy dog 0123456789 ";
    bench_compress_for_scrollback("120 cells, one pen", || {
        let mut line =
            Line::with_width_and_cell(120, Cell::blank_with_attrs(pen.clone()), SEQ_ZERO);
        line.overlay_text_with_attribute(0, text, pen.clone(), SEQ_ZERO);
        line
    });
    let mut red = CellAttributes::default();
    red.set_foreground(ColorAttribute::PaletteIndex(1));
    bench_compress_for_scrollback("120 cells, 8 colour runs", || {
        let mut line = Line::with_width_and_cell(120, Cell::blank(), SEQ_ZERO);
        for (i, word) in text.split(' ').take(8).enumerate() {
            let attrs = if i % 2 == 0 { pen.clone() } else { red.clone() };
            line.overlay_text_with_attribute(i * 7, word, attrs, SEQ_ZERO);
        }
        line
    });
    for clustered in [true, false] {
        for width in [80, 120, 200] {
            bench_resize_and_clear(width, clustered);
        }
    }
    bench(
        "ASCII words",
        "status=busy | worker=042 | redraw payload with several short words here",
    );
    bench(
        "Cyrillic words",
        "\u{441}\u{442}\u{430}\u{442}\u{443}\u{441} \u{433}\u{43e}\u{442}\u{43e}\u{432} \u{440}\u{430}\u{431}\u{43e}\u{447}\u{438}\u{439} \u{43f}\u{43e}\u{442}\u{43e}\u{43a} \u{43f}\u{435}\u{440}\u{435}\u{440}\u{438}\u{441}\u{43e}\u{432}\u{43a}\u{430} \u{441}\u{442}\u{440}\u{43e}\u{43a}\u{438} \u{442}\u{435}\u{43a}\u{441}\u{442}\u{430}",
    );
    bench(
        "CJK",
        "\u{754c}\u{6f22}\u{5b57}\u{65e5}\u{672c}\u{8a9e} \u{754c}\u{6f22}\u{5b57}\u{65e5}\u{672c}\u{8a9e} \u{754c}\u{6f22}\u{5b57}",
    );
    bench(
        "Hebrew phrase in ASCII",
        "open (\u{5e9}\u{5dc}\u{5d5}\u{5dd} \u{5e2}\u{5d5}\u{5dc}\u{5dd}) and close the rest of the line here",
    );
}
