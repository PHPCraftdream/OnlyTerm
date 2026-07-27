# `SpawnCommandInNewWindow`

Spawn a new tab into a brand new window.
The argument is a `SpawnCommand` struct that is discussed in more
detail in the [SpawnCommand](../SpawnCommand.md) docs.

```rhai
config.keys = [
  // CMD-y starts `top` in a new window
  #{
    key: "y",
    mods: "CMD",
    action: act.SpawnCommandInNewWindow(#{
      args: [ "top" ],
    }),
  },
]
```


