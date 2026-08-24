---
title: onlyterm.split_by_newlines
tags:
 - utility
 - string
---
# `onlyterm.split_by_newlines(str)`

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

{{since('20200503-171512-b13ef15f')}}

This function takes the input string and splits it by newlines (both `\n` and `\r\n`
are recognized as newlines) and returns the result as an array of strings that
have the newlines removed.

```lua
local onlyterm = require 'onlyterm'

local example = 'hello\nthere\n'

for _, line in ipairs(onlyterm.split_by_newlines(example)) do
  onlyterm.log_error(line)
end
```


