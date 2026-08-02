# `SwitchWorkspaceRelative`

{{since('20220319-142410-0fcdea07')}}

Switch to the workspace relative to the current workspace.  Workspaces are ordered
lexicographically based on their names.

The argument value specifies an offset. eg: `-1` switches to the workspace
immediately prior to the current workspace, while `1` switches to the workspace
immediately following the current workspace.

This example binds CTRL-N and CTRL-P to move forwards, backwards through workspaces.
It shows the active workspace in the title bar.  The launcher menu can be used
to create workspaces.

!!! note "`update-right-status` no longer exists"

    The original version of this example also registered a
    `wezterm.on('update-right-status', ...)` handler to show the active
    workspace name in the title bar. That event hook has been removed along
    with the rest of the scripting engine — see the
    [changelog](../../../changelog.md#continuousnightly). Only the key
    bindings below (the actual `SwitchWorkspaceRelative` usage) still work.

```
keys: [
  { key: 9, mods: ALT, action: { ShowLauncherArgs: { flags: FUZZY|WORKSPACES } } }
  { key: n, mods: CTRL, action: { SwitchWorkspaceRelative: 1 } }
  { key: p, mods: CTRL, action: { SwitchWorkspaceRelative: -1 } }
]
```

