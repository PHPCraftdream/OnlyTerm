---
tags:
  - font
---
# `font_dirs`

By default, wezterm will use an appropriate system-specific method for
locating the fonts that you specify using the options below.  In addition,
if you configure the `font_dirs` option, wezterm will load fonts from that
set of directories:

```
## This tells wezterm to look first for fonts in the directory named
## `fonts` that is found alongside your `onlyterm.ktav` file.
## As this option is an array, you may list multiple locations if
## you wish.
font_dirs: [fonts]
```

wezterm will scan the `font_dirs` to build a database of available fonts.  When
resolving a font, wezterm will first use the configured
[font_locator](font_locator.md) which is typically the system specific font
resolver.  If the system doesn't resolve the requested font, the fonts from
`font_dirs` are searched for a match.

If you want to only find fonts from your `font_dirs`, perhaps because you have
a self-contained wezterm config that you carry around with you between multiple
systems and don't want to install those fonts on every system that you use,
then you can set:

```
font_locator: ConfigDirsOnly
```

## Keep `font_dirs` portable across operating systems

Relative entries in `font_dirs` are resolved relative to the directory that
holds your `onlyterm.ktav` file, and that resolution already happens the same
way on every OS -- so a relative path such as `fonts` or `../shared-fonts` is
safe to sync between a Windows machine and Linux/macOS without any changes.

Avoid writing an **absolute, OS-specific path** into `font_dirs`, for example:

```
## Don't do this if you sync your config between operating systems:
font_dirs: [C:/Windows/Fonts]
```

A path like this only exists on Windows; on Linux or macOS it silently
resolves to nothing, and even on Windows, scanning an entire system font
directory through `font_dirs` is not the recommended way to reach the system
fonts (it has been observed to be slow and, with a large enough directory, to
crash the font parser). If you want wezterm to use the fonts that are already
installed on whichever OS you're running on, leave `font_locator` unset (or
explicit per-OS via `Gdi`/`CoreText`/`FontConfig` -- see
[font_locator](font_locator.md)): its default already resolves to the correct
system-native font locator for the current platform (`Gdi` on Windows,
`CoreText` on macOS, `FontConfig` elsewhere), with no path to hardcode at all.

Reserve `font_dirs` for a small, deliberately curated set of *extra* font
files that you bundle and carry around with your config -- ideally referenced
with paths relative to your `onlyterm.ktav` -- rather than for pointing at an
entire OS-provided system font folder.


