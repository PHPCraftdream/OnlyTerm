# `SwitchToWorkspace`

{{since('20220319-142410-0fcdea07')}}

Switch to a different workspace, creating it if it doesn't already exist.

`SwitchToWorkspace` accepts two optional parameters:

* `name` - the name of the workspace. If omitted, a randomly generated name will be chosen.
* `spawn` - a [SpawnCommand](../SpawnCommand.md) describing the command that should be started in the workspace if it doesn't already exist.  If omitted, the default program will be spawned in the newly created workspace.

!!! note "`update-right-status` no longer exists"

    The original version of this example also registered a
    `wezterm.on('update-right-status', ...)` handler to show the active
    workspace name in the status bar. That event hook has been removed along
    with the rest of the scripting engine — see the
    [changelog](../../../changelog.md#continuousnightly). Only the key
    bindings below (the actual `SwitchToWorkspace` usage) still work.

```
keys: [
  // Switch to the default workspace
  { key: y, mods: CTRL|SHIFT, action: { SwitchToWorkspace: { name: default } } }
  // Switch to a monitoring workspace, which will have `top` launched into it
  {
    key: u
    mods: CTRL|SHIFT
    action: { SwitchToWorkspace: { name: monitoring, spawn: { args: [top] } } }
  }
  // Create a new workspace with a random name and switch to it
  { key: i, mods: CTRL|SHIFT, action: SwitchToWorkspace }
  // Show the launcher in fuzzy selection mode and have it list all workspaces
  // and allow activating one.
  { key: "9", mods: ALT, action: { ShowLauncherArgs: { flags: "FUZZY|WORKSPACES" } } }
]
```

## Prompting for the workspace name

{{since('20230408-112425-69ae8472')}}

!!! danger "Non-functional: required a scripting callback"

    The original version of this example used `PromptInputLine` with a
    `wezterm.action_callback(...)` to take the entered name and switch to a
    newly-named workspace. `PromptInputLine`'s callback mechanism no longer
    works (see [PromptInputLine](PromptInputLine.md)), so this specific
    "prompt then switch" flow currently has no working equivalent.

