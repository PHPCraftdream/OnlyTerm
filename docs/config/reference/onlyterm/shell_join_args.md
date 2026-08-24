---
title: onlyterm.shell_join_args
tags:
 - utility
 - open
 - spawn
 - string
---
# onlyterm.shell_join_args({"foo", "bar"})

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

`onlyterm.shell_join_args` joins together its array arguments by applying posix
style shell quoting on each argument and then adding a space.

```
> onlyterm.shell_join_args{"foo", "bar"}
"foo bar"
> onlyterm.shell_join_args{"hello there", "you"}
"\"hello there\" you"
```

This is useful to safely construct command lines that you wish to pass to the shell.
