//! GUI, mux and configured-tab startup.
use super::*;

fn have_panes_in_domain_and_ws(domain: &Arc<dyn Domain>, workspace: &Option<String>) -> bool {
    let mux = Mux::get();
    let have_panes_in_domain = mux
        .iter_panes()
        .iter()
        .any(|p| p.domain_id() == domain.domain_id());

    if !have_panes_in_domain {
        return false;
    }

    if let Some(ws) = &workspace {
        for window_id in mux.iter_windows_in_workspace(ws) {
            if let Some(win) = mux.get_window(window_id) {
                for t in win.iter() {
                    for p in t.iter_panes_ignoring_zoom() {
                        if p.pane.domain_id() == domain.domain_id() {
                            return true;
                        }
                    }
                }
            }
        }
        false
    } else {
        true
    }
}

async fn spawn_tab_in_domain_if_mux_is_empty(
    cmd: Option<CommandBuilder>,
    is_connecting: bool,
    domain: Option<Arc<dyn Domain>>,
    workspace: Option<String>,
) -> anyhow::Result<()> {
    let mux = Mux::get();

    let domain = domain.unwrap_or_else(|| mux.default_domain());

    if !is_connecting && have_panes_in_domain_and_ws(&domain, &workspace) {
        return Ok(());
    }

    let window_id = {
        // Force the builder to notify the frontend early,
        // so that the attach await below doesn't block it.
        // This has the consequence of creating the window
        // at the initial size instead of populating it
        // from the size specified in the remote mux.
        // We use the TabAddedToWindow mux notification
        // to detect and adjust the size later on.
        let position = None;
        let builder = mux.new_empty_window(workspace.clone(), position);
        *builder
    };

    let config = config::configuration();
    config.update_ulimit()?;

    domain.attach(Some(window_id)).await?;

    if have_panes_in_domain_and_ws(&domain, &workspace) {
        return Ok(());
    }

    let _config_subscription = config::subscribe_to_config_reload(move || {
        promise::spawn::spawn_into_main_thread(async move {
            if let Err(err) = update_mux_domains(&config::configuration()) {
                log::error!("Error updating mux domains: {:#}", err);
            }
        })
        .detach();
        true
    });

    let dpi = config.dpi.unwrap_or_else(::window::default_dpi);
    let _tab = domain
        .spawn(
            config.initial_size(dpi as u32, Some(cell_pixel_dims(&config, dpi)?)),
            cmd,
            None,
            window_id,
        )
        .await?;
    Ok(())
}

