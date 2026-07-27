# `window.active_pane()`

{{since('20230408-112425-69ae8472')}}

A convenience accessor for returning the active pane in the active tab of the window.

In earlier versions of wezterm, you could obtain this via:

!!! warning "Pending rhai conversion"

    The code example(s) below still use Lua syntax from before OnlyTerm's
    config engine switched to rhai. The *option names, event names and
    object/method shapes* are unchanged -- only the scripting syntax differs.
    See the [migration guide](../../../migration-lua-to-rhai.md) for the Lua-to-rhai
    syntax mapping to translate this example yourself, or watch for a
    follow-up documentation pass that rewrites it directly.

```lua
function active_tab(window)
  for _, item in ipairs(window:tabs_with_info()) do
    if item.is_active then
      return item.tab
    end
  end
end

function active_pane(tab)
  for _, item in ipairs(tab:panes_with_info()) do
    if item.is_active then
      return item.pane
    end
  end
end
```

See also [gui_window:active_pane()](../window/active_pane.md), which is similar
to this method, but which can return overlay panes that are not visible to
the mux layer of the API.

