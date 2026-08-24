---
tags:
  - event
---
# `default_gui_startup_args = [start]`

{{since('20220101-133340-7edc5b5a')}}

When launching the GUI using either `onlyterm` or `onlyterm-gui` (with no
subcommand explicitly specified), onlyterm will use the value of
`default_gui_startup_args` to pick a default mode for running the GUI.

The default for this config is `[start]` which makes `onlyterm` with no
additional subcommand arguments equivalent to `onlyterm start`.

If you know that you always want to connect to a particular multiplexer
domain, then you might consider using this configuration:

```
default_gui_startup_args: [ connect, some-domain ]
```

which will cause `onlyterm` with no additional subcommand arguments to be
equivalent to running `onlyterm connect some-domain`.

Specifying subcommand arguments on the command line is NOT additive with
this config; the command line arguments always take precedence.

Depending on your desktop environment, you may find it simpler to use
your operating system shortcut or alias function to set up a shortcut
that runs the subcommand you desire.