/// Spawns every tab described by a `--start-conf` layout into a single new
/// window, in order. Each tab's environment is `layout.vars` overlaid with
/// that tab's own `vars` (a key present in both is won by the tab), and
/// after the tab's shell starts, `layout.commands` followed by the tab's own
/// `commands` are "typed" into it via `Pane::writer()` -- the same
/// immediate, not-prompt-aware write used for the "open dropped file in a
/// new tab" path (see `frontend.rs`'s drag-and-drop handling).
async fn spawn_startup_layout(
    layout: &config::StartupLayout,
    domain: Option<Arc<dyn Domain>>,
    workspace: Option<String>,
) -> anyhow::Result<()> {
    let layout = Arc::new(layout.clone());
    let mux = Mux::get();
    let domain = domain.unwrap_or_else(|| mux.default_domain());

    let window_id = {
        // See spawn_tab_in_domain_if_mux_is_empty: force the builder to
        // notify the frontend early, so the attach await below doesn't
        // block it.
        let position = None;
        let builder = mux.new_empty_window(workspace, position);
        *builder
    };

    let config = config::configuration();
    config.update_ulimit()?;

    domain.attach(Some(window_id)).await?;

    let dpi = config.dpi.unwrap_or_else(::window::default_dpi);
    let size = config.initial_size(dpi as u32, Some(cell_pixel_dims(&config, dpi)?));

    const NON_ELEVATED_STARTUP_CONCURRENCY: usize = 4;
    let mut pending_non_elevated = Vec::new();
    let mut previous_isolated = Vec::new();
    for (idx, tab_conf) in layout.tabs.iter().enumerate() {
        let options = layout.tab_options(tab_conf);
        let startup_order = if !options.admin && config.per_tab_process_isolation {
            let (order, done) = crate::spawn::StartupOrder::after(&previous_isolated);
            previous_isolated.push(done);
            Some(order)
        } else {
            None
        };
        if !options.admin {
            if config.per_tab_process_isolation {
                let layout = Arc::clone(&layout);
                let domain = domain.clone();
                let config = config.clone();
                pending_non_elevated.push(promise::spawn::spawn(async move {
                    spawn_startup_non_elevated_tab(
                        &layout,
                        idx,
                        domain,
                        window_id,
                        size,
                        config,
                        startup_order,
                    )
                    .await
                }));
                if pending_non_elevated.len() == NON_ELEVATED_STARTUP_CONCURRENCY {
                    for result in futures::future::join_all(pending_non_elevated.drain(..)).await {
                        result?;
                    }
                    previous_isolated.clear();
                }
            } else {
                // `Domain::spawn` materializes its tab as part of the call;
                // there is no reserve/commit API, so concurrent calls could
                // reorder visible tabs when remote process startup completes
                // out of order. Keep this path ordered; isolated tabs use
                // prepare/ordered-commit above to overlap the expensive work.
                spawn_startup_non_elevated_tab(
                    &layout,
                    idx,
                    domain.clone(),
                    window_id,
                    size,
                    config.clone(),
                    None,
                )
                .await?;
            }
            continue;
        }

        if config.per_tab_process_isolation {
            for result in futures::future::join_all(pending_non_elevated.drain(..)).await {
                result?;
            }
            if options.admin {
                previous_isolated.clear();
            }
        }

        // Elevated startup tabs intentionally remain ordered: each one may
        // display a UAC prompt and the next prompt must not race it.
        for result in futures::future::join_all(pending_non_elevated.drain(..)).await {
            result?;
        }
        // A tab's own `root_dir` wins over the layout-wide one, which in
        // turn wins over `config.default_cwd` that `build_prog` applies --
        // the same override order `run_terminal_gui` already uses for the
        // plain `--cwd` flag vs. `config.default_cwd`.
        let cwd = tab_conf.root_dir.as_ref().or(layout.root_dir.as_ref());
        let shell_argv = options.shell.map(|shell| shell.argv());

        let tab = {
            // Elevation can only go through the hosting-process path, which
            // takes an argv rather than a `CommandBuilder`. It also cannot
            // carry this tab's `vars`: the elevated child is launched by
            // `ShellExecuteExW`, which gives no way to hand it an
            // environment block, so say so rather than silently dropping
            // them.
            if !layout.vars.is_empty() || !tab_conf.vars.is_empty() {
                log::warn!(
                    "--start-conf: tab {} is `admin: true`, so its environment \
                     variables cannot be applied -- elevated tabs are launched \
                     via ShellExecuteExW, which cannot pass an environment",
                    idx + 1
                );
            }
            let argv = shell_argv.unwrap_or_else(|| {
                config
                    .default_prog
                    .clone()
                    .unwrap_or_else(|| vec!["cmd.exe".to_string()])
            });
            crate::spawn::spawn_elevated_single_pane_tab(
                argv,
                options.priority,
                cwd.cloned(),
                Some(window_id),
                Arc::new(config::TermConfig::with_config(config.clone())),
            )
            .await
            .with_context(|| {
                format!(
                    "spawning elevated tab {} of {} (--start-conf)",
                    idx + 1,
                    layout.tabs.len()
                )
            })?
        };

        // `None` means the spawn was debounced away or its tab could not be
        // resolved; both already logged, and there is nothing left to title
        // or type into.
        let Some(tab) = tab else {
            continue;
        };

        if let Some(title) = &tab_conf.title {
            tab.set_title(title);
        }

        if let Some(pane) = tab.get_active_pane() {
            let mut writer = pane.writer();
            for (cmd_idx, command) in layout
                .commands
                .iter()
                .chain(tab_conf.commands.iter())
                .enumerate()
            {
                // A real Enter keystroke is CR (`\r`), not LF (`\n`) --
                // sending a bare `\n` (what `writeln!` would send) doesn't
                // submit a line to a Windows shell reading from a ConPTY; a
                // smoke test showed every command's text just piling up
                // unexecuted on the same input line instead of running.
                // Sending `\r` alone (not `\r\n`) is the portable fix:
                // ConPTY treats `\r` as Enter directly, and a Unix pty's
                // termios `ICRNL` (on by default) translates an incoming
                // `\r` to `\n` -- which is what canonical mode treats as
                // the actual line terminator -- so a trailing `\n` on top
                // of that would submit a second, empty line on Unix.
                if let Err(err) = write!(writer, "{command}\r") {
                    // Deliberately not logging `command` itself: --start-conf
                    // startup commands can contain tokens, passwords, or
                    // credential-bearing URLs, and this warning goes into a
                    // long-lived per-PID log file on disk that may end up in
                    // a bug report. Naming which list the command came from,
                    // its position in that list, and its length is enough to
                    // point at the offending config line without exposing
                    // its content.
                    let layout_command_count = layout.commands.len();
                    let (source, source_idx) = if cmd_idx < layout_command_count {
                        ("layout-wide", cmd_idx)
                    } else {
                        ("tab", cmd_idx - layout_command_count)
                    };
                    log::warn!(
                        "--start-conf: failed to send {} startup command {} of tab {} \
                         ({} bytes): {:#}",
                        source,
                        source_idx + 1,
                        idx + 1,
                        command.len(),
                        err
                    );
                }
            }
        }
    }

    for result in futures::future::join_all(pending_non_elevated.drain(..)).await {
        result?;
    }

    // Final selection runs after all configured tabs have been attached.
    if let Some(mut window) = mux.get_window_mut(window_id) {
        select_startup_tab(&mut window);
    }

    Ok(())
}

