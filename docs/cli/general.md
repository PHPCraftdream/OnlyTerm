# Command Line

This section documents the onlyterm command line.

*Note that `onlyterm --help` or `onlyterm SUBCOMMAND --help` will show the precise
set of options that are applicable to your installed version of onlyterm.*

onlyterm is deployed with two major executables:

* `onlyterm` (or `onlyterm.exe` on Windows) - for interacting with onlyterm from the terminal
* `onlyterm-gui` (or `onlyterm-gui.exe` on Windows) - for spawning onlyterm from a desktop environment

You will typically use `onlyterm` when scripting onlyterm; it knows when to
delegate to `onlyterm-gui` under the covers.

If you are setting up a launcher for onlyterm to run in the Windows GUI
environment then you will want to explicitly target `onlyterm-gui` so that
Windows itself doesn't pop up a console host for its logging output.

!!! note
    `onlyterm-gui.exe --help` will not output anything to a console when
    run on Windows systems, because it runs in the Windows GUI subsystem and has no
    connection to the console.  You can use `onlyterm.exe --help` to see information
    about the various commands; it will delegate to `onlyterm-gui.exe` when
    appropriate.

## Synopsis

```console
{% include "../examples/cmd-synopsis-onlyterm--help.txt" %}
```
