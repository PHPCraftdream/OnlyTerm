# ``ShowDebugOverlay``

{{since('20210814-124438-54e29167')}}

!!! danger "The REPL has been removed"

    This overlay used to combine a debug log with a rhai (and, before that,
    Lua) [REPL](https://en.wikipedia.org/wiki/Read%E2%80%93eval%E2%80%93print_loop)
    that exposed the same functions/modules and the current `window` object
    for live, interactive evaluation. With the scripting engine removed
    entirely (see the [changelog](../../../changelog.md#continuousnightly)),
    there is nothing left for a REPL to evaluate against, so it has been
    deleted. The overlay now shows a static summary of the running OnlyTerm
    process (version, target triple, renderer/GPU environment) plus a live
    tail of the application's own log, and nothing else.

Overlays the current tab with the debug overlay: a version/environment
summary and a live tail of OnlyTerm's own application log, useful for
troubleshooting.

```
keys: [
  ## CTRL-SHIFT-l activates the debug overlay
  { key: L, mods: CTRL, action: ShowDebugOverlay }
]
```
