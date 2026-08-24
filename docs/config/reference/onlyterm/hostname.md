---
title: onlyterm.hostname
tags:
 - utility
---

# `onlyterm.hostname()`

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

This function returns the current hostname of the system that is running onlyterm.
This can be useful to adjust configuration based on the host.

Note that environments that use DHCP and have many clients and short leases may
make it harder to rely on the hostname for this purpose.

```lua
local onlyterm = require 'onlyterm'
local hostname = onlyterm.hostname()

local font_size
if hostname == 'pixelbookgo-localdomain' then
  -- Use a bigger font on the smaller screen of my PixelBook Go
  font_size = 12.0
else
  font_size = 10.0
end

return {
  font_size = font_size,
}
```


