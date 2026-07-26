!!! note
    *multiplexing is still a young feature and is evolving rapidly.  Your feedback is welcomed!*

## Multiplexing

The out-of-the-box experience with OnlyTerm allows you to multiplex local tabs
and windows which will persist until they are closed.  With a little extra
configuration you can enable local terminal multiplexing with features similar
to those in [tmux](https://github.com/tmux/tmux/wiki) or [screen](https://en.wikipedia.org/wiki/GNU_Screen).

Multiplexing in OnlyTerm is based around the concept of *multiplexing domains*;
a domain is a distinct set of windows and tabs.  When OnlyTerm starts up it
creates a default *local domain* to manage the windows and tabs in the UI, but it
can also be configured to start or connect to additional domains.

Once connected to a domain, OnlyTerm can attach its windows and tabs to the
local native UI, providing a more natural experience for interacting with
the mouse, clipboard and scrollback features of the terminal.

Key bindings allow you to spawn new tabs in the default local domain,
the domain of the current tab, or a specific numbered domain.

## Unix Domains

A connection to a multiplexer made via a unix socket is referred to
as a *unix domain*.  Unix domains are supported on all systems,
[even Windows](https://devblogs.microsoft.com/commandline/af_unix-comes-to-windows/)
and are a way to connect the native win32 GUI into the Windows Subsystem for Linux (WSL).

The bare minimum configuration to enable a unix domain is this, which will
spawn a server if needed and then connect the gui to it automatically
when OnlyTerm is launched:

```lua
config.unix_domains = {
  {
    name = 'unix',
  },
}

-- This causes OnlyTerm to act as though it was started as
-- `onlyterm connect unix` by default, connecting to the unix
-- domain on startup.
-- If you prefer to connect manually, leave out this line.
config.default_gui_startup_args = { 'connect', 'unix' }
```

If you prefer to connect manually, omit the `default_gui_startup_args` setting
and then run:

```console
$ onlyterm connect unix
```

Note that in earlier versions of WezTerm, a `connect_automatically` domain
option was shown as the way to connect on startup.  Using
`default_gui_startup_args` is recommended instead as it works more reliably.

The possible configuration values are:

```lua
config.unix_domains = {
  {
    -- The name; must be unique amongst all domains
    name = 'unix',

    -- The path to the socket.  If unspecified, a reasonable default
    -- value will be computed.

    -- socket_path = "/some/path",

    -- If true, do not attempt to start this server if we try and fail to
    -- connect to it.

    -- no_serve_automatically = false,

    -- If true, bypass checking for secure ownership of the
    -- socket_path.  This is not recommended on a multi-user
    -- system, but is useful for example when running the
    -- server inside a WSL container but with the socket
    -- on the host NTFS volume.

    -- skip_permissions_check = false,
  },
}
```

{{since('20220101-133340-7edc5b5a')}}

It is now possible to specify a `proxy_command` that will be used
in place of making a direct unix connection.  When `proxy_command`
is specified, it will be used instead of the optional `socket_path`.

This example shows a redundant use of `nc` (netcat) to connect to
the unix socket path on my mac.  This isn't useful on its own,
but may help with the WSL 2 issue mentioned below when translated
to an appropriate invocation of netcat/socat on Windows:

```lua
config.unix_domains = {
  {
    name = 'unix',
    proxy_command = { 'nc', '-U', '/Users/wez/.local/share/wezterm/sock' },
  },
}
```

{{since('20220319-142410-0fcdea07')}}

You may now specify the round-trip latency threshold for enabling predictive
local echo using `local_echo_threshold_ms`. If the measured round-trip latency
between the OnlyTerm client and the server exceeds the specified threshold, the
client will attempt to predict the server's response to key events and echo the
result of that prediction locally without waiting, hence hiding latency to the
user. This option only applies when `multiplexing = "WezTerm"`.

```lua
config.unix_domains = {
  {
    name = 'unix',
    local_echo_threshold_ms = 10,
  },
}
```

### Connecting into Windows Subsystem for Linux

*Note: this only works with WSL 1. [WSL 2 doesn't support AF_UNIX interop](https://github.com/microsoft/WSL/issues/5961)*

Inside your WSL instance, configure `.wezterm.lua` with this snippet:

```lua
config.unix_domains = {
  {
    name = 'wsl',
    -- Override the default path to match the default on the host win32
    -- filesystem.  This will allow the host to connect into the WSL
    -- container.
    socket_path = '/mnt/c/Users/USERNAME/.local/share/wezterm/sock',
    -- NTFS permissions will always be "wrong", so skip that check
    skip_permissions_check = true,
  },
}
```

In the host win32 configuration, use this snippet:

```lua
config.unix_domains = {
  {
    name = 'wsl',
    serve_command = { 'wsl', 'onlyterm-mux-server', '--daemonize' },
  },
}
config.default_gui_startup_args = { 'connect', 'wsl' }
```

Now when you start OnlyTerm you'll be presented with a WSL tab.

You can also omit `default_gui_startup_args` and use:

```console
$ onlyterm connect wsl
```

to manually connect into your WSL instance.
