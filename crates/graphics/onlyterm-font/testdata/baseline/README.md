# Cairo baseline glyph renders (cairo -> tiny-skia migration, phase A)

This directory holds the "before" reference renders of a handful of
representative COLR/COLRv1 color glyphs, captured while `onlyterm-font`'s
rasterizer (`src/rasterizer/colr.rs`, `freetype.rs`, `harfbuzz.rs`) still
goes through the original **cairo**-based paint path (see
`docs/plans/2026-07-23-decairo-tiny-skia.md`, phase A).

Task C (porting `colr.rs` to tiny-skia types) and the rasterizer ports that
follow (D/E) will change the real render path, so this baseline can no
longer be regenerated against cairo after that point. It exists to be
diffed against the tiny-skia-based result in phase G, using
`onlyterm-font/examples/diff_glyph.rs`.

## Provenance

- Captured at git rev: `047ffddc79d4c4ff7aab1060dc7b6b990ca5c5c6`
  ("onlyterm-font: add tiny-skia painter module scaffolding (cairo
  migration, phase B)") — the render path at this commit is still 100%
  cairo; phase B only added an unused `rasterizer/paint.rs` scaffold.
- Tooling: `onlyterm-font/examples/dump_glyph.rs` (rasterize + PNG/JSON
  dump) and `onlyterm-font/examples/diff_glyph.rs` (PNG diff), both
  introduced in commit `ceb52b8f8`.
- Font: `assets/fonts/NotoColorEmoji.ttf` (COLRv1, gradient-based color
  glyphs — linear/radial/sweep gradients, groups/compositing).
- Render params: `--size 32 --dpi 96`, both rasterizers (`freetype` and
  `harfbuzz`).

## Contents

Naming: `noto-emoji-U+<CODEPOINT>-<rasterizer>.{png,json}`.

| Codepoint | Glyph | Why chosen |
|---|---|---|
| U+1F600 | grinning face | simple baseline color glyph |
| U+1F970 | smiling face with hearts | blush/gradient shading |
| U+1F308 | rainbow | strong linear/radial gradient bands |
| U+1F44D | thumbs up | simple glyph, different silhouette/skin-tone base |
| U+1F929 | star-struck | complex multi-layer composite (eyes/stars/blush) |

Each codepoint has 4 files: `-freetype.png`/`.json` and
`-harfbuzz.png`/`.json`.

Note on ZWJ sequences: `dump_glyph` resolves a single Unicode codepoint to
a glyph id via `cmap` lookup (`FT_Get_Char_Index`); it does not run
HarfBuzz shaping over a full ZWJ grapheme cluster, so it cannot produce
the ligature glyph id for a composed emoji (e.g. the rainbow flag
`1F3F3 FE0F 200D 1F308`). Per the plan, this is acceptable — the 5
single-codepoint glyphs above are a representative sample and this
limitation is simply noted rather than worked around.

## freetype vs harfbuzz comparison (informational, not a pass/fail test)

Ran `diff_glyph` for each codepoint, freetype PNG (`a`) vs harfbuzz PNG
(`b`), default thresholds (`--channel-threshold 24
--max-diff-fraction 0.02`):

| Codepoint | Size | Max per-channel diff | Mean per-channel diff | Pixels over threshold | Result |
|---|---|---|---|---|---|
| U+1F600 | 46x44 | 0 | 0.000 | 0/2024 (0.0000%) | PASS (identical) |
| U+1F970 | 50x48 | 0 | 0.000 | 0/2400 (0.0000%) | PASS (identical) |
| U+1F308 | 42x47 | 0 | 0.000 | 0/1974 (0.0000%) | PASS (identical) |
| U+1F44D | 40x44 | 0 | 0.000 | 0/1760 (0.0000%) | PASS (identical) |
| U+1F929 | 46x44 | 0 | 0.000 | 0/2024 (0.0000%) | PASS (identical) |

All 5 pairs are byte-for-byte pixel-identical (max per-channel diff is 0
in every case), which matches expectations: both rasterizer front-ends
(FreeType-COLR and HarfBuzz-paint) currently funnel through the same
cairo paint backend (`apply_draw_ops_to_context` and friends in
`colr.rs`/`freetype.rs`/`harfbuzz.rs`), so there is no reason for them to
diverge pre-migration. Metadata (`width`, `height`, `bearing_x`,
`bearing_y`, `has_color`) also matches for every pair.

## How to compare against a future tiny-skia result (phase G)

```sh
cargo run -p onlyterm-font --example dump_glyph -- \
    --font assets/fonts/NotoColorEmoji.ttf \
    --codepoint 1F600 --size 32 --dpi 96 \
    --rasterizer both --out /tmp/after-U+1F600

cargo run -p onlyterm-font --example diff_glyph -- \
    --a onlyterm-font/testdata/baseline/noto-emoji-U+1F600-freetype.png \
    --b /tmp/after-U+1F600-freetype.png \
    --meta-a onlyterm-font/testdata/baseline/noto-emoji-U+1F600-freetype.json \
    --meta-b /tmp/after-U+1F600-freetype.json
```

Repeat per codepoint/rasterizer. Some tolerance for subpixel
antialiasing differences is expected and acceptable (see plan, "Риски").
