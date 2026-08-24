# `pane.get_progress()`

!!! danger "Removed: no scripting engine"

    This page documents part of the rhai (and, before that, Lua) **scripting
    API**, which has been removed entirely. OnlyTerm's configuration format
    is now [ktav](../../../migration-to-ktav.md), a static `key: value` data
    format with no expressions, function calls, or callbacks of any kind --
    there is nothing left in OnlyTerm that could call this function, invoke
    this method, or construct this object. The description and examples
    below are kept for historical reference (e.g. if you're migrating a very
    old config and trying to understand what it used to do), but none of it
    is callable today. See the [changelog](../../../changelog.md#continuousnightly)
    for the full rationale.

{{since('nightly')}}

Returns the *progress* state associated with the pane.

By default, when the terminal is reset, the progress state will be `"None"` to
indicate that no progress has been reported.

An application can use the ConEmu style progress report OSC sequence to alter
the progress state, which has the following form:

```
ESC ] 9 ; 4 ; st ; pr ST
```

When `st` is:

* `0`: set progress to `"None"`
* `1`: set progress value to `pr` (number, 0-100). Reported as `{Percentage = pr}`
* `2`: set error state in progress, `pr` is optional and will be assumed 0 if omitted. Reported as `{Error = pr}`
* `3`: set indeterminate state. Reported as `"Indeterminate"`
* `4`: set paused state, pr is optional. *Not supported by OnlyTerm*

When the progress OSC is processed, the state is captured, and if it has
changed, OnlyTerm will trigger an update of the tab bar, which will in turn
cause events such as
[format-tab-title](../window-events/format-tab-title.md) to trigger.

The progress information is not directly used by onlyterm, but you can access it
from your config to style tabs differently according to progress. For example, the
following will prefix the tab with the progress percentage expressed using Nerd
Fonts circle symbols and adjust its color based on whether it is success or
error progress:

```lua
local onlyterm = require 'onlyterm'

local function tab_title(tab_info)
  local title = tab_info.tab_title
  -- if the tab title is explicitly set, take that
  if title and #title > 0 then
    return title
  end
  -- Otherwise, use the title from the active pane
  -- in that tab
  return tab_info.active_pane.title
end

local PCT_GLYPHS = {
  onlyterm.nerdfonts.md_circle_slice_1,
  onlyterm.nerdfonts.md_circle_slice_2,
  onlyterm.nerdfonts.md_circle_slice_3,
  onlyterm.nerdfonts.md_circle_slice_4,
  onlyterm.nerdfonts.md_circle_slice_5,
  onlyterm.nerdfonts.md_circle_slice_6,
  onlyterm.nerdfonts.md_circle_slice_7,
  onlyterm.nerdfonts.md_circle_slice_8,
}
local function pct_glyph(pct)
  local slot = math.floor(pct / 12)
  return PCT_GLYPHS[slot + 1]
end

onlyterm.on(
  'format-tab-title',
  function(tab, tabs, panes, config, hover, max_width)
    local progress = tab.active_pane.progress or 'None'
    local title = tab_title(tab)
    local elements = {
      { Text = string.format('%d: ', tab.tab_index + 1) },
    }

    if progress ~= 'None' then
      local color = 'green'
      local status
      if progress.Percentage ~= nil then
        -- status = string.format("%d%%", progress.Percentage)
        status = pct_glyph(progress.Percentage)
      elseif progress.Error ~= nil then
        -- status = string.format("%d%%", progress.Error)
        status = pct_glyph(progress.Error)
        color = 'red'
      elseif progress == 'Indeterminate' then
        status = '~'
      else
        status = onlyterm.serde.json_encode(progress)
      end

      table.insert(elements, { Foreground = { Color = color } })
      table.insert(elements, { Text = status })
      table.insert(elements, { Foreground = 'Default' })
    end

    table.insert(elements, { Text = ' ' .. title .. ' ' })

    return elements
  end
)

return onlyterm.config_builder()
```
