
## Introduction

<!-- See also https://github.com/wez/wezterm/commit/e4ae8a844d8feaa43e1de34c5cc8b4f07ce525dd -->

A WezTerm plugin is a package of rhai files that provides some predefined
functionality not in the core product.

!!! Warning

    **Git-based plugin installation has been removed.** WezTerm no longer
    embeds a Git implementation, so `plugin::require()` can no longer clone a
    plugin repo by URL. Plugins must instead be installed by placing their
    files on your local disk and requiring that local path/directory directly,
    as described below.

!!! Tip

    Plugins are now written in [rhai](https://rhai.rs/), WezTerm's current
    config language. Plugins published as Lua (`plugin/init.lua`) are **not**
    compatible with the rhai engine and must be republished with a
    `plugin/init.rhai` entry point. See the
    [Lua → rhai migration guide](../migration-lua-to-rhai.md) for the syntax
    translation.

    Michael Brusegard maintains a [list of plugins](https://github.com/michaelbrusegard/awesome-wezterm)

## Installing a Plugin

1. Obtain the plugin's files yourself (for example, `git clone` it manually
   from the command line, or download and extract a release archive) into a
   directory on your local disk.
2. Pass that local directory's path to [`plugin::require()`](lua/wezterm.plugin/require.md):

```rhai
let a_plugin = plugin::require("/home/user/projects/myPlugin");

let mut config = #{};

a_plugin.apply_to_config(config);

config
```

Plugins can be configured, for example:

```rhai
let a_plugin = plugin::require("/home/user/projects/myPlugin");

let mut config = #{};

let my_plugin_config = #{ enable: true, location: "right" };

a_plugin.apply_to_config(config, my_plugin_config);

config
```

!!! Note

    Consult the README for a particular plugin to discover any specific configuration options.

## Updating Plugins

Since WezTerm no longer manages a clone of the plugin for you, updating a
plugin means updating the files in the local directory yourself (for example,
`git pull` in that directory, or downloading a newer release) and then
reloading your WezTerm configuration.

`plugin::list()` and `plugin::update_all()` are retained as callable names for
backwards compatibility with existing configs, but they now report an error
explaining that git-based plugin management has been removed; they no longer
enumerate or update anything.

## Removing a Plugin

Delete the local plugin directory and remove the corresponding
`plugin::require(...)` line from your config.

## Developing a Plugin

1. Create a local project directory.
2. Add a file `plugin/init.rhai`.
3. `plugin/init.rhai` must evaluate to a module object that exports an
   `apply_to_config` function. This function must accept at least a config
   parameter, but may take other parameters, or a map with a `config` field.
4. Add any other rhai code needed to fulfil the plugin feature set.
5. Reference the plugin using its local path, e.g.
   ```rhai
   let a_plugin = plugin::require("/home/user/projects/myPlugin");
   ```

A minimal `plugin/init.rhai`:

```rhai
// plugin/init.rhai
#{
    apply_to_config: |config| {
        config.color_scheme = "Batman";
    },
}
```

Since the plugin is required directly from its local path, changes made to the
project take effect the next time your WezTerm configuration is reloaded — no
separate sync/update step is needed.

### Splitting a plugin across multiple files

WezTerm's rhai engine does not yet wire up rhai's `import`/module resolution, so
a plugin is currently a single `plugin/init.rhai` file. If you have shared
logic you want to reuse across plugins, keep each plugin self-contained in its
`init.rhai`, or factor the shared code into its own plugin that the others
`plugin::require`.

!!! Tip
    Review other published plugins to discover more details on how to structure a plugin project
