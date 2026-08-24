---
tags:
  - font
---
# `font_rasterizer`

Specifies the method by which fonts are rendered on screen. The default is
`Swash`, a pure-Rust rasterizer. `FreeType` and `Harfbuzz` remain available
if you hit a rendering regression on the new default.

{{since('nightly')}}

The default changed from `FreeType` to `Swash`, a pure-Rust rasterizer, as
part of an effort to remove C/C++ dependencies from onlyterm. `Swash` handles
ordinary (non-color) glyph outlines itself, and automatically falls back to
the `Harfbuzz`-based paint rasterizer for COLR/COLRv1/CBDT/sbix color
glyphs (e.g. color emoji), the same way `FreeType` already did via the
`font_colr_rasterizer` setting. If you notice a rendering regression, you
can set `config.font_rasterizer = "FreeType"` to restore the previous
behavior and please file an issue.
