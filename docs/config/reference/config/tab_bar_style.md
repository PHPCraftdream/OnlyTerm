---
tags:
  - tab_bar
---
# `tab_bar_style`

{{since('20210814-124438-54e29167')}}

`new_tab_left`, `new_tab_right`, `new_tab_hover_left`, `new_tab_hover_right`
have been removed and replaced by the more flexible `new_tab` and `new_tab_hover` elements.

{{since('20210502-154244-3f7122cb')}}

`active_tab_left`, `active_tab_right`, `inactive_tab_left`,
`inactive_tab_right`, `inactive_tab_hover_left`, `inactive_tab_hover_right`
have been removed and replaced by the more flexible
[format-tab-title](../window-events/format-tab-title.md) event.

{{since('20210314-114017-04b7cedd')}}

This config option allows styling the elements that appear in the tab bar.
This configuration supplements the [tab bar color](../../appearance.md#tab-bar-appearance-colors)
options.

Styling in this context refers to how the edges of the tabs and the new tab button are rendered.
The default is simply a space character. Each field's value is a plain string: previously that
string was produced by calling the [wezterm.format](../wezterm/format.md) scripting function to
build up a sequence of terminal escape codes (for color/attribute changes) around some text; with
no scripting engine, `format(...)` can no longer be called from config.

The defaults for each of these styles is simply a space.  For each element, the foreground
and background colors are set as per the tab bar colors you've configured.

The available elements are:

* `active_tab_left`, `active_tab_right` - the left and right sides of the active tab
* `inactive_tab_left`, `inactive_tab_right` - the left and right sides of inactive tabs
* `inactive_tab_hover_left`, `inactive_tab_hover_right` - the left and right sides of inactive tabs in the hover state
* `new_tab_left`, `new_tab_right` - the left and right sides of the new tab `+` button
* `new_tab_hover_left`, `new_tab_hover_right` - the left and right sides of the new tab `+` button in the hover state.

!!! danger "Non-functional: styled tab edges required the scripting engine"

    The example that used to appear here called `nerdfonts(...)` and
    `format(...)` (rhai scripting functions, now both removed) to build a
    string containing terminal escape sequences for the PowerLine-styled
    tab edges shown below. Since `tab_bar_style` fields are plain `String`
    values, you could in principle still set one to a literal string
    containing raw terminal escape bytes — but ktav's string escape set
    (`\\`, `\,`, `\}`, `\]`, `\{`, `\[`, `\n`, `\r`, `\.`, `\:`; see the
    [migration guide](../../../migration-to-ktav.md)) has no `\x1b`-style
    hex-byte escape, so there is currently no way to write the required ESC
    (0x1B) control byte directly in a ktav value either. There is presently
    no static way to reproduce this styled-tab-edge example; a plain-text
    (non-escape-sequence) value such as a Nerd Font glyph character used
    directly, e.g. `active_tab_left: ` followed by the glyph itself pasted
    in as literal UTF-8 text with no coloring, is the closest static
    approximation.

#### Retro Tab Bar with Integrated Window Management Buttons

{{since('20230408-112425-69ae8472')}}

When using [`window_decorations =
"INTEGRATED_BUTTONS|RESIZE"`](window_decorations.md), you can
control how the different buttons are drawn for the retro tab bar:

* `window_hide`, `window_hide_hover` - the minimize/hide button
* `window_maximize`, `window_maximize_hover` - the maximize button
* `window_close`, `window_close_hover` - the close button

