
## Introduction

<!-- See also https://github.com/wez/wezterm/commit/e4ae8a844d8feaa43e1de34c5cc8b4f07ce525dd -->

A Wezterm plugin is a package of Lua files that provide
some predefined functionality not in the core product.

!!! Warning

    **Git-based plugin installation has been removed.** Wezterm no longer
    embeds a Git implementation, so `wezterm.plugin.require()` can no longer
    clone a plugin repo by URL. Plugins must instead be installed by placing
    their files on your local disk and requiring that local path/directory
    directly, as described below.

!!! Tip

    Michael Brusegard maintains a [list of plugins](https://github.com/michaelbrusegard/awesome-wezterm)

## Installing a Plugin

1. Obtain the plugin's files yourself (for example, `git clone` it manually
   from the command line, or download and extract a release archive) into a
   directory on your local disk.
2. Pass that local directory's path to [`wezterm.plugin.require()`](lua/wezterm.plugin/require.md):

```lua
local wezterm = require 'wezterm'
local a_plugin = wezterm.plugin.require '/home/user/projects/myPlugin'

local config = wezterm.config_builder()

a_plugin.apply_to_config(config)

return config
```

Plugins can be configured, for example:

```lua
local wezterm = require 'wezterm'
local a_plugin = wezterm.plugin.require '/home/user/projects/myPlugin'

local config = wezterm.config_builder()

local myPluginConfig = { enable = true, location = 'right' }

a_plugin.apply_to_config(config, myPluginConfig)

return config
```

!!! Note

    Consult the README for a particular plugin to discover any specific configuration options.

## Updating Plugins

Since Wezterm no longer manages a clone of the plugin for you, updating a
plugin means updating the files in the local directory yourself (for
example, `git pull` in that directory, or downloading a newer release) and
then reloading your Wezterm configuration.

`wezterm.plugin.list()` and `wezterm.plugin.update_all()` are retained as
callable functions for backwards compatibility with existing configs, but
they now report an error explaining that git-based plugin management has
been removed; they no longer enumerate or update anything.

## Removing a Plugin

Delete the local plugin directory and remove the corresponding
`wezterm.plugin.require(...)` line from your config.

## Developing a Plugin

1. Create a local project directory.
2. Add a file `plugin/init.lua`.
3. `init.lua` must return a module that exports an `apply_to_config`
   function. This function must accept at least a config builder parameter, but may
   pass other parameters, or a Lua table with a `config` field that maps
   to a config build parameter.
4. Add any other Lua code needed to fulfil the plugin feature set.
5. Reference the plugin using its local path, e.g.
   ```lua
   local a_plugin = wezterm.plugin.require '/home/user/projects/myPlugin'
   ```

Since the plugin is required directly from its local path, changes made to
the project take effect the next time your Wezterm configuration is
reloaded -- no separate sync/update step is needed.

### Managing a Plugin with Multiple Lua Modules

When `requiring` other Lua modules in your plugin, update `package.path` to
include the plugin's own directory. For example:

```lua
local plugin_dir = '/home/user/projects/myPlugin'
local separator = package.config:sub(1, 1) == '\\' and '\\' or '/'
package.path = package.path .. ';' .. plugin_dir .. separator .. 'plugin' .. separator .. '?.lua'
```

!!! Tip
    Review other published plugins to discover more details on how to structure a plugin project
