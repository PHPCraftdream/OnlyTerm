## `window.spawn_tab(args)`

{{since('20220624-141144-bd1b7c5d')}}

Spawns a program into a new tab within this window, returning a 3-element
array holding the [MuxTab](../MuxTab/index.md), [Pane](../pane/index.md) and
[MuxWindow](index.md) objects associated with it, in that order:

```rhai
let result = window.spawn_tab()
let tab = result[0]
let pane = result[1]
let spawned_window = result[2]
```

When called with no arguments, the default program is spawned.

The following parameters are supported:

### args

Specifies the argument array for the command that should be spawned.
If omitted the default program for the domain will be spawned.

```rhai
window.spawn_tab(#{ args: [ "top" ] })
```

### cwd

Specify the current working directory that should be used for
the program.

If unspecified, follows the rules from [default_cwd](../config/default_cwd.md)

```rhai
window.spawn_tab(#{ cwd: "/tmp" })
```

### set_environment_variables

Sets additional environment variables in the environment for
this command invocation.

```rhai
window.spawn_tab(#{ set_environment_variables: #{ FOO: "BAR" } })
```

### domain

Specifies the multiplexer domain into which the program should
be spawned.  The default value is assumed to be `"CurrentPaneDomain"`,
which causes the domain from the currently active pane to be used.

You may specify the name of one of the multiplexer domains
defined in your configuration using the following:

```rhai
window.spawn_tab(#{ domain: #{ DomainName: "my.name" } })
```
