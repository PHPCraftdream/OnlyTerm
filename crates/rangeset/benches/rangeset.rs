use bench_scale_tool::Harness;
use rangeset::RangeSet;
use std::hint::black_box;

fn build_contig_rangeset(size: usize) -> RangeSet<usize> {
    let mut set = RangeSet::new();
    for i in 0..size {
        set.add(i);
    }
    set
}

fn build_sparse_rangeset(size: usize) -> RangeSet<usize> {
    let mut set = RangeSet::new();
    for i in (0..size).step_by(2) {
        set.add(i);
    }
    set
}

fn main() {
    let mut h = Harness::new("rangeset", env!("CARGO_MANIFEST_DIR"));

    h.bench("contig/100", || {
        black_box(build_contig_rangeset(100));
    });
    h.bench("contig/10000", || {
        black_box(build_contig_rangeset(10000));
    });
    h.bench("contig/1000000", || {
        black_box(build_contig_rangeset(1000000));
    });

    h.bench("sparse/100", || {
        black_box(build_sparse_rangeset(100));
    });
    h.bench("sparse/10000", || {
        black_box(build_sparse_rangeset(10000));
    });
    h.bench("sparse/1000000", || {
        black_box(build_sparse_rangeset(1000000));
    });

    h.run();
}
