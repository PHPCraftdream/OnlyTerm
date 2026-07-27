---
title: wezterm.has_action
tags:
 - utility
 - version
---

# wezterm.has_action(NAME)

{{since('20230408-112425-69ae8472')}}

Returns true if the string *NAME* is a valid key assignment action variant
that can be used with [wezterm.action](action.md).

This is useful when you want to use a wezterm configuration across multiple
different versions of wezterm.

!!! warning "Pending rhai conversion"

    The code example(s) below still use Lua syntax from before OnlyTerm's
    config engine switched to rhai. The *option names, event names and
    object/method shapes* are unchanged -- only the scripting syntax differs.
    See the [migration guide](../../../migration-lua-to-rhai.md) for the Lua-to-rhai
    syntax mapping to translate this example yourself, or watch for a
    follow-up documentation pass that rewrites it directly.

```lua
if wezterm.has_action 'PromptInputLine' then
  table.insert(config.keys, {
    key = 'p',
    mods = 'LEADER',
    action = wezterm.action.PromptInputLine {
      -- other parameters here
    },
  })
end
```
