# window.get_appearance()

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

**NOTE: You probably want to use [onlyterm.gui.get_appearance()](../onlyterm.gui/get_appearance.md) instead, as it is easier to use!**

{{since('20210814-124438-54e29167')}}

This method returns the appearance of the window environment.  The appearance
can be one of the following 4 values:

* `"Light"` - the normal appearance, with dark text on a light background
* `"Dark"` - "dark mode", with predominantly dark colors and probably a lighter, lower contrasting, text color on a dark background
* `"LightHighContrast"` - light mode but with high contrast colors (not reported on all systems)
* `"DarkHighContrast"` - dark mode but with high contrast colors (not reported on all systems)

onlyterm is able to detect when the appearance has changed and will generate a
[window-config-reloaded](../window-events/window-config-reloaded.md) event for
each window.

This example configuration shows how you can have your color scheme
automatically adjust to the current appearance:

```lua
local onlyterm = require 'onlyterm'

function scheme_for_appearance(appearance)
  if appearance:find 'Dark' then
    return 'Builtin Solarized Dark'
  else
    return 'Builtin Solarized Light'
  end
end

onlyterm.on('window-config-reloaded', function(window, pane)
  local overrides = window:get_config_overrides() or {}
  local appearance = window:get_appearance()
  local scheme = scheme_for_appearance(appearance)
  if overrides.color_scheme ~= scheme then
    overrides.color_scheme = scheme
    window:set_config_overrides(overrides)
  end
end)

return {}
```

### Wayland GNOME Appearance

{{since('20220807-113146-c2fee766')}}

onlyterm uses [XDG Desktop
Portal](https://flatpak.github.io/xdg-desktop-portal/) to determine the
appearance.

In earlier versions you may wish to use an alternative method to determine the
appearance, as onlyterm didn't know how to interrogate the appearance on Wayland
systems, and would always report `"Light"`.

The GNOME desktop environment provides the `gsettings` tool that can
inform us of the selected appearance even in a Wayland session. We can
substitute the call to `window:get_appearance` above with a call to the
following function, which takes advantage of this:

```lua
function query_appearance_gnome()
  local success, stdout = onlyterm.run_child_process {
    'gsettings',
    'get',
    'org.gnome.desktop.interface',
    'gtk-theme',
  }
  -- lowercase and remove whitespace
  stdout = stdout:lower():gsub('%s+', '')
  local mapping = {
    highcontrast = 'LightHighContrast',
    highcontrastinverse = 'DarkHighContrast',
    adwaita = 'Light',
    ['adwaita-dark'] = 'Dark',
  }
  local appearance = mapping[stdout]
  if appearance then
    return appearance
  end
  if stdout:find 'dark' then
    return 'Dark'
  end
  return 'Light'
end
```

Since OnlyTerm will not fire a `window-config-reloaded` event on Wayland for
older versions of onlyterm, you will instead need to listen on the
[update-right-status](../window-events/update-right-status.md) event, which
will essentially poll for the appearance periodically:

```lua
local onlyterm = require 'onlyterm'

function scheme_for_appearance(appearance)
  if appearance:find 'Dark' then
    return 'Builtin Solarized Dark'
  else
    return 'Builtin Solarized Light'
  end
end

onlyterm.on('update-right-status', function(window, pane)
  local overrides = window:get_config_overrides() or {}
  local appearance = query_appearance_gnome()
  local scheme = scheme_for_appearance(appearance)
  if overrides.color_scheme ~= scheme then
    overrides.color_scheme = scheme
    window:set_config_overrides(overrides)
  end
end)

return {}
```
