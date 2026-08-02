# `wezterm.procinfo.get_info_for_pid(pid)`

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

Returns a [LocalProcessInfo](../LocalProcessInfo.md) object for the specified
process id.

This function may return `nil` if it was unable to return the info.

```
> wezterm.procinfo.get_info_for_pid(wezterm.procinfo.pid())
{
    "argv": [
        "/home/wez/wez-personal/wezterm/target/debug/wezterm-gui",
    ],
    "children": {
        540513: {
            "argv": [
                "-zsh",
            ],
            "children": {},
            "cwd": "/home/wez",
            "executable": "/usr/bin/zsh",
            "name": "zsh",
            "pid": 540513,
            "ppid": 540450,
            "start_time": 232656896,
            "status": "Sleep",
        },
    },
    "cwd": "/home/wez/wez-personal/wezterm",
    "executable": "/home/wez/wez-personal/wezterm/target/debug/wezterm-gui",
    "name": "wezterm-gui",
    "pid": 540450,
    "ppid": 425276,
    "start_time": 8671498240,
    "status": "Run",
}
```
