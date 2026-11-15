---
tags:
  - multiplexing
---
# `default_ssh_auth_sock`

!!! danger "Currently unused"

    This setting used to replace the `SSH_AUTH_SOCK` environment variable
    when onlyterm started, and was the socket registered with the
    multiplexer server for the agent proxy (see
    [mux_enable_ssh_agent](mux_enable_ssh_agent.md)), which has been removed.
    The option is still accepted in a config file (so old configs that set it
    don't fail to load) and produces a deprecation warning, but nothing reads
    it.
