# Function require

{{since('20230320-124340-559cb7b0')}}

!!! Warning

    Git-based plugin installation has been removed from wezterm. `require()`
    no longer accepts a Git URL and will no longer clone anything; only a
    local filesystem path/directory is accepted. Install plugins by placing
    their files locally (for example by cloning the repo yourself from the
    command line) and requiring that local path instead.

The function takes a single string parameter: the path to a local directory
containing the plugin (specifically, a `plugin/init.lua` inside that
directory).

```lua
local local_plugin =
  wezterm.plugin.require '/Users/developer/projects/my.Plugin'
```
