//! Per-line costs the renderer pays for every row whose content changed:
//! splitting the row into clusters (with and without bidi), hashing it for
//! the shape cache, and turning it into a string.
//!
//! ```text
//! cargo run --release -p onlyterm-surface --features std --example line_bench
//! ```

use onlyterm_bidi::ParagraphDirectionHint;
use onlyterm_cell::CellAttributes;
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

fn main() {
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