async fn spawn_startup_non_elevated_tab(
    layout: &config::StartupLayout,
    idx: usize,
    domain: Arc<dyn Domain>,
    window_id: mux::window::WindowId,
    size: TerminalSize,
    config: config::ConfigHandle,
    startup_order: Option<crate::spawn::StartupOrder>,
) -> anyhow::Result<()> {
    let tab_conf = &layout.tabs[idx];
    let options = layout.tab_options(tab_conf);
    let cwd = tab_conf.root_dir.as_ref().or(layout.root_dir.as_ref());
    let shell_argv = options.shell.map(|shell| shell.argv());
    let tab = if config.per_tab_process_isolation {
        let spawn = config::keyassignment::SpawnCommand {
            args: shell_argv.clone(),
            cwd: cwd.map(|c| c.to_path_buf()),
            set_environment_variables: layout
                .vars
                .iter()
                .chain(tab_conf.vars.iter())
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            priority: Some(options.priority),
            ..Default::default()
        };
        crate::spawn::spawn_single_pane_tab_for_startup(
            spawn,
            crate::spawn::SpawnWhere::NewTab,
            size,
            Some(window_id),
            Arc::new(config::TermConfig::with_config(config.clone())),
            startup_order,
        )
        .await
        .with_context(|| {
            format!(
                "spawning isolated tab {} of {} (--start-conf)",
                idx + 1,
                layout.tabs.len()
            )
        })?
    } else {
        let prog = shell_argv
            .as_ref()
            .map(|argv| argv.iter().map(OsStr::new).collect());
        let mut builder = config.build_prog(
            prog,
            config.default_prog.as_ref(),
            config.default_cwd.as_ref(),
        )?;
        if let Some(cwd) = cwd {
            builder.cwd(cwd);
        }
        for (k, v) in layout.vars.iter().chain(tab_conf.vars.iter()) {
            builder.env(k, v);
        }
        #[cfg(windows)]
        builder.set_priority_class(options.priority.to_win32_flag());
        Some(
            domain
                .spawn(size, Some(builder), None, window_id)
                .await
                .with_context(|| {
                    format!(
                        "spawning tab {} of {} (--start-conf)",
                        idx + 1,
                        layout.tabs.len()
                    )
                })?,
        )
    };

    let Some(tab) = tab else {
        return Ok(());
    };
    if let Some(title) = &tab_conf.title {
        tab.set_title(title);
    }
    if let Some(pane) = tab.get_active_pane() {
        let mut writer = pane.writer();
        for (cmd_idx, command) in layout
            .commands
            .iter()
            .chain(tab_conf.commands.iter())
            .enumerate()
        {
            if let Err(err) = write!(writer, "{command}\r") {
                let layout_command_count = layout.commands.len();
                let (source, source_idx) = if cmd_idx < layout_command_count {
                    ("layout-wide", cmd_idx)
                } else {
                    ("tab", cmd_idx - layout_command_count)
                };
                log::warn!(
                    "--start-conf: failed to send {} startup command {} of tab {} ({} bytes): {:#}",
                    source,
                    source_idx + 1,
                    idx + 1,
                    command.len(),
                    err
                );
            }
        }
    }
    Ok(())
}

