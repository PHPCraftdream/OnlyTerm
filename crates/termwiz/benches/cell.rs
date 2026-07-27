use bench_scale_tool::Harness;
use std::hint::black_box;
use termwiz::cell::{Cell, CellAttributes};

fn main() {
    let mut h = Harness::new("cell", env!("CARGO_MANIFEST_DIR"));

    h.bench("cell/blank", || {
        black_box(Cell::blank());
    });
    h.bench("cell/new", || {
        black_box(Cell::new(black_box('a'), CellAttributes::default()));
    });
    h.bench("cell/new_grapheme", || {
        black_box(Cell::new_grapheme(
            black_box("a"),
            CellAttributes::default(),
            None,
        ));
    });
    h.bench("cell/new_grapheme_with_width", || {
        black_box(Cell::new_grapheme_with_width(
            black_box("a"),
            1,
            CellAttributes::default(),
        ));
    });

    h.bench("cell_attributes/blank", || {
        black_box(CellAttributes::blank());
    });

    h.run();
}
