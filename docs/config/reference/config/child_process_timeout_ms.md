---
tags:
  - tuning
---
# `child_process_timeout_ms = 3000`

Bounds how long, in milliseconds, `run_child_process` may block waiting on
a child process before giving up.

`run_child_process` is commonly used from `format-tab-title`,
`format-window-title` and `update-status` event callbacks to shell out to
an external command (`git`, `kubectl`, etc.) for a status-bar or tab-title
fragment. Those callbacks run synchronously on the GUI thread -- there is
no async execution model for rhai/lua event callbacks -- so if the spawned
command hangs (a stuck `git` invocation against a stalled network mount, a
wedged `wsl.exe`, and so on), every window in the process would otherwise
freeze forever with no recovery path.

When the timeout elapses, the child process is killed (it is not left
running as an orphan) and `run_child_process` returns an error to the
calling script instead of hanging.

Defaults to `3000` (3 seconds), which is comfortably above how long a
legitimate status-bar refresh command should normally take, while still
being far short of the point where the user would notice the window has
frozen.

Set to `0` to disable the timeout and wait indefinitely, restoring the
historical (pre-timeout) behavior.

```lua
config.child_process_timeout_ms = 3000
```
