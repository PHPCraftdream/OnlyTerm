---
tags:
  - multiplexing
---
# `mux_enable_ssh_agent = true`

!!! danger "Currently unused"

    This setting used to make onlyterm set the `SSH_AUTH_SOCK` environment
    variable of every pane to a symbolic link that followed the
    authentication socket of the most recently active multiplexer client.
    That only works for forwarded Unix agent sockets. This Windows build has
    none, and the link pointed at nothing, so the feature was removed.
    The option is still accepted in a config file (so old configs that set it
    don't fail to load) and produces a deprecation warning, but nothing reads
    it.

Panes no longer get a proxied `SSH_AUTH_SOCK`. Windows' OpenSSH agent is
reached through a named pipe and needs no environment variable.
