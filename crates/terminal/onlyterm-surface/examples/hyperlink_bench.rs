//! Cost of the default implicit-hyperlink rules on lines with no link in
//! them, which is almost every line of a screen. Reports per rule and for
//! `Rule::match_hyperlinks` as a whole.
//!
//! ```text
//! cargo run --release -p onlyterm-surface --example hyperlink_bench
//! ```

use onlyterm_surface::hyperlink::{
    Rule, CLOSING_PARENTHESIS_HYPERLINK_PATTERN, GENERIC_HYPERLINK_PATTERN,
};
use std::time::Instant;

fn lines(count: usize) -> Vec<String> {
    (0..count)
        .map(|row| {
            format!(
                "{:03} frame {:05} status={} worker={:03} redraw payload {}{}",
                row,
                row * 7,
                if row % 2 == 0 { "busy" } else { "idle" },
                (row * 13) % 1000,
                "\u{754c}".repeat(8),
                "x".repeat(40)
            )
        })
        .collect()
}

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

fn main() {
    let rules = vec![
        (
            "(url)",
            Rule::with_highlight(r"\((\w+://\S+)\)", "$1", 1).unwrap(),
        ),
        (
            "[url]",
            Rule::with_highlight(r"\[(\w+://\S+)\]", "$1", 1).unwrap(),
        ),
        (
            "<url>",
            Rule::with_highlight(r"<(\w+://\S+)>", "$1", 1).unwrap(),
        ),
        (
            "closing-paren (lookahead)",
            Rule::new(CLOSING_PARENTHESIS_HYPERLINK_PATTERN, "$0").unwrap(),
        ),
        (
            "generic",
            Rule::new(GENERIC_HYPERLINK_PATTERN, "$0").unwrap(),
        ),
        (
            "mailto",
            Rule::new(r"\b\w+@[\w-]+(\.[\w-]+)+\b", "mailto:$0").unwrap(),
        ),
    ];
    let text = lines(40);
    println!("40 link-free lines of ~{} bytes", text[0].len());

    for (name, rule) in &rules {
        let us = median_us(300, || {
            for line in &text {
                for captures in rule.regex.captures_iter(line).flatten() {
                    std::hint::black_box(captures);
                }
            }
        });
        let is_match = median_us(300, || {
            for line in &text {
                std::hint::black_box(rule.regex.is_match(line).unwrap_or(true));
            }
        });
        println!(
            "  {:<28} captures_iter {:>8.1} us   is_match {:>8.1} us   (per line {:.2} us)",
            name,
            us,
            is_match,
            us / text.len() as f64
        );
    }

    let all: Vec<Rule> = rules.iter().map(|(_, r)| r.clone()).collect();
    let whole = median_us(300, || {
        for line in &text {
            std::hint::black_box(Rule::match_hyperlinks(line, &all));
        }
    });
    println!(
        "  {:<28} {:>8.1} us for 40 lines ({:.2} us per line)",
        "match_hyperlinks, all rules",
        whole,
        whole / text.len() as f64
    );

    // The same rules with the required-text hints the defaults now carry.
    let hinted: Vec<Rule> = all
        .iter()
        .enumerate()
        .map(|(index, rule)| {
            rule.clone()
                .requiring_text(if index == 5 { "@" } else { "://" })
        })
        .collect();
    let fast = median_us(300, || {
        for line in &text {
            std::hint::black_box(Rule::match_hyperlinks(line, &hinted));
        }
    });
    println!(
        "  {:<28} {:>8.1} us for 40 lines ({:.2} us per line)   {:.0}x faster",
        "same, with required text",
        fast,
        fast / text.len() as f64,
        whole / fast.max(1e-6)
    );

    // Lines that do contain a link still pay for the regex.
    let mut with_links = lines(40);
    for (row, line) in with_links.iter_mut().enumerate() {
        if row % 8 == 0 {
            line.push_str(" see https://example.com/page");
        }
    }
    let plain_links = median_us(300, || {
        for line in &with_links {
            std::hint::black_box(Rule::match_hyperlinks(line, &all));
        }
    });
    let hinted_links = median_us(300, || {
        for line in &with_links {
            std::hint::black_box(Rule::match_hyperlinks(line, &hinted));
        }
    });
    println!(
        "  {:<28} {:>8.1} us -> {:.1} us (1 line in 8 holds a URL)",
        "mixed screen", plain_links, hinted_links
    );
}
