!!! note
    *multiplexing is still a young feature and is evolving rapidly.  Your feedback is welcomed!*

## Multiplexing

The out-of-the-box experience with OnlyTerm allows you to multiplex local tabs
and windows which will persist until they are closed.  With a little extra
configuration you can enable local terminal multiplexing with features similar
to those in [tmux](https://github.com/tmux/tmux/wiki) or [screen](https://en.wikipedia.org/wiki/GNU_Screen).

Multiplexing in OnlyTerm is based around the concept of *multiplexing domains*;
a domain is a distinct set of windows and tabs.  When OnlyTerm starts up it
creates a default *local domain* to manage the windows and tabs in the UI, but it
can also be configured to start or connect to additional domains.

Once connected to a domain, OnlyTerm can attach its windows and tabs to the
local native UI, providing a more natural experience for interacting with
the mouse, clipboard and scrollback features of the terminal.

Key bindings allow you to spawn new tabs in the default local domain,
the domain of the current tab, or a specific numbered domain.

## Unix Domains

A connection to a multiplexer made via a unix socket is referred to
as a *unix domain*.  Unix domains are supported on all systems,
[even Windows](https://devblogs.microsoft.com/commandline/af_unix-comes-to-windows/)
and are a way to connect the native win32 GUI into the Windows Subsystem for Linux (WSL).

The bare minimum configuration to enable a unix domain is this, which will
spawn a server if needed and then connect the gui to it automatically
when OnlyTerm is launched:

```
unix_domains: [
  {
    name: unix
  }
]

// This causes OnlyTerm to act as though it was started as
// `onlyterm connect unix` by default, connecting to the unix
// domain on startup.
// If you prefer to connect manually, leave out this line.
default_gui_startup_args: [connect, unix]
```

If you prefer to connect manually, omit the `default_gui_startup_args` setting
and then run:

```console
$ onlyterm connect unix
```

Note that in earlier versions of WezTerm, a `connect_automatically` domain
option was shown as the way to connect on startup.  Using
`default_gui_startup_args` is recommended instead as it works more reliably.

The possible configuration values are:

```
unix_domains: [
  {
    // The name; must be unique amongst all domains
    name: unix

    // The path to the socket.  If unspecified, a reasonable default
    // value will be computed.

    // socket_path: "/some/path"

    // If true, do not attempt to start this server if we try and fail to
    // connect to it.

    // no_serve_automatically: false

    // If true, bypass checking for secure ownership of the
    // socket_path.  This is not recommended on a multi-user
    // system, but is useful for example when running the
    // server inside a WSL container but with the socket
    // on the host NTFS volume.

    // skip_permissions_check: false
  }
]
```

{{since('20220101-133340-7edc5b5a')}}

It is now possible to specify a `proxy_command` that will be used
in place of making a direct unix connection.  When `proxy_command`
is specified, it will be used instead of the optional `socket_path`.

This example shows a redundant use of `nc` (netcat) to connect to
the unix socket path on my mac.  This isn't useful on its own,
but may help with the WSL 2 issue mentioned below when translated
to an appropriate invocation of netcat/socat on Windows:

```
unix_domains: [
  {
    name: unix
    proxy_command: [nc, "-U", "/Users/wez/.local/share/wezterm/sock"]
  }
]
```

{{since('20220319-142410-0fcdea07')}}

You may now specify the round-trip latency threshold for enabling predictive
local echo using `local_echo_threshold_ms`. If the measured round-trip latency
between the OnlyTerm client and the server exceeds the specified threshold, the
client will attempt to predict the server's response to key events and echo the
result of that prediction locally without waiting, hence hiding latency to the
user. This option only applies when `multiplexing = "WezTerm"`.

```
unix_domains: [
  {
    name: unix
    local_echo_threshold_ms: 10
  }
]
```

### Connecting into Windows Subsystem for Linux

*Note: this only works with WSL 1. [WSL 2 doesn't support AF_UNIX interop](https://github.com/microsoft/WSL/issues/5961)*

Inside your WSL instance, configure `.onlyterm.ktav` with this snippet:

```
unix_domains: [
  {
    name: wsl
    // Override the default path to match the default on the host win32
    // filesystem.  This will allow the host to connect into the WSL
    // container.
    socket_path: "/mnt/c/Users/USERNAME/.local/share/wezterm/sock"
    // NTFS permissions will always be "wrong", so skip that check
    skip_permissions_check: true
  }
]
```

In the host win32 configuration, use this snippet:

```
unix_domains: [
  {
    name: wsl
    serve_command: [wsl, "onlyterm-mux-server", "--daemonize"]
  }
]
default_gui_startup_args: [connect, wsl]
```

Now when you start OnlyTerm you'll be presented with a WSL tab.

You can also omit `default_gui_startup_args` and use:

```console
$ onlyterm connect wsl
```

to manually connect into your WSL instance.

## Surviving a crashed or killed GUI process

The unix-domain multiplexing described above can be pointed at OnlyTerm's own
standalone `onlyterm-mux-server` daemon (`onlyterm-mux-server.exe` on Windows)
instead of at another GUI process's embedded mux. `onlyterm-mux-server` has no
window, no message loop and no GPU/renderer of any kind — it is a plain
headless daemon that owns the panes, tabs and windows and speaks the mux
protocol over a unix-domain socket. Running against it turns "GUI process
crashes" into a non-event for the programs running in your panes.

Put this in `~/.onlyterm.ktav` (`%USERPROFILE%\.onlyterm.ktav` on Windows):

```
unix_domains: [ { name: main, connect_automatically: true } ]
default_domain: main
default_gui_startup_args: [start, "--always-new-process"]
```

What each setting does in this recipe:

- `unix_domains: [ { name: main, connect_automatically: true } ]` declares
  a unix-domain named `"main"`. Because `socket_path` is left unset, it
  resolves to the default socket path under OnlyTerm's runtime directory
  (`$XDG_RUNTIME_DIR`/equivalent on Unix, or the per-user runtime dir on
  Windows) — the same default every GUI process and `onlyterm-mux-server`
  instance will agree on as long as they share the same config.
  `connect_automatically: true` makes any GUI process that starts up (via
  `onlyterm connect main`, or implicitly through `default_gui_startup_args`
  below) auto-attach to this domain instead of waiting to be told to.
- `default_domain: "main"` makes `"main"` the default multiplexing domain for
  new windows/tabs, instead of the built-in `"local"` domain (which spawns
  processes directly in the GUI process itself, with no survival guarantee).
- `default_gui_startup_args: ["start", "--always-new-process"]` makes every
  invocation of the GUI act like `onlyterm start --always-new-process`.
  `--always-new-process` is the important part: it stops the GUI from trying
  to find and reuse an already-running GUI window (see the gui-sock caveat
  below) and instead always starts a fresh GUI process that connects to the
  `main` unix domain (because that's now the default domain). If you launch
  the GUI twice, you get two independent GUI *processes*, each rendering
  windows attached to the *same* underlying panes/tabs owned by
  `onlyterm-mux-server`.

If no `onlyterm-mux-server` is already listening on the socket, the first GUI
process to connect will auto-spawn one in the background (unless you set
`no_serve_automatically: true` on the domain, or start it explicitly:
`onlyterm-mux-server.exe` with the same config file).

### What this buys you

Because the shells and programs in your panes live inside the separate
`onlyterm-mux-server` process rather than inside any particular GUI process:

- Killing or crashing one GUI window process (task-manager "End task", a
  segfault, an access violation, the process wedging and being force-closed)
  does **not** kill the programs running in its panes. They keep running,
  unattended, inside `onlyterm-mux-server`.
- The next time a GUI process connects to the `main` domain — either a fresh
  `onlyterm-gui` you launch by hand, or (if `connect_automatically` is set,
  as above) the next `onlyterm start` — it reattaches to the existing
  windows/tabs/panes and their scrollback, as if nothing happened.
- This is a strictly stronger guarantee than "one wedged window can be
  rebuilt or closed without killing the rest of *that same* process" (which
  is what the in-process renderer-rebuild/circuit-breaker work in this
  session, tasks #244.1-#244.9, already provides): here the *entire GUI
  process* can die and the panes still survive, because they were never
  owned by that process's address space to begin with.

### Honestly documented degradations

This topology is not free of rough edges. As of this session (verified by
reading the current source, not assumed from upstream WezTerm docs):

- **Blank/default tab titles and formatters.** `ClientPane` (the pane type
  used for panes that live in a remote mux domain — see
  `crates/wezterm-client/src/pane/clientpane.rs`) does not override
  `get_foreground_process_name` or `get_foreground_process_info`. Both fall
  back to the `Pane` trait's defaults (`crates/mux/src/pane.rs`), which
  simply return `None`. Anything in your tab title / status-bar formatter
  that reads the foreground process name (e.g. to show `nvim` or `cargo`
  instead of the shell name) will show blank/default output for panes
  reached through this unix-domain topology, even though the same formatter
  works for local panes. Confirmed by manual testing in this session: `cli
  list`'s `TITLE` column showed only the pane's initial title/CWD, never a
  foreground process name, for the entire life of the test panes.
- **Window tracking is per-GUI-process, not global.** Each GUI process
  tracks only the `known_windows` it created locally
  (`crates/wezterm-gui/src/frontend.rs`). A second GUI process attached to
  the same `main` domain has its own, disjoint set of known windows, and
  (now that the scripting engine that used to expose a
  `wezterm.gui.gui_windows()` function is gone) there is no way to enumerate
  windows from config at all. To see "every window across every GUI process
  attached to this mux-server" you need to go through `wezterm cli list`
  (which talks to the mux-server directly) instead.
- **No automatic reconnect if `onlyterm-mux-server` itself dies.**
  `Reconnectable::reconnectable()` for a unix-domain `ClientDomain`
  (`crates/wezterm-client/src/client.rs`) unconditionally returns `false`,
  with a comment explaining why: reconnecting to a *respawned* unix socket
  server wouldn't preserve the original set of tabs anyway, so silently
  retrying would just produce confusing, inconsistent state. If the daemon
  process itself is killed (not just a GUI process), your panes are gone;
  GUI processes attached to it will show the domain as detached/dead rather
  than silently spinning up a replacement behind your back.
- **`wezterm cli` from outside any pane needs the right config to find the
  socket.** When `wezterm cli` runs from a plain external shell (not from
  inside a running OnlyTerm session), it resolves its target unix domain via
  `Client::compute_unix_domain` (`crates/wezterm-client/src/client.rs`):
  1. If `$ONLYTERM_UNIX_SOCKET` is set in the environment, that path is used
     directly, bypassing everything else.
  2. Otherwise, unless `--prefer-mux` forces past this step, it tries to
     resolve a **published gui-sock** — the location of a currently-running
     GUI process's own embedded mux (see `Publish::resolve` in
     `crates/wezterm-gui/src/main.rs`).
  3. Otherwise it falls back to `config.unix_domains.first()` — i.e. the
     `main` domain above, using its default socket path.

  **With this exact recipe, step 2 never finds anything, because it's never
  published in the first place.** `Publish::resolve` refuses to publish a
  gui-sock whenever `config.default_domain` is not `"local"`
  (`mux.default_domain().domain_name() != config.default_domain`), which is
  true here since `default_domain = "main"`; it *also* refuses whenever
  `always_new_process` is set, which is redundant in this recipe but matters
  if you ever drop the `default_domain` override. Confirmed by manual test
  in this session: after starting a GUI process under this config, no
  `gui-sock-<pid>` file appeared in the runtime directory. Practically, this
  means `wezterm cli` invoked from an external shell always falls through to
  step 3 (the `main` unix domain's default socket path) as long as it loads
  the *same* config file — which is exactly what you want for this
  topology, but relies on the CLI and the GUI/mux-server agreeing on config
  resolution (same `--config-file`, or the same default config path).

### Manual smoke-test recipe

To verify the topology end-to-end yourself:

1. Save the config snippet above to a config file, e.g.
   `smoke.onlyterm.ktav`.
2. Start (or let auto-spawn) the daemon:
   `onlyterm-mux-server.exe --config-file smoke.onlyterm.ktav`
3. Launch a GUI process attached to it:
   `onlyterm-gui.exe --config-file smoke.onlyterm.ktav start --always-new-process`
4. In the resulting window's shell, run something identifiable, e.g.
   `echo hello-from-pane-1`.
5. From another shell, confirm the mux-server sees it:
   `onlyterm.exe --config-file smoke.onlyterm.ktav cli --no-auto-start get-text --pane-id 0`
   should show the echoed text.
6. Find the GUI process's PID (e.g. via Task Manager or `tasklist`) and kill
   it directly and ungracefully (`taskkill /F /PID <pid>` on Windows,
   `kill -9 <pid>` on Unix) rather than closing the window normally.
7. Confirm the daemon is still running and the pane content is still there:
   re-run the `cli get-text` command from step 5 — it should still show
   `hello-from-pane-1`, even though no GUI process is attached at all.
8. Launch a fresh GUI process (step 3 again) and confirm it reattaches to
   the same pane with scrollback intact, rather than starting a blank shell.

**Smoke test results (this session, Windows, debug build):** all eight steps
were carried out against the real `onlyterm-mux-server.exe` / `onlyterm-gui.exe`
/ `onlyterm.exe` binaries, using two concurrently-attached GUI processes. Text
written to the pane before killing a GUI process (including a hard
`taskkill /F` on the *only* remaining attached GUI process, leaving zero GUIs
attached) remained visible via `cli get-text` throughout, and a subsequently
launched fresh GUI process, as well as `cli`, both reattached to the original
pane with its scrollback intact. The mux-server process itself was
unaffected by any GUI process being killed. The topology works as described
above.
