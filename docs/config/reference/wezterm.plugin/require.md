# Function require

!!! danger "Removed: no scripting engine"

    This page documents part of the rhai (and, before that, Lua) **scripting
    API**, which has been removed entirely. OnlyTerm's configuration format
    is now [ktav](../../../migration-to-ktav.md), a static `key: value` data
    format with no expressions, function calls, or callbacks of any kind --
    there is nothing left in OnlyTerm that could call this function, invoke
    this method, or construct this object. The description and examples
    below are kept for historical reference (e.g. if you're migrating a very
    old config and trying to understand what it used to do), but none of it
    is callable today. See the [changelog](../../../changelog.md#continuousnightly)
    for the full rationale.

{{since('20230320-124340-559cb7b0')}}

!!! Warning

    Git-based plugin installation has been removed from wezterm. `require()`
    no longer accepts a Git URL and will no longer clone anything; only a
    local filesystem path/directory is accepted. Install plugins by placing
    their files locally (for example by cloning the repo yourself from the
    command line) and requiring that local path instead.

The function takes a single string parameter: the path to a local directory
containing the plugin (specifically, a `plugin/init.rhai` inside that
directory).

```rhai
let local_plugin = plugin::require("/Users/developer/projects/my.Plugin")
```
