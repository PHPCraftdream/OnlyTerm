# Config Reference

OnlyTerm uses [rhai](https://rhai.rs/) as its configuration language. This
section documents the various functions, modules and types that are available
to your configuration file. Most of these are simply global functions or
live in one of a small number of modules (`color::`, `serde::`, `procinfo::`,
`plugin::`, `mux::`) that don't require any explicit import:

```rhai
config.font = font("JetBrains Mono")
```

If you have an existing `.wezterm.lua` configuration file, see the
[migration guide](../../migration-lua-to-rhai.md) for how to translate it to
the `.wezterm.rhai` syntax documented in this section.

## Full List of Configuration Options

[Config Options](config/index.md) has a list of the main configuration options.

