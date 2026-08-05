### Font Related Configuration

WezTerm bundles [JetBrains Mono](https://www.jetbrains.com/lp/mono/),
[Nerd Font Symbols](https://nerdfonts.com),
[Noto Color Emoji](https://www.google.com/get/noto/help/emoji/), and
[Noto Sans Symbols / Noto Sans Symbols
2](https://fonts.google.com/noto/specimen/Noto+Sans+Symbols) fonts and uses
those for the default font configuration. The two Noto Sans Symbols fonts
sit at the end of the built-in fallback chain as a broad-coverage safety
net for symbol codepoints that neither the primary monospace font nor the
Nerd Font glyph set include (for example the Miscellaneous Technical block,
U+2300-U+23FF), so that unsupported symbols degrade to *some* legible glyph
instead of an empty tofu box.

If you wish to use a different font face, set the `font` option directly to
a `TextStyle` object (there is no `wezterm.font(...)` helper anymore — see
[wezterm.font](reference/wezterm/font.md), a removed scripting function):

```
font: { font: [{ family: Fira Code }] }
```

```
## You can specify some parameters to influence the font selection;
## for example, this selects a Bold, Italic font variant.
font: { font: [{ family: JetBrains Mono, weight: Bold, italic: true }] }
```

#### Fallback

WezTerm allows specifying an ordered list of fonts; when resolving
text into glyphs the first font in the list is consulted, and if the
glyph isn't present in that font, WezTerm proceeds to the next font
in the fallback list.

The default fallback includes the popular [Nerd Font
Symbols](https://nerdfonts.com) font, which means that you don't need to use
specially patched fonts to use the powerline or Nerd Fonts symbol glyphs.

You can specify your own fallback; that's useful if you've got a killer
monospace font, but it doesn't have glyphs for the asian script that you
sometimes work with:

```
font: {
  font: [
    { family: Fira Code }
    { family: DengXian }
  ]
}
```

WezTerm will still append its default fallback to whatever list you specify,
so you needn't worry about replicating that list if you set your own fallback.

If none of the fonts in the fallback list (including WezTerm's default fallback
list) contain a given glyph, then wezterm will resolve the system fallback list
and try those fonts too.  If a glyph cannot be resolved, wezterm will render a
special "Last Resort" glyph as a placeholder.  You may notice the placeholder
appear momentarily and then refresh itself to the system fallback glyph on some
systems.

### Font Related Options

Additional options for configuring fonts can be found elsewhere in the docs:

* [bold_brightens_ansi_colors](reference/config/bold_brightens_ansi_colors.md) - whether bold text uses the bright ansi palette
* [dpi](reference/config/dpi.md) - override the DPI; potentially useful for X11 users with high-density displays if experiencing tiny or blurry fonts
* [font_dirs](reference/config/font_dirs.md) - look for fonts in a set of directories
* [font_locator](reference/config/font_locator.md) - override the system font resolver
* [font_rules](reference/config/font_rules.md) - advanced control over which fonts are used for italic, bold and other textual styles
* [font_shaper](reference/config/font_shaper.md) - affects kerning and ligatures
* [font_size](reference/config/font_size.md) - change the size of the text
* [freetype_load_flags](reference/config/freetype_load_flags.md) - advanced hinting configuration
* [freetype_load_target](reference/config/freetype_load_target.md) - configure hinting and anti-aliasing
* [freetype_render_target](reference/config/freetype_render_target.md) - configure anti-aliasing
* [cell_width](reference/config/cell_width.md) - scale the font-specified cell width
* [line_height](reference/config/line_height.md) - scale the font-specified line height
* [search_font_dirs_for_fallback](reference/config/search_font_dirs_for_fallback.md) - also search `font_dirs` when resolving fallback fonts for missing glyphs
* [wezterm.font](reference/wezterm/font.md) - removed scripting function; see the page for what a `{ family: ..., ... }` object replaces it with
* [wezterm.font_with_fallback](reference/wezterm/font_with_fallback.md) - removed scripting function; see the page for what a `font: { font: [...] }` block replaces it with

## Troubleshooting Fonts

You may use `wezterm ls-fonts` to have wezterm explain information about which font files it will use for the different text styles.

It shows output like this:

```console
$ wezterm ls-fonts
Primary font:
font: {
    font: [
        ## /home/wez/.fonts/OperatorMonoSSmLig-Medium.otf, FontDirs
        { family: Operator Mono SSm Lig, weight: DemiLight }

        ## /home/wez/.fonts/MaterialDesignIconsDesktop.ttf, FontDirs
        { family: Material Design Icons Desktop }

        ## /usr/share/fonts/jetbrains-mono-fonts/JetBrainsMono-Regular.ttf, FontConfig
        { family: JetBrains Mono }

        ## /usr/share/fonts/google-noto-emoji/NotoColorEmoji.ttf, FontConfig
        ## Assumed to have Emoji Presentation
        ## Pixel sizes: [128]
        { family: Noto Color Emoji }
    ]
}


When Intensity=Half Italic=true:
font: {
    font: [
        ## /home/wez/.fonts/OperatorMonoSSmLig-BookItalic.otf, FontDirs
        { family: Operator Mono SSm Lig, weight: 325, style: Italic }

        ## /home/wez/.fonts/MaterialDesignIconsDesktop.ttf, FontDirs
        { family: Material Design Icons Desktop }

        ## /usr/share/fonts/jetbrains-mono-fonts/JetBrainsMono-Regular.ttf, FontConfig
        { family: JetBrains Mono }

        ## /usr/share/fonts/google-noto-emoji/NotoColorEmoji.ttf, FontConfig
        ## Assumed to have Emoji Presentation
        ## Pixel sizes: [128]
        { family: Noto Color Emoji }
    ]
}
...
```

You can ask wezterm to including a listing of all of the fonts on the system in a form that can be copied and pasted into the configuration file:

```console
$ wezterm ls-fonts --list-system
<same output as above, but then:>
112 fonts found in your font_dirs + built-in fonts:
{ family: Cascadia Code, weight: ExtraLight, stretch: Normal, style: Normal } -- /home/wez/.fonts/CascadiaCode.ttf index=0 variation=1, FontDirs
{ family: Cascadia Code, weight: Light, stretch: Normal, style: Normal } -- /home/wez/.fonts/CascadiaCode.ttf index=0 variation=2, FontDirs
{ family: Cascadia Code, weight: DemiLight, stretch: Normal, style: Normal } -- /home/wez/.fonts/CascadiaCode.ttf index=0 variation=3, FontDirs
{ family: Cascadia Code, weight: Regular, stretch: Normal, style: Normal } -- /home/wez/.fonts/CascadiaCode.ttf index=0 variation=4, FontDirs
{ family: Cascadia Code, weight: DemiBold, stretch: Normal, style: Normal } -- /home/wez/.fonts/CascadiaCode.ttf index=0 variation=5, FontDirs
{ family: Cascadia Code, weight: Bold, stretch: Normal, style: Normal } -- /home/wez/.fonts/CascadiaCode.ttf index=0 variation=6, FontDirs
{ family: Fira Code, weight: Light, stretch: Normal, style: Normal } -- /home/wez/.fonts/FiraCode-Light.otf, FontDirs
{ family: Fira Code, weight: Regular, stretch: Normal, style: Normal } -- /home/wez/.fonts/FiraCode-Regular.otf, FontDirs
{ family: Fira Code, weight: 450, stretch: Normal, style: Normal } -- /home/wez/.fonts/FiraCode-Retina.otf, FontDirs
{ family: Fira Code, weight: Medium, stretch: Normal, style: Normal } -- /home/wez/.fonts/FiraCode-Medium.otf, FontDirs
{ family: Fira Code, weight: Bold, stretch: Normal, style: Normal } -- /home/wez/.fonts/FiraCode-Bold.otf, FontDirs
{ family: Font Awesome 5 Free, weight: Black, stretch: Normal, style: Normal } -- /home/wez/.fonts/Font Awesome 5 Free-Solid-900.otf, FontDirs
...
690 system fonts found using FontConfig:
{ family: Abyssinica SIL, weight: Regular, stretch: Normal, style: Normal } -- /usr/share/fonts/sil-abyssinica-fonts/AbyssinicaSIL-R.ttf, FontConfig
{ family: C059, weight: Regular, stretch: Normal, style: Normal } -- /usr/share/fonts/urw-base35/C059-Bold.t1, FontConfig
{ family: C059, weight: Regular, stretch: Normal, style: Normal } -- /usr/share/fonts/urw-base35/C059-Roman.otf, FontConfig
{ family: C059, weight: Regular, stretch: Normal, style: Normal } -- /usr/share/fonts/urw-base35/C059-Roman.t1, FontConfig
{ family: C059, weight: Regular, stretch: Normal, style: Italic } -- /usr/share/fonts/urw-base35/C059-BdIta.t1, FontConfig
{ family: C059, weight: Regular, stretch: Normal, style: Italic } -- /usr/share/fonts/urw-base35/C059-Italic.otf, FontConfig
...
```

You may also display the shaping plan for a given text string; in this example,
the `a` and the `b` are separated by a special symbol which is not present in
the main font, so we expect to see a different font used for that glyph:

```console
$ wezterm ls-fonts --text a🞄b
a    \u{61}       x_adv=8  glyph=29   { family: Operator Mono SSm Lig, weight: DemiLight, stretch: Normal, style: Normal }
                                      /home/wez/.fonts/OperatorMonoSSmLig-Medium.otf, FontDirs
🞄    \u{1f784}    x_adv=4  glyph=9129 { family: Symbola, weight: Regular, stretch: SemiCondensed, style: Normal }
                                      /usr/share/fonts/gdouros-symbola/Symbola.ttf, FontConfig
b    \u{62}       x_adv=8  glyph=30   { family: Operator Mono SSm Lig, weight: DemiLight, stretch: Normal, style: Normal }
                                      /home/wez/.fonts/OperatorMonoSSmLig-Medium.otf, FontDirs
```
