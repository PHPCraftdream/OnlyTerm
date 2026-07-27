# `SpawnCommandInNewTab`

Spawn a new tab into the current window.
The argument is a `SpawnCommand` struct that is discussed in more
detail in the [SpawnCommand](../SpawnCommand.md) docs.

```rhai
config.keys = [
  // CMD-y starts `top` in a new tab
  #{
    key: "y",
    mods: "CMD",
    action: act.SpawnCommandInNewTab(#{
      args: [ "top" ],
    }),
  },
]
```


