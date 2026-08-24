---
tags:
  - multiplexing
---
# `default_domain = "local"`

{{since('20220319-142410-0fcdea07')}}

!!! note
    This option only applies to the GUI.  For the equivalent option in
    the standalone mux server, see [default_mux_server_domain](default_mux_server_domain.md)

When starting the GUI (not using the `serial` or `connect` subcommands), by default onlyterm will set the built-in `"local"` domain as the default multiplexing domain.

The `"local"` domain represents processes that are spawned directly on the local system.

`default_domain` will accept the name of any of the available
[multiplexing domains](../../../multiplexing.md).

!!! note
    WSL domain support has been removed from this fork: it required
    enumerating distributions, which added
    measurable startup latency even for users who never touch WSL. To
    launch a WSL distribution's shell, invoke it directly as your
    `default_prog`/`SpawnCommand` instead.
