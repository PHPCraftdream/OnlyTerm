# `window.active_tab()`

{{since('20230408-112425-69ae8472')}}

A convenience accessor for returning the active tab within the window.

In earlier versions of wezterm, you could obtain this via:

!!! warning "Pending rhai conversion"

    The code example(s) below still use Lua syntax from before OnlyTerm's
    config engine switched to rhai. The *option names, event names and
    object/method shapes* are unchanged -- only the scripting syntax differs.
    See the [migration guide](../../../migration-lua-to-rhai.md) for the Lua-to-rhai
    syntax mapping to translate this example yourself, or watch for a
    follow-up documentation pass that rewrites it directly.

```lua
function active_tab_for_gui_window(gui_window)
  for _, item in ipairs(gui_window:mux_window():tabs_with_info()) do
    if item.is_active then
      return item.tab
    end
  end
end
```