async fn connect_to_auto_connect_domains() -> anyhow::Result<()> {
    let mux = Mux::get();
    let domains = mux.iter_domains();
    for dom in domains {
        if let Some(dom) = dom.downcast_ref::<ClientDomain>() {
            if dom.connect_automatically() {
                dom.attach(None).await?;
            }
        }
    }
    Ok(())
}

// `gui-startup` and `gui-attached` were rhai event-callback hooks fired here;
// with the scripting layer removed there is nothing left to notify, so
// spawning the GUI/attaching to a domain no longer needs to trigger anything
// beyond what already happens in their callers.

pub(super) fn cell_pixel_dims(config: &ConfigHandle, dpi: f64) -> anyhow::Result<(usize, usize)> {
    // Startup-latency diagnostics: see the "startup:" checkpoints elsewhere
    // in this file. This builds its own throwaway `FontConfiguration` (font
    // enumeration/parsing) just to measure one cell's pixel size before the
    // real window -- and its own separate `FontConfiguration` -- exist, so
    // this pair of checkpoints is what showed that cost is cheap (a few
    // tens of ms) rather than a second, redundant font-enumeration cost.
    log::info!("startup: cell_pixel_dims font enumeration starting");
    let fontconfig = Rc::new(FontConfiguration::new(Some(config.clone()), dpi as usize)?);
    log::info!("startup: cell_pixel_dims font enumeration done");
    let render_metrics = RenderMetrics::new(&fontconfig)?;
    Ok((
        render_metrics.cell_size.width as usize,
        render_metrics.cell_size.height as usize,
    ))
}

pub(super) async fn async_run_terminal_gui(
    cmd: Option<CommandBuilder>,
    opts: StartCommand,
    should_publish: bool,
) -> anyhow::Result<()> {
    let unix_socket_path = config::RUNTIME_DIR.join(format!("gui-sock-{}", std::process::id()));
    std::env::set_var("ONLYTERM_UNIX_SOCKET", unix_socket_path.clone());
    onlyterm_blob_leases::register_storage(Arc::new(
        onlyterm_blob_leases::simple_tempdir::SimpleTempDir::new_in(&*config::CACHE_DIR)?,
    ))?;
    if let Err(err) = spawn_mux_server(unix_socket_path, should_publish) {
        log::warn!("{:#}", err);
    }

    if !opts.no_auto_connect {
        connect_to_auto_connect_domains().await?;
    }

    let mux = Mux::get();

    let domain = if let Some(name) = &opts.domain {
        let domain = mux
            .get_domain_by_name(name)
            .ok_or_else(|| anyhow!("invalid domain {name}"))?;
        Some(domain)
    } else {
        None
    };

    // `--start-conf` replaces the single-tab spawn below entirely: it opens
    // its own window and one tab per entry in the layout file, so it
    // deliberately skips both the explicit-`--domain` single-spawn block
    // and `spawn_tab_in_domain_if_mux_is_empty` further down. `--attach` is
    // not meaningfully combinable with a from-scratch multi-tab layout, so
    // it (like `--domain`'s "attach instead of spawn" behavior) is not
    // specially handled here -- the full layout is always spawned into
    // whichever domain was resolved above.
    if let Some(start_conf) = &opts.start_conf {
        let layout = config::StartupLayout::load(start_conf)?;
        return spawn_startup_layout(&layout, domain, opts.workspace.clone()).await;
    }

    // `--choose-tab` opens a window with the New Tab Options dialog and no
    // tab at all: the first tab is spawned only once the user picks a shell
    // and presses Run, and dismissing the dialog exits, because there is
    // nothing behind it. Like the `--start-conf` branch above it returns
    // early, bypassing `spawn_tab_in_domain_if_mux_is_empty`.
    //
    // The `Activity` is what keeps this alive. A mux holding a window with
    // zero panes is "empty", and `prune_dead_windows` deletes it and sends
    // `MuxNotification::Empty`, which terminates the process -- unless an
    // Activity is outstanding, which both that function and the notification
    // handler check first. The one `new_empty_window`'s builder holds is no
    // use here: it is surrendered inside `notify()` and dropped as soon as
    // `WindowCreated` has been sent.
    //
    // Armed *before* the window is created, not after: `MuxWindowBuilder`
    // sends its notification synchronously when it is dropped on the mux
    // thread, and the slot has to already hold the Activity by the time
    // anything can act on that notification.
    if opts.choose_tab {
        crate::startup_chooser::arm(mux::activity::Activity::new(), opts.cwd.clone());
        // Dropping the builder is what publishes `WindowCreated`; binding it
        // to `_` would drop it here anyway, but silently.
        drop(mux.new_empty_window(opts.workspace.clone(), None));
        return Ok(());
    }

    let is_connecting = opts.attach;

    if let Some(domain) = &domain {
        if !opts.attach {
            let window_id = {
                // Force the builder to notify the frontend early,
                // so that the attach await below doesn't block it.
                let workspace = None;
                let position = None;
                let builder = mux.new_empty_window(workspace, position);
                *builder
            };

            domain.attach(Some(window_id)).await?;
            let config = config::configuration();
            let dpi = config.dpi.unwrap_or_else(::window::default_dpi);
            let tab = domain
                .spawn(
                    config.initial_size(dpi as u32, Some(cell_pixel_dims(&config, dpi)?)),
                    cmd.clone(),
                    None,
                    window_id,
                )
                .await?;
            let mut window = mux
                .get_window_mut(window_id)
                .ok_or_else(|| anyhow!("failed to get mux window id {window_id}"))?;
            if let Some(tab_idx) = window.idx_by_id(tab.tab_id()) {
                window.set_active_without_saving(tab_idx);
            }
        }
    }
    spawn_tab_in_domain_if_mux_is_empty(cmd, is_connecting, domain, opts.workspace).await
}

