## `mux::spawn_window(args)`

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

{{since('20220624-141144-bd1b7c5d')}}

Spawns a program into a new window, returning a 3-element array holding the
[MuxTab](../MuxTab/index.md), [Pane](../pane/index.md) and
[MuxWindow](../mux-window/index.md) objects associated with it, in that order:

```rhai
let result = mux::spawn_window()
let tab = result[0]
let pane = result[1]
let window = result[2]
```

When called with no arguments, the default program is spawned.

The following parameters are supported:

### args

Specifies the argument array for the command that should be spawned.
If omitted the default program for the domain will be spawned.

```rhai
mux::spawn_window(#{ args: [ "top" ] })
```

### cwd

Specify the current working directory that should be used for
the program.

If unspecified, follows the rules from [default_cwd](../config/default_cwd.md)

```rhai
mux::spawn_window(#{ cwd: "/tmp" })
```

### set_environment_variables

Sets additional environment variables in the environment for
this command invocation.

```rhai
mux::spawn_window(#{ set_environment_variables: #{ FOO: "BAR" } })
```

### domain

Specifies the multiplexer domain into which the program should
be spawned.  The default value is assumed to be `"DefaultDomain"`,
which causes the default domain to be used.

You may specify the name of one of the multiplexer domains
defined in your configuration using the following:

```rhai
mux::spawn_window(#{ domain: #{ DomainName: "my.name" } })
```

### width and height

Only valid when width and height are used together, allows specifying
the number of column and row cells that the window should have.

```rhai
mux::spawn_window(#{ width: 60, height: 30 })
```

### workspace

Specifies the name of the workspace that the newly created window
will be associated with.  If omitted, the currently active workspace
name will be used.

```rhai
mux::spawn_window(#{ workspace: "coding" })
```

### position

{{since('20230320-124340-559cb7b0')}}

Specify the initial position for the GUI window that will be created to display
this mux window.

The value is a rhai map:

```
mux::spawn_window(#{
  position: #{
    x: 10,
    y: 300,
    // Optional origin to use for x and y.
    // Possible values:
    // * "ScreenCoordinateSystem" (this is the default)
    // * "MainScreen" (the primary or main screen)
    // * "ActiveScreen" (whichever screen hosts the active/focused window)
    // * #{ Named: "HDMI-1" } - uses a screen by name. See gui::screens()
    // origin: "ScreenCoordinateSystem"
  },
})
```

See also [gui::screens()](../onlyterm.gui/screens.md)
