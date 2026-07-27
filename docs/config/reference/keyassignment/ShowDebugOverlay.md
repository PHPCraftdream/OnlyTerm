# ``ShowDebugOverlay``

{{since('20210814-124438-54e29167')}}

Overlays the current tab with the debug overlay, which is a combination
of a debug log and a rhai [REPL](https://en.wikipedia.org/wiki/Read%E2%80%93eval%E2%80%93print_loop).

The REPL has the following globals available:

* all of the usual top-level functions and modules (`color::`, `serde::`, `procinfo::`, `plugin::`, `mux::`)
* `window` - the [window](../window/index.md) object for the current window

The rhai context in the REPL is not connected to any global state; you cannot use it
to dynamically assign event handlers for example.  It is primarily useful for
prototyping rhai snippets before you integrate them fully into your config.

```rhai
config.keys = [
  // CTRL-SHIFT-l activates the debug overlay
  #{ key: "L", mods: "CTRL", action: act.ShowDebugOverlay },
]
```
