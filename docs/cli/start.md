# `onlyterm start`

```console
{% include "../examples/cmd-synopsis-onlyterm-start--help.txt" %}
```

## Examples

Options that apply to the whole program go **before** `start`.

```console
$ onlyterm start                                  # your default shell
$ onlyterm start -- powershell.exe -NoLogo        # a program instead of the shell
$ onlyterm start --cwd C:/work                    # initial directory
$ onlyterm start --choose-tab --cwd C:/work       # pick shell / elevation / priority first
$ onlyterm start --start-conf project.onlyterm-run  # a startup layout
$ onlyterm start --workspace dev                  # open in the "dev" workspace
$ onlyterm start --position 100,100               # initial window position
$ onlyterm start --always-new-process             # do not reuse a running GUI process
$ onlyterm --config font_size=14 start            # override a setting
$ onlyterm --config-file D:/cfg/other.ktav start  # another config file
$ onlyterm --skip-config start                    # built-in defaults only
```

See [Launching OnlyTerm](../launching.md) for the Explorer entries and the
`--choose-tab` dialog.

## `--start-conf`: opening a fixed set of tabs at startup

`--start-conf <path.ktav>` loads a startup layout instead of spawning the
single tab this command would otherwise open. The file is plain ktav data
(the same format as `.onlyterm.ktav`, but parsed standalone -- it is not
merged with your regular config). Tabs open in the listed order; once startup
finishes, the first tab is active. This does not change which tab is activated
when you open additional tabs normally.

The layout is shaped like this:

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
      NODE_ENV: development
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

### Run files: opening a layout from Explorer

The installer registers the `.onlyterm-run` file type. A `*.onlyterm-run` file is
an ordinary `--start-conf` layout; opening it in Explorer (double click, Enter)
runs `onlyterm-gui start --start-conf <file>`.

OnlyTerm keeps a folder for such files, `.onlyterm-run`, beside the config file
(`%USERPROFILE%\.onlyterm-run` for the usual `%USERPROFILE%\.onlyterm.ktav`). It
is created the first time the GUI starts.

### Folder settings

Right-clicking a folder, or the empty space inside one, offers
**OnlyTerm folder settings**. It creates that folder's run file in the `.onlyterm-run`
folder (named after the folder's path, e.g. `D--dev-myproj-1a2b3c4d.onlyterm-run`)
if there is none yet, and opens it in Notepad++ when that is installed, otherwise
in Notepad. From then on **Open OnlyTerm here**
in that folder starts the layout from this file instead of a single tab.

New files are made from `.onlyterm-run\template.ktav`, which is created with a
one-tab default and can be edited. In it `${LAUNCH_DIR}` stands for the folder
the file is created for. A path after it is resolved when the file is made:
`${LAUNCH_DIR}\..\shared` becomes `D:\dev\shared` for `D:\dev\myproj` (`..`
goes one folder up, `.` is dropped). Editing the template does not change files
that already exist.

`--start-conf` is mutually exclusive with `PROG`/`--cwd`, since the layout
file specifies its own per-tab program/commands and working directory
instead.
