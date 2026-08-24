---
tags:
  - appearance
  - pane_select
  - font
---
# `pane_select_font`

{{since('nightly')}}

Configures the font to use for pane selection mode. The `pane_select_font`
setting can specify a set of fallbacks and other options, and is described
in more detail in the [Fonts](../../fonts.md) section.

If not specified, the font is same as the font in `window_frame.font`

`pane_select_font` is a `TextStyle` object (the same shape used by the main
[font](font.md) option); the `onlyterm.font`/`onlyterm.font_with_fallback`
scripting helpers linked from older versions of this page no longer exist
— write the `TextStyle` value directly.

To specify `pane_select_font`:

```
pane_select_font: { font: [{ family: Roboto }] }
```
