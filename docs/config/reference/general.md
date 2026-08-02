# Config Reference

OnlyTerm uses [ktav](../../migration-to-ktav.md), a static `key: value` data
format, as its configuration language. This section documents the various
configuration options and types that are available to your configuration
file. There is no scripting engine, no function calls, and no modules to
import — every option is just a key in the config document:

```
font: { font: [{ family: JetBrains Mono }] }
```

If you have an existing `.wezterm.lua` or `.wezterm.rhai`/`onlyterm.rhai`
configuration file, see the [migration guide](../../migration-to-ktav.md)
for how to translate it to the `.ktav` syntax documented in this section.
Many pages in this section still carry a "Removed: no scripting engine"
notice where the page describes part of the old scripting API (functions,
callback objects, event hooks) that no longer exists at all; those are kept
for historical reference only.

## Full List of Configuration Options

[Config Options](config/index.md) has a list of the main configuration options.

