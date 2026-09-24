use bench_scale_tool::Harness;
use std::hint::black_box;
use std::rc::Rc;
use termwiz::cell::{grapheme_column_width, UnicodeVersion};

include!("../src/widechar_width.rs");

// `include!` above inlines widechar_width.rs, whose trailing `#[cfg(test)] mod
// test` precedes this `main`; main cannot precede the include because it uses
// the included items. Suppress the resulting items-after-test-module lint.
#[allow(clippy::items_after_test_module)]
fn main() {
    let table = Rc::new(WcLookupTable::new());

    let mut h = Harness::new("wcwidth", env!("CARGO_MANIFEST_DIR"));

    h.bench("classify_ascii/wcwidth", || {
        black_box(WcWidth::from_char(black_box('a')));
    });
    {
        let table = Rc::clone(&table);
        h.bench("classify_ascii/lookup_table", move || {
            black_box(table.classify(black_box('a')));
        });
    }

    h.bench("classify_double_width/wcwidth", || {
        black_box(WcWidth::from_char(black_box('\u{1100}')));
    });
    {
        let table = Rc::clone(&table);
        h.bench("classify_double_width/lookup_table", move || {
            black_box(table.classify(black_box('\u{1100}')));
        });
    }

    h.bench("classify_widened_in9/wcwidth", || {
        black_box(WcWidth::from_char(black_box('\u{231a}')));
    });
    {
        let table = Rc::clone(&table);
        h.bench("classify_widened_in9/lookup_table", move || {
            black_box(table.classify(black_box('\u{231a}')));
        });
    }

    h.bench("classify_unassigned/wcwidth", || {
        black_box(WcWidth::from_char(black_box('\u{fbc9}')));
    });
    {
        let table = Rc::clone(&table);
        h.bench("classify_unassigned/lookup_table", move || {
            black_box(table.classify(black_box('\u{fbc9}')));
        });
    }

    h.bench("column_width_ascii/grapheme_column_width", || {
        black_box(grapheme_column_width(black_box("a"), None));
    });

    h.bench(
        "column_width_variation_selector/grapheme_column_width",
        || {
            black_box(grapheme_column_width(black_box("\u{00a9}\u{FE0F}"), None));
        },
    );

    h.bench(
        "column_width_variation_selector_unicode14/grapheme_column_width",
        || {
            let version = UnicodeVersion {
                version: 14,
                ambiguous_are_wide: false,
                cell_widths: None,
            };
            black_box(grapheme_column_width(
                black_box("\u{00a9}\u{FE0F}"),
                Some(&version),
            ));
        },
    );

    h.bench("column_width_widened_in9/grapheme_column_width", || {
        black_box(grapheme_column_width(black_box("\u{231a}"), None));
    });

    h.bench("column_width_unassigned/grapheme_column_width", || {
        black_box(grapheme_column_width(black_box("\u{fbc9}"), None));
    });

    h.run();
}
