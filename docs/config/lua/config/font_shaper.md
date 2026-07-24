---
tags:
  - font
---
# `font_shaper`

specifies the method by which text is mapped to glyphs in the available fonts.
The shaper is responsible for handling kerning, ligatures and emoji
composition.  The default is `RustyBuzz`, a pure-Rust port of the HarfBuzz
shaping algorithm. `Harfbuzz` (the original C/C++ library) remains available
if you hit a shaping regression on the new default.

{{since('20211204-082213-a66c61ee9')}}

The incomplete `Allsorts` shaper was removed.

{{since('nightly')}}

The default changed from `Harfbuzz` to `RustyBuzz`, a pure-Rust
reimplementation of the same shaping algorithm, as part of an effort to
remove C/C++ dependencies from wezterm. If you notice a shaping regression,
you can set `config.font_shaper = "Harfbuzz"` to restore the previous
behavior and please file an issue.
