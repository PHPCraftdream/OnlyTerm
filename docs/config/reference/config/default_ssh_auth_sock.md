---
tags:
  - multiplexing
  - ssh
---
# `default_ssh_auth_sock`

{{since('nightly')}}

Setting this value will cause wezterm to replace the value of the
`SSH_AUTH_SOCK` environment when it first starts up, and to use this value for
the auth socket registered with the multiplexer server (visible via `wezterm
cli list-clients`).

You won't normally need to set this, but if you are running with an alternative
identity agent and want to replace the default on your system, this gives
you that ability.

For example, @wez currently uses the 1Password SSH Auth Agent, but when
running on Gnome the system default is Gnome's keyring agent.

While you can fix this up in your shell startup files, those are not involved
when spawning the GUI directly from the desktop environment.

A simple example that unconditionally points at the 1Password SSH agent
socket:

```
default_ssh_auth_sock: "/home/you/.1password/agent.sock"
```

!!! note "No conditional logic in ktav"

    An earlier version of this example detected, at config-load time,
    whether gnome-keyring's ssh-agent socket was the current
    `SSH_AUTH_SOCK` and conditionally substituted the 1Password agent's
    socket only if it existed, using Lua's `if`/`os.getenv`/`wezterm.glob`.
    ktav is a static data format with no conditional expressions, no
    environment variable lookups, and no filesystem globbing at config-load
    time, so that kind of environment-dependent, self-adjusting logic can no
    longer be expressed in the config file itself. If you need
    machine-specific values, maintain separate `.ktav` files per machine (see
    [Configuration Files](../../files.md)) rather than branching inside one
    file.