// OnlyTerm: a `start` invocation never asks an already-running GUI instance
// to spawn the window on its behalf and then exit -- every launch always
// becomes its own independent process with its own window, mux, and render
// state, regardless of whether another instance happens to be running.
// Upstream's default was to look for a running GUI's published gui-sock and
// delegate the spawn to it (opt out via `--always-new-process`); that
// default made a plain "launch OnlyTerm" delegate to a possibly much older
// process instead of actually starting fresh, which is exactly the
// confusing behavior this fork intentionally removes. `--always-new-process`
// still parses (for compatibility) but is now a no-op: this was always the
// only behavior.
//
// This process still publishes its own gui-sock (see `should_publish_gui_sock`
// and its use in `spawn_mux_server`) so unrelated tooling (e.g. a `cli`
// subcommand run from a separate process) can still find and control *this*
// window -- only the "delegate my own spawn to some other, already-running
// window" behavior is gone.
fn should_publish_gui_sock(mux: &Arc<Mux>, config: &ConfigHandle) -> bool {
    mux.default_domain().domain_name() == config.default_domain.as_deref().unwrap_or("local")
}

fn spawn_mux_server(unix_socket_path: PathBuf, should_publish: bool) -> anyhow::Result<()> {
    let mut listener =
        onlyterm_mux_server_impl::local::LocalListener::with_domain(&config::UnixDomain {
            socket_path: Some(unix_socket_path.clone()),
            ..Default::default()
        })?;
    std::thread::spawn(move || {
        let name_holder;
        if should_publish {
            name_holder = onlyterm_client::discovery::publish_gui_sock_path(
                &unix_socket_path,
                &crate::termwindow::get_window_class(),
            );
            if let Err(err) = &name_holder {
                log::warn!("{:#}", err);
            }
        }

        listener.run();
        std::fs::remove_file(unix_socket_path).ok();
    });

    Ok(())
}

