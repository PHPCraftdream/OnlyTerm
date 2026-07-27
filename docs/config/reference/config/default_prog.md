---
tags:
  - spawn
---
# `default_prog`

If no `prog` is specified on the command line, use this
instead of running the user's shell.

For example, to have `wezterm` always run `top` by default,
you'd use this:

```rhai
config.default_prog = [ "top" ]
```

`default_prog` is implemented as an array where the 0th element
is the command to run and the rest of the elements are passed
as the positional arguments to that command.

On Windows, OnlyTerm defaults `default_prog` to `["cmd.exe"]` so that new
panes/tabs launch Command Prompt rather than whatever shell the ambient
`ComSpec` environment variable happens to point at. Set `default_prog`
explicitly (eg: to `["pwsh.exe"]`) if you'd prefer PowerShell.

See also: [Launching Programs](../../launch.md)
