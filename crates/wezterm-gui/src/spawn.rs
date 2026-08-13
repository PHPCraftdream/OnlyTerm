use anyhow::{anyhow, bail, Context};
use config::keyassignment::{SpawnCommand, SpawnTabDomain};
use config::{TermConfig, UnixDomain};
use mux::activity::Activity;
use mux::domain::{alloc_domain_id, Domain, SplitSource};
use mux::tab::SplitRequest;
use mux::window::WindowId as MuxWindowId;
use mux::Mux;
use portable_pty::CommandBuilder;
use std::sync::Arc;
use wezterm_client::domain::{ClientDomain, ClientDomainConfig};
use wezterm_term::TerminalSize;

/// Spawn a tab using a single-pane hosting process (per-tab isolation).
/// This creates a dynamic ClientDomain with a proxy_command to onlyterm-mux-server.exe
/// --single-pane, registers it with the Mux, attaches to it, and spawns the tab there.
async fn spawn_single_pane_tab(
    spawn: SpawnCommand,
    spawn_where: SpawnWhere,
    size: TerminalSize,
    src_window_id: Option<MuxWindowId>,
    term_config: Arc<TermConfig>,
) -> anyhow::Result<()> {
    let mux = Mux::get();

    // Get the path to onlyterm-mux-server.exe
    let mux_server_path = std::env::current_exe()?.with_file_name(if cfg!(windows) {
        "onlyterm-mux-server.exe"
    } else {
        "onlyterm-mux-server"
    });

    // Create a unique domain name for this tab (per-session uniqueness is enough)
    // Using a counter-based name ensures we can spawn multiple tabs with different domains
    let domain_name = format!("single-pane-{}", alloc_domain_id());

    // Build the UnixDomain config with proxy_command to spawn --single-pane mode
    let unix_domain = UnixDomain {
        name: domain_name.clone(),
        socket_path: None, // Not used with proxy_command
        connect_automatically: false,
        no_serve_automatically: true,
        serve_command: None,
        proxy_command: Some(vec![
            mux_server_path.to_string_lossy().to_string(),
            "--single-pane".to_string(),
        ]),
        skip_permissions_check: true, // Not applicable for proxy_command
        read_timeout: std::time::Duration::from_secs(60),
        write_timeout: std::time::Duration::from_secs(60),
        local_echo_threshold_ms: None,
        overlay_lag_indicator: false,
    };

    // Create and register the ClientDomain
    let client_domain_config = ClientDomainConfig::Unix(unix_domain);
    let domain: Arc<dyn Domain> = Arc::new(ClientDomain::new(client_domain_config));
    mux.add_domain(&domain);

    // Attach to the domain (spawns the single-pane process and establishes connection)
    domain.attach(src_window_id).await?;

    // Now spawn the tab in this new domain
    let cwd = if let Some(cwd) = spawn.cwd.as_ref() {
        Some(cwd.to_str().map(|s| s.to_owned()).ok_or_else(|| {
            anyhow!(
                "Domain::spawn requires that the cwd be unicode in {:?}",
                cwd
            )
        })?)
    } else {
        None
    };

    let cmd_builder = match (
        spawn.args.as_ref(),
        spawn.cwd.as_ref(),
        spawn.set_environment_variables.is_empty(),
    ) {
        (None, None, true) => None,
        _ => {
            let mut builder = spawn
                .args
                .as_ref()
                .map(|args| CommandBuilder::from_argv(args.iter().map(Into::into).collect()))
                .unwrap_or_else(CommandBuilder::new_default_prog);
            for (k, v) in spawn.set_environment_variables.iter() {
                builder.env(k, v);
            }
            if let Some(cwd) = &spawn.cwd {
                builder.cwd(cwd);
            }
            #[cfg(windows)]
            if let Some(priority) = spawn.priority {
                builder.set_priority_class(priority.to_win32_flag());
            }
            Some(builder)
        }
    };

    let workspace = mux.active_workspace().clone();

    match spawn_where {
        SpawnWhere::SplitPane(_) => {
            // Splitting through single-pane domains is NOT supported in Phase B
            // (explicitly out of scope per task requirements)
            bail!("SplitPane is not supported with per_tab_process_isolation yet");
        }
        _ => {
            let (_tab, pane, window_id) = mux
                .spawn_tab_or_window(
                    match spawn_where {
                        SpawnWhere::NewWindow => None,
                        _ => src_window_id,
                    },
                    SpawnTabDomain::DomainName(domain_name),
                    cmd_builder,
                    cwd,
                    size,
                    None, // No current_pane_id for new tab spawns
                    workspace,
                    spawn.position,
                )
                .await
                .context("spawn_tab_or_window in single-pane domain")?;

            if Some(window_id) == src_window_id {
                pane.set_config(term_config);
            }
        }
    }

    Ok(())
}

