---
tags:
  - tuning
---
# `child_process_timeout_ms = 3000`

!!! danger "Currently unused: its only consumer was `run_child_process`"

    This setting bounded how long, in milliseconds, the scripting function
    `run_child_process` could block waiting on a child process before giving
    up. `run_child_process` was formerly used from `format-tab-title`,
    `format-window-title` and `update-status` event callbacks to shell out
    to an external command (`git`, `kubectl`, etc.) for a status-bar or
    tab-title fragment. `run_child_process`, those callbacks, and the
    scripting engine that ran them have all been removed — see the
    [changelog](../../../changelog.md#continuousnightly). `child_process_timeout_ms`
    is still accepted in a config file (so old configs that set it don't
    fail to load), but nothing currently reads it.

Historically: bounds how long, in milliseconds, `run_child_process` may
block waiting on a child process before giving up. When the timeout
elapsed, the child process was killed (not left running as an orphan)
instead of allowed to hang indefinitely.

Defaults to `3000` (3 seconds). Set to `0` to disable the timeout and wait
indefinitely, restoring the historical (pre-timeout) behavior.

```
child_process_timeout_ms: 3000
```
