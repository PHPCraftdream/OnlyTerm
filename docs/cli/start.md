# `onlyterm start`

```console
{% include "../examples/cmd-synopsis-onlyterm-start--help.txt" %}
```

## `--start-conf`: opening a fixed set of tabs at startup

`--start-conf <path.ktav>` loads a startup layout instead of spawning the
single tab this command would otherwise open. The file is plain ktav data
(the same format as `.onlyterm.ktav`, but parsed standalone -- it is not
merged with your regular config) shaped like this:

```
## root_dir is where a tab's shell starts. A relative path (like
## "logs" on the "logs" tab below) is resolved against the directory
## this layout file itself lives in, NOT against wherever `onlyterm
## --start-conf` happens to be run from -- so a layout file checked
## into a project keeps working no matter where you launch it from.
## Set here, it's the default for every tab; a tab's own root_dir
## wins over this one.
root_dir: C:\src\my-project

## Applied to every tab, before that tab's own vars/commands. A key
## present in both a global and a per-tab `vars` is won by the tab.
vars: {
  MY_PROJECT_ROOT: C:\src\my-project
}

commands: [
  echo Welcome!
]

tabs: [
  {
    title: editor
  }
  {
    title: server
    vars: {
      PORT: 8080
    }
    commands: [
      npm run dev
    ]
  }
  {
    title: logs
    ## Relative to this file's own directory, e.g. C:\src\my-project\logs
    ## if this file lives at C:\src\my-project\start.ktav -- overrides the
    ## root_dir set at the top of the file for this one tab.
    root_dir: logs
  }
]
```

All tabs from `tabs` are opened in a single new window, in the order
listed. `root_dir`/`vars`/`commands` at the top level apply to every tab
that doesn't set its own; a tab's own value always wins over the top-level
one (for `vars` this is a per-key merge, not all-or-nothing: a key set at
both levels takes the tab's value, other keys from the top level still
apply). `commands` are concatenated instead of overridden: the top-level
list runs first, then the tab's own. Neither `root_dir` is required --
without either one, a tab falls back to the normal
[default_cwd](../config/reference/config/default_cwd.md)/domain default,
same as not passing `--start-conf` at all. `commands` are "typed" into the
tab's shell immediately after it starts -- there is no prompt-readiness
detection, so a command that depends on a slow-starting shell profile may
need to be preceded by something that waits, or just accept that it's
queued input the shell will process once it's ready. `title` is optional;
a tab without one keeps its normal automatic title. At least one entry in
`tabs` is required.

`--start-conf` is mutually exclusive with `PROG`/`--cwd`, since the layout
file specifies its own per-tab program/commands and working directory
instead.