#[derive(Copy, Debug, Clone, Eq, PartialEq)]
pub enum SpawnWhere {
    NewWindow,
    NewTab,
    SplitPane(SplitRequest),
}

pub fn spawn_command_impl(
    spawn: &SpawnCommand,
    spawn_where: SpawnWhere,
    size: TerminalSize,
    src_window_id: Option<MuxWindowId>,
    term_config: Arc<TermConfig>,
) {
    let spawn = spawn.clone();

    promise::spawn::spawn(async move {
        let config = config::configuration();

        // Check if per-tab process isolation is enabled
        // Phase B only applies this to NewTab and NewWindow, not SplitPane
        let use_single_pane =
            config.per_tab_process_isolation && !matches!(spawn_where, SpawnWhere::SplitPane(_));

        if let Err(err) = if use_single_pane {
            spawn_single_pane_tab(spawn, spawn_where, size, src_window_id, term_config).await
        } else {
            spawn_command_internal(spawn, spawn_where, size, src_window_id, term_config).await
        } {
            log::error!("Failed to spawn: {:#}", err);
        }
    })
    .detach();
}

pub async fn spawn_command_internal(
    spawn: SpawnCommand,
    spawn_where: SpawnWhere,
    size: TerminalSize,
    src_window_id: Option<MuxWindowId>,
    term_config: Arc<TermConfig>,
) -> anyhow::Result<()> {
    let mux = Mux::get();
    let activity = Activity::new();

    let current_pane_id = match src_window_id {
        Some(window_id) => {
            if let Some(tab) = mux.get_active_tab_for_window(window_id) {
                tab.get_active_pane().map(|p| p.pane_id())
            } else {
                None
            }
        }
        None => None,
    };

    let cwd = if let Some(cwd) = spawn.cwd.as_ref() {
        Some(cwd.to_str().map(|s| s.to_owned()).ok_or_else(|| {
            anyhow!(
                "Domain::spawn requires that the cwd be unicode in {:?}",
                cwd
            )
        })?)
    } else {
        None
    };

    let cmd_builder = match (
        spawn.args.as_ref(),
        spawn.cwd.as_ref(),
        spawn.set_environment_variables.is_empty(),
    ) {
        (None, None, true) => None,
        _ => {
            let mut builder = spawn
                .args
                .as_ref()
                .map(|args| CommandBuilder::from_argv(args.iter().map(Into::into).collect()))
                .unwrap_or_else(CommandBuilder::new_default_prog);
            for (k, v) in spawn.set_environment_variables.iter() {
                builder.env(k, v);
            }
            if let Some(cwd) = &spawn.cwd {
                builder.cwd(cwd);
            }
            #[cfg(windows)]
            if let Some(priority) = spawn.priority {
                builder.set_priority_class(priority.to_win32_flag());
            }
            Some(builder)
        }
    };

    let workspace = mux.active_workspace().clone();

    match spawn_where {
        SpawnWhere::SplitPane(direction) => {
            let src_window_id = match src_window_id {
                Some(id) => id,
                None => anyhow::bail!("no src window when splitting a pane?"),
            };
            if let Some(tab) = mux.get_active_tab_for_window(src_window_id) {
                let pane = tab
                    .get_active_pane()
                    .ok_or_else(|| anyhow!("tab to have a pane"))?;

                log::trace!("doing split_pane");
                let (pane, _size) = mux
                    .split_pane(
                        // tab.tab_id(),
                        pane.pane_id(),
                        direction,
                        SplitSource::Spawn {
                            command: cmd_builder,
                            command_dir: cwd,
                        },
                        spawn.domain,
                    )
                    .await
                    .context("split_pane")?;
                pane.set_config(term_config);
            } else {
                bail!("there is no active tab while splitting pane!?");
            }
        }
        _ => {
            let (_tab, pane, window_id) = mux
                .spawn_tab_or_window(
                    match spawn_where {
                        SpawnWhere::NewWindow => None,
                        _ => src_window_id,
                    },
                    spawn.domain,
                    cmd_builder,
                    cwd,
                    size,
                    current_pane_id,
                    workspace,
                    spawn.position,
                )
                .await
                .context("spawn_tab_or_window")?;

            // If it was created in this window, it copies our handlers.
            // Otherwise, we'll pick them up when we later respond to
            // the new window being created.
            if Some(window_id) == src_window_id {
                pane.set_config(term_config);
            }
        }
    };

    drop(activity);

    Ok(())
}