fn setup_mux(
    local_domain: Arc<dyn Domain>,
    config: &ConfigHandle,
    default_domain_name: Option<&str>,
    default_workspace_name: Option<&str>,
) -> anyhow::Result<Arc<Mux>> {
    let mux = Arc::new(mux::Mux::new(Some(local_domain.clone())));
    Mux::set_mux(&mux);
    let client_id = Arc::new(mux::client::ClientId::new());
    mux.register_client(client_id.clone());
    mux.replace_identity(Some(client_id));
    let default_workspace_name = default_workspace_name.unwrap_or(
        config
            .default_workspace
            .as_deref()
            .unwrap_or(mux::DEFAULT_WORKSPACE),
    );
    mux.set_active_workspace(default_workspace_name);
    crate::update::load_last_release_info_and_set_banner();
    update_mux_domains(config)?;

    let default_name =
        default_domain_name.unwrap_or(config.default_domain.as_deref().unwrap_or("local"));

    let domain = mux.get_domain_by_name(default_name).ok_or_else(|| {
        anyhow::anyhow!(
            "desired default domain '{}' was not found in mux!?",
            default_name
        )
    })?;
    mux.set_default_domain(&domain);

    Ok(mux)
}

pub(super) fn build_initial_mux(
    config: &ConfigHandle,
    default_domain_name: Option<&str>,
    default_workspace_name: Option<&str>,
) -> anyhow::Result<Arc<Mux>> {
    let domain: Arc<dyn Domain> = Arc::new(LocalDomain::new("local")?);
    setup_mux(domain, config, default_domain_name, default_workspace_name)
}

pub(super) fn run_terminal_gui(
    opts: StartCommand,
    default_domain_name: Option<String>,
) -> anyhow::Result<()> {
    if let Some(cls) = opts.class.as_ref() {
        crate::set_window_class(cls);
    }
    if let Some(pos) = opts.position.as_ref() {
        set_window_position(pos.clone());
    }

    let config = config::configuration();
    let need_builder = !opts.prog.is_empty() || opts.cwd.is_some();

    let cmd = if need_builder {
        let prog = opts.prog.iter().map(|s| s.as_os_str()).collect::<Vec<_>>();
        let mut builder = config.build_prog(
            if prog.is_empty() { None } else { Some(prog) },
            config.default_prog.as_ref(),
            config.default_cwd.as_ref(),
        )?;
        if let Some(cwd) = &opts.cwd {
            builder.cwd(if cwd.is_relative() {
                current_dir()?.join(cwd).into_os_string().into()
            } else {
                Cow::Borrowed(cwd.as_ref())
            });
        }
        Some(builder)
    } else {
        None
    };

    let mux = build_initial_mux(
        &config,
        default_domain_name.as_deref(),
        opts.workspace.as_deref(),
    )?;
    log::info!("startup: mux/domains ready");

    // OnlyTerm: never delegate this spawn to an already-running GUI instance
    // -- see `should_publish_gui_sock`'s doc comment. Always become our own
    // independent process/window here.
    let should_publish = should_publish_gui_sock(&mux, &config);

    let gui = crate::frontend::try_new()?;
    let activity = Activity::new();

    promise::spawn::spawn(async move {
        if let Err(err) = async_run_terminal_gui(cmd, opts, should_publish).await {
            terminate_with_error(err);
        }
        drop(activity);
    })
    .detach();

    maybe_show_configuration_error_window();
    gui.run_forever()
}

fn select_startup_tab(window: &mut mux::window::Window) {
    if !window.is_empty() {
        window.set_active_without_saving(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_layout_selects_first_tab_without_reordering() {
        // No native windows or child processes; exercise the real mux window.
        let mux = Arc::new(Mux::new(None));
        Mux::set_mux(&mux);
        struct ResetMux;
        impl Drop for ResetMux {
            fn drop(&mut self) {
                Mux::shutdown();
            }
        }
        let _reset = ResetMux;
        for count in [0, 1, 3] {
            let mut window = mux::window::Window::new(Some("startup-test".into()), None);
            for _ in 0..count {
                window.push(&Arc::new(mux::tab::Tab::new(&TerminalSize::default())));
            }
            let original: Vec<_> = window.iter().map(|tab| tab.tab_id()).collect();
            if count > 0 {
                window.set_active_without_saving(count - 1);
            }
            select_startup_tab(&mut window);
            assert_eq!(
                window.get_active().map(|tab| tab.tab_id()),
                original.first().copied()
            );
            assert_eq!(
                window.iter().map(|tab| tab.tab_id()).collect::<Vec<_>>(),
                original
            );
        }
    }
}
