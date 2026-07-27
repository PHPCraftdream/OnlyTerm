---
title: wezterm.run_child_process
tags:
 - utility
 - open
 - spawn
---
# `wezterm.run_child_process(args)`

{{since('20200503-171512-b13ef15f')}}

This function accepts an argument list; it will attempt to spawn that command
and will return a 3-element array consisting of the boolean success of the
invocation, the stdout data and the stderr data.

```rhai
let result = run_child_process([ "ls", "-l" ])
let success = result[0]
let stdout = result[1]
let stderr = result[2]
```

See also [background_child_process](background_child_process.md)
