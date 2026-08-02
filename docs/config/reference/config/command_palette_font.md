---
tags:
  - font
  - command_palette
---
# `command_palette_font`

{{since('nightly')}}

Configures the font to use for command palette. The `command_palette_font`
setting can specify a set of fallbacks and other options, and is described
in more detail in the [Fonts](../../fonts.md) section.

If not specified, the font is same as the font in `window_frame.font`

`command_palette_font` is a `TextStyle` object (the same shape used by the
main [font](font.md) option); the `wezterm.font`/`wezterm.font_with_fallback`
scripting helpers linked from older versions of this page no longer exist
— write the `TextStyle` value directly.

To specify `command_palette_font`:

```
command_palette_font: { font: [{ family: Roboto }] }
```
