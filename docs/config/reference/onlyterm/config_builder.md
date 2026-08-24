---
title: onlyterm.config_builder
tags:
 - utility
---

# onlyterm.config_builder()

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

Returns a config builder object that can be used to define your configuration:

```lua
local onlyterm = require 'onlyterm'

local config = onlyterm.config_builder()

config.color_scheme = 'Batman'

return config
```

The config builder may look like a regular rhai map but it is really a special
type that knows how to log warnings or generate errors if you attempt
to define an invalid configuration option.

For example, with this erroneous config:

```lua
local onlyterm = require 'onlyterm'

-- Allow working with both the current release and the nightly
local config = {}
if onlyterm.config_builder then
  config = onlyterm.config_builder()
end

function helper(config)
  config.wrong = true
end

function another_layer(config)
  helper(config)
end

config.color_scheme = 'Batman'

another_layer(config)

return config
```

When evaluated by earlier versions of onlyterm, this config will produce the
following warning, which is terse and doesn't provide any context on where the
mistake was made, requiring you to hunt around and find where `wrong` was
referenced:

```
11:44:11.668  WARN   onlyterm_dynamic::error > `wrong` is not a valid Config field.  There are too many alternatives to list here; consult the documentation!
```

When using the config builder, the warning message is improved:

```
11:45:23.774  WARN   onlyterm_dynamic::error > `wrong` is not a valid Config field.  There are too many alternatives to list here; consult the documentation!
11:45:23.787  WARN   config::lua            > Attempted to set invalid config option `wrong` at:
    [1] /tmp/wat.lua:10 global helper
    [2] /tmp/wat.lua:14 global another_layer
    [3] /tmp/wat.lua:19
```

The config builder provides a method that allows you to promote the warning to a lua error:

```
config:set_strict_mode(true)
```

The consequence of an error is that onlyterm will show a configuration error
window and use the default config until you have resolved the error and
reloaded the configuration.  When not using strict mode, the warning
will not prevent the rest of your configuration from being used.



