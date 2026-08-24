# `onlyterm.time.call_after(interval_seconds, function)`

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

{{since('20220807-113146-c2fee766')}}

Arranges to call your callback function after the specified number of seconds
have elapsed.

Here's a contrived example that demonstrates a configuration that
varies based on the time. In this case, the idea is that the background
color is derived from the current number of minutes past the hour.

In order for the value to be picked up for the next minute, `call_after`
is used to schedule a callback 60 seconds later and it then generates
a background color by extracting the current minute value and scaing
it to the range 0-255 and using that to assign a background color:

```lua
local onlyterm = require 'onlyterm'

-- Reload the configuration every minute
onlyterm.time.call_after(60, function()
  onlyterm.reload_configuration()
end)

local amount =
  math.ceil((tonumber(onlyterm.time.now():format '%M') / 60) * 255)

return {
  colors = {
    background = 'rgb(' .. amount .. ',' .. amount .. ',' .. amount .. ')',
  },
}
```

With great power comes great responsibility: if you schedule a lot of frequent
callbacks, or frequently reload your configuration in this way, you may
increase the CPU load on your system because you are asking it to work harder.

{{since('20230320-124340-559cb7b0')}}

You can use fractional seconds to delay by more precise intervals.
