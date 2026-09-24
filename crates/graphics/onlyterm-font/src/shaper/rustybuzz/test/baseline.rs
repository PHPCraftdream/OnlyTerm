use super::*;

/// One shaped glyph's regression-relevant fields, used by
/// `assert_shape_matches_baseline` below.
#[derive(Debug, Clone, Copy, PartialEq)]
struct GlyphBaseline {
    glyph_pos: u32,
    cluster: u32,
    x_advance: f64,
}

/// Shapes `text` with `RustybuzzShaper` (size=10, dpi=72, JetBrains
/// Mono) and asserts the result matches a hardcoded baseline exactly
/// for `glyph_pos`/`cluster`, and within `eps` pixels for `x_advance`
/// (not bit-exact against the baseline capture, to tolerate
/// float-rounding jitter across platforms/toolchains rather than
/// requiring the environment that captured the baseline).
///
/// This replaces a former harfbuzz-vs-rustybuzz parity comparison
/// (see the module doc comment on the H0-established guarantee): now
/// that the `harfbuzz` crate/`HarfbuzzShaper` have been removed
/// (phase H4), there is no live oracle to compare against, so this
/// instead pins down the current `RustybuzzShaper` output as a
/// regression baseline (captured by actually running the shaper, not
/// guessed) -- it will still catch a shaping regression from a
/// rustybuzz/ttf-parser upgrade or a refactor of `do_shape`, just not
/// a *divergence from harfbuzz* (which H0/H1 already established was
/// zero for glyph_id/cluster, and small/tolerance-bounded for
/// x_advance, before this crate was removed).
fn assert_shape_matches_baseline(text: &str, eps: f64, expected: &[GlyphBaseline]) {
    let config = onlyterm_config::configuration();
    let handle = jetbrains_handle();
    let rb_shaper = RustybuzzShaper::new(&config, &[handle]).unwrap();

    let mut no_glyphs = vec![];
    let info = rb_shaper
        .shape(
            text,
            10.,
            72,
            &mut no_glyphs,
            None,
            Direction::LeftToRight,
            None,
            None,
        )
        .unwrap();
    assert!(no_glyphs.is_empty(), "{:?}", no_glyphs);

    assert_eq!(
        expected.len(),
        info.len(),
        "glyph count mismatch for {text:?}: expected={expected:#?} actual={info:#?}"
    );

    for (want, got) in expected.iter().zip(info.iter()) {
        assert_eq!(
            want.glyph_pos, got.glyph_pos,
            "glyph_id mismatch for {text:?}: want={want:?} got={got:?}"
        );
        assert_eq!(
            want.cluster, got.cluster,
            "cluster mismatch for {text:?}: want={want:?} got={got:?}"
        );
        assert!(
            (want.x_advance - got.x_advance.get()).abs() <= eps,
            "x_advance mismatch beyond eps={eps} for {text:?}: want={want:?} got={got:?}"
        );
    }
}

#[test]
fn parity_simple_latin() {
    let _ = env_logger::Builder::new()
        .is_test(true)
        .filter_level(log::LevelFilter::Trace)
        .try_init();
    // Baselines captured from a real `RustybuzzShaper::shape` run
    // against JetBrainsMono-Regular.ttf at size=10, dpi=72 (see
    // `assert_shape_matches_baseline`'s doc comment for why these are
    // hardcoded rather than compared live against harfbuzz).
    assert_shape_matches_baseline(
        "abc",
        1.0,
        &[
            GlyphBaseline {
                glyph_pos: 189,
                cluster: 0,
                x_advance: 6.0,
            },
            GlyphBaseline {
                glyph_pos: 214,
                cluster: 1,
                x_advance: 6.0,
            },
            GlyphBaseline {
                glyph_pos: 215,
                cluster: 2,
                x_advance: 6.0,
            },
        ],
    );
    assert_shape_matches_baseline(
        "x x",
        1.0,
        &[
            GlyphBaseline {
                glyph_pos: 367,
                cluster: 0,
                x_advance: 6.0,
            },
            GlyphBaseline {
                glyph_pos: 958,
                cluster: 1,
                x_advance: 6.0,
            },
            GlyphBaseline {
                glyph_pos: 367,
                cluster: 2,
                x_advance: 6.0,
            },
        ],
    );
    assert_shape_matches_baseline(
        "x\u{3000}x",
        1.0,
        &[
            GlyphBaseline {
                glyph_pos: 367,
                cluster: 0,
                x_advance: 6.0,
            },
            GlyphBaseline {
                glyph_pos: 958,
                cluster: 1,
                x_advance: 10.0,
            },
            GlyphBaseline {
                glyph_pos: 367,
                cluster: 4,
                x_advance: 6.0,
            },
        ],
    );
}

#[test]
fn parity_ligatures() {
    let _ = env_logger::Builder::new()
        .is_test(true)
        .filter_level(log::LevelFilter::Trace)
        .try_init();
    // JetBrains Mono applies contextual (`calt`) substitution to
    // `<-`/`<--` (each character gets a different glyph id than its
    // standalone form, e.g. `<`'s glyph_pos changes from 1052 to
    // 1742 once followed by `-`), exercising the same
    // feature-driven substitution path a former `HarfbuzzShaper`
    // comparison test covered (see `assert_shape_matches_baseline`'s
    // doc comment) -- note this does not collapse into a single
    // merged glyph per sequence at this size/config (each character
    // keeps its own glyph and cluster), so the baselines below have
    // one entry per input character, not one per ligated sequence.
    // Baselines captured from a real shaper run, same as
    // `parity_simple_latin`.
    assert_shape_matches_baseline(
        "<",
        1.0,
        &[GlyphBaseline {
            glyph_pos: 1052,
            cluster: 0,
            x_advance: 6.0,
        }],
    );
    assert_shape_matches_baseline(
        "<-",
        1.0,
        &[
            GlyphBaseline {
                glyph_pos: 1742,
                cluster: 0,
                x_advance: 6.0,
            },
            GlyphBaseline {
                glyph_pos: 1588,
                cluster: 1,
                x_advance: 6.0,
            },
        ],
    );
    assert_shape_matches_baseline(
        "<--",
        1.0,
        &[
            GlyphBaseline {
                glyph_pos: 1742,
                cluster: 0,
                x_advance: 6.0,
            },
            GlyphBaseline {
                glyph_pos: 1742,
                cluster: 1,
                x_advance: 6.0,
            },
            GlyphBaseline {
                glyph_pos: 1589,
                cluster: 2,
                x_advance: 6.0,
            },
        ],
    );
}
