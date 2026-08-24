
## Plugins have been removed

!!! danger "Removed: plugins required a scripting engine"

    A OnlyTerm plugin was a package of rhai (and, before that, Lua)
    files loaded via `plugin::require(path)`, evaluated by the config's
    scripting engine to produce configuration data or additional behavior.
    OnlyTerm's configuration format is now [ktav](../migration-to-ktav.md), a
    static `key: value` data format with no expressions, function calls, or
    module loading of any kind — there is no scripting engine left to
    evaluate a plugin's `init.rhai`/`init.lua`, and consequently no plugin
    mechanism at all. `plugin::require`, `plugin::list()` and
    `plugin::update_all()` no longer exist in any form. See the
    [changelog](../changelog.md#continuousnightly) for the full rationale
    behind this change.

    Git-based plugin installation (cloning a plugin by URL) had already been
    removed in an earlier release, independently of this change.

If you were relying on a plugin, the options are:

* If the plugin only produced static configuration values (colors, key
  bindings, and so on), copy the resulting values directly into your own
  `onlyterm.ktav` file by hand.
* If the plugin relied on runtime behavior (event hooks, callbacks, or
  anything computed while OnlyTerm is running), there is currently no way to
  reproduce that: the entire scripting/event-hook surface it would have
  depended on has also been removed. See the
  [migration guide](../migration-to-ktav.md) for what's still possible in a
  static config.

An external hooks API may be added later if there's a real need for scripted
customization; this page will be updated if/when that happens.
