# `LocalProcessInfo`

!!! danger "Removed: no scripting engine"

    This page documents part of the rhai (and, before that, Lua) **scripting
    API**, which has been removed entirely. OnlyTerm's configuration format
    is now [ktav](../../migration-to-ktav.md), a static `key: value` data
    format with no expressions, function calls, or callbacks of any kind --
    there is nothing left in OnlyTerm that could call this function, invoke
    this method, or construct this object. The description and examples
    below are kept for historical reference (e.g. if you're migrating a very
    old config and trying to understand what it used to do), but none of it
    is callable today. See the [changelog](../../changelog.md#continuousnightly)
    for the full rationale.

{{since('20220101-133340-7edc5b5a')}}

`LocalProcessInfo` represents a process running on the local machine.

It has the following fields:

* `pid` - the process id
* `ppid` - the parent process id
* `name` - a short name for the process. Due to platform limitations, this may be inaccurate and truncated; you probably should prefer to look at the `executable` or `argv` fields instead of this one
* `status` - a string holding the status of the process; it can be `Idle`, `Run`, `Sleep`, `Stop`, `Zombie`, `Tracing`, `Dead`, `Wakekill`, `Waking`, `Parked`, `LockBlocked`, `Unknown`.
* `argv` - a table holding the argument array for the process
* `executable` - the full path to the executable image for the process (may be empty)
* `cwd` - the current working directory for the process (may be empty)
* `children` - a table keyed by child process id and whose values are themselves `LocalProcessInfo` objects that describe the child processes

See [mux-is-process-stateful](mux-events/mux-is-process-stateful.md) and [pane:get_foreground_process_info()](pane/get_foreground_process_info.md)
