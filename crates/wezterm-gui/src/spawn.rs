use anyhow::{anyhow, bail, Context};
use config::keyassignment::SpawnCommand;
use config::{ProcessPriority, TermConfig, UnixDomain};
use mux::activity::Activity;
use mux::domain::{alloc_domain_id, Domain, SplitSource};
use mux::tab::{SplitRequest, Tab};
use mux::window::WindowId as MuxWindowId;
use mux::Mux;
use portable_pty::CommandBuilder;
use promise::spawn::spawn_into_new_thread;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use wezterm_client::domain::{ClientDomain, ClientDomainConfig};
use wezterm_term::TerminalSize;

/// Windows this function is currently spawning a single-pane hosting
/// process for. Guards against the case where the connect takes a few
/// seconds (cold process start + mux handshake) and the user, seeing no
/// immediate feedback, clicks "new tab" again for the same window before
/// the first click's tab has appeared -- each such repeat click used to
/// spawn its own redundant onlyterm-mux-server.exe process (confirmed
/// live: a handful of impatient clicks left that many orphaned hosting
/// processes running). One in-flight spawn per originating window at a
/// time; extra clicks are dropped rather than queued or erroring.
fn pending_single_pane_spawns() -> &'static Mutex<HashSet<MuxWindowId>> {
    static PENDING: OnceLock<Mutex<HashSet<MuxWindowId>>> = OnceLock::new();
    PENDING.get_or_init(|| Mutex::new(HashSet::new()))
}

struct PendingSpawnGuard(MuxWindowId);
impl Drop for PendingSpawnGuard {
    fn drop(&mut self) {
        pending_single_pane_spawns().lock().unwrap().remove(&self.0);
    }
}

/// Spawn a tab using a single-pane hosting process (per-tab isolation).
/// This creates a dynamic ClientDomain with a proxy_command to onlyterm-mux-server.exe
/// --single-pane, registers it with the Mux, and attaches to it.
///
/// The single-pane hosting process spawns its one-and-only pane itself, at
/// startup (see `run_single_pane_mode` in wezterm-mux-server) -- attaching
/// below materializes that pre-spawned pane as our tab. This function must
/// NOT also issue an explicit spawn RPC afterwards: the single-pane server
/// hosts exactly one pane and isn't designed to host a second one, so an
/// additional `mux.spawn_tab_or_window()` call produced a stray extra tab
/// plus a stray window in practice (confirmed via live testing) -- the
/// desired shell/cwd/priority must instead be passed to the child process
/// as CLI args, which is what the arguments below do.
async fn spawn_single_pane_tab(
    spawn: SpawnCommand,
    spawn_where: SpawnWhere,
    _size: TerminalSize,
    src_window_id: Option<MuxWindowId>,
    term_config: Arc<TermConfig>,
) -> anyhow::Result<()> {
    if matches!(spawn_where, SpawnWhere::SplitPane(_)) {
        // Splitting through single-pane domains is NOT supported in Phase B
        // (explicitly out of scope per task requirements)
        bail!("SplitPane is not supported with per_tab_process_isolation yet");
    }
    if !spawn.set_environment_variables.is_empty() {
        bail!(
            "per_tab_process_isolation does not yet support custom \
             environment variables for spawned tabs"
        );
    }

    // See `pending_single_pane_spawns`'s doc comment: silently drop a
    // repeat "new tab" trigger for a window that already has one of these
    // spawns in flight, instead of piling up another hosting process.
    let _pending_guard = if let Some(window_id) = src_window_id {
        if !pending_single_pane_spawns()
            .lock()
            .unwrap()
            .insert(window_id)
        {
            return Ok(());
        }
        Some(PendingSpawnGuard(window_id))
    } else {
        None
    };

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

    let mut proxy_command = vec![
        mux_server_path.to_string_lossy().to_string(),
        "--single-pane".to_string(),
    ];
    if let Some(cwd) = spawn.cwd.as_ref() {
        let cwd = cwd.to_str().ok_or_else(|| {
            anyhow!(
                "Domain::spawn requires that the cwd be unicode in {:?}",
                cwd
            )
        })?;
        proxy_command.push("--cwd".to_string());
        proxy_command.push(cwd.to_string());
    }
    #[cfg(windows)]
    if let Some(priority) = spawn.priority {
        proxy_command.push("--priority".to_string());
        proxy_command.push(format!("{:?}", priority));
    }
    if let Some(args) = spawn.args.as_ref() {
        proxy_command.push("--".to_string());
        proxy_command.extend(args.iter().cloned());
    }

    // Build the UnixDomain config with proxy_command to spawn --single-pane mode
    let unix_domain = UnixDomain {
        name: domain_name,
        socket_path: None, // Not used with proxy_command
        connect_automatically: false,
        no_serve_automatically: true,
        serve_command: None,
        proxy_command: Some(proxy_command),
        skip_permissions_check: true, // Not applicable for proxy_command
        read_timeout: std::time::Duration::from_secs(60),
        write_timeout: std::time::Duration::from_secs(60),
        local_echo_threshold_ms: None,
        overlay_lag_indicator: false,
    };

    // Create and register the ClientDomain. Kept as a concrete `Arc<ClientDomain>`
    // (rather than immediately erasing to `Arc<dyn Domain>`) so we can call
    // `attach_headless` below, which isn't part of the `Domain` trait.
    let client_domain_config = ClientDomainConfig::Unix(unix_domain);
    let client_domain = Arc::new(ClientDomain::new_single_pane(client_domain_config));
    let domain: Arc<dyn Domain> = client_domain.clone();
    mux.add_domain(&domain);

    let attach_window_id = match spawn_where {
        SpawnWhere::NewWindow => None,
        _ => src_window_id,
    };

    // Attach to the domain: spawns the single-pane process, establishes the
    // connection, and materializes its one pre-spawned pane as a tab in
    // `attach_window_id` (or a brand-new window, if that's None or its
    // workspace doesn't match). `attach_with_spinner` (rather than the plain
    // `Domain::attach`) shows a quiet animated spinner in that placeholder
    // tab instead of a raw connection-progress log -- see its doc comment.
    client_domain.attach_with_spinner(attach_window_id).await?;

    if attach_window_id.is_some() {
        if let Some(pane) = mux
            .iter_panes()
            .into_iter()
            .find(|p| p.domain_id() == domain.domain_id())
        {
            pane.set_config(term_config);
        }
    }

    Ok(())
}

/// Spawn an elevated (admin, UAC) single-pane tab using the WebSocket rendezvous path.
/// This is the elevated counterpart to `spawn_single_pane_tab`.
///
/// This function:
/// - Participates in the same per-window debounce as `spawn_single_pane_tab`
///   (via `pending_single_pane_spawns()`) to prevent duplicate UAC prompts.
/// - Offloads the blocking `spawn_elevated_single_pane` call to a real OS thread
///   via `spawn_into_new_thread` (it blocks on the UAC prompt and the WebSocket
///   handshake wait).
/// - On success, builds a `ClientDomain`, registers it with the `Mux`, and
///   attaches via `attach_stream_with_spinner` (quiet spinner, not the raw log).
/// - On failure/cancellation, returns an `Err` with a clear message; the caller
///   (`execute_new_tab_run_request`) surfaces this as a toast notification.
///
/// # Cancel-safety
/// This function awaits a spawned task (`spawn_into_new_thread`) and then
/// `attach_stream_with_spinner`. Dropping the `Task` detaches the thread (§B21),
/// but the thread owns the `stream`, so no resource is lost. `attach_stream_with_spinner`
/// is cancel-safe (see its doc comment). The debounce guard's `Drop` impl is
/// infallible and idempotent.
/// Returns the tab that now hosts the elevated pane, or `None` when there is
/// none to return: either the spawn was debounced away, or the tab could not
/// be resolved afterwards. Callers that only want the side effect can ignore
/// it; `--start-conf` needs it to apply the tab's title and startup commands.
pub async fn spawn_elevated_single_pane_tab(
    argv: Vec<String>,
    priority: ProcessPriority,
    cwd: Option<PathBuf>,
    src_window_id: Option<MuxWindowId>,
    term_config: Arc<TermConfig>,
) -> anyhow::Result<Option<Arc<Tab>>> {
    // See `pending_single_pane_spawns`'s doc comment: silently drop a
    // repeat "new tab" trigger for a window that already has one of these
    // spawns in flight, instead of piling up another UAC prompt.
    let _pending_guard = if let Some(window_id) = src_window_id {
        if !pending_single_pane_spawns()
            .lock()
            .unwrap()
            .insert(window_id)
        {
            return Ok(None);
        }
        Some(PendingSpawnGuard(window_id))
    } else {
        None
    };

    let mux = Mux::get();

    // Run the blocking spawn (UAC prompt + WebSocket handshake) on a real OS thread.
    let result_task = spawn_into_new_thread(move || {
        Ok(crate::elevate::spawn_elevated_single_pane(
            &argv,
            cwd.as_deref(),
            priority,
            Duration::from_secs(30),
        ))
    });

    let result = result_task.await.context("failed to join spawn thread")?;

    let stream = match result {
        crate::elevate::ElevatedSinglePaneResult::Success(stream) => stream,
        crate::elevate::ElevatedSinglePaneResult::UserCancelled => {
            bail!("elevation was cancelled")
        }
        crate::elevate::ElevatedSinglePaneResult::Failed(msg) => {
            bail!("failed to spawn elevated single pane: {}", msg)
        }
    };

    // Build a UnixDomain config for the ClientDomain.
    // Unlike the non-elevated `spawn_single_pane_tab`, we have no `proxy_command`
    // to run — the connection already exists — so we set `proxy_command: None`.
    // Other fields match the non-elevated case.
    let domain_name = format!("single-pane-elev-{}", alloc_domain_id());
    let unix_domain = UnixDomain {
        name: domain_name,
        socket_path: None, // Not used when we already have a stream
        connect_automatically: false,
        no_serve_automatically: true,
        serve_command: None,
        proxy_command: None, // No proxy_command — we already have a connected stream
        skip_permissions_check: true, // Not applicable for already-connected stream
        read_timeout: Duration::from_secs(60),
        write_timeout: Duration::from_secs(60),
        local_echo_threshold_ms: None,
        overlay_lag_indicator: false,
    };

    // Create and register the ClientDomain.
    let client_domain_config = ClientDomainConfig::Unix(unix_domain);
    let client_domain = Arc::new(ClientDomain::new_single_pane(client_domain_config));
    let domain: Arc<dyn Domain> = client_domain.clone();
    mux.add_domain(&domain);

    // Attach to the domain: the elevated child has already connected and is ready.
    // Use `attach_stream_with_spinner` (quiet animated spinner) instead of the raw
    // `Domain::attach` log, matching the UX pattern of the non-elevated path.
    client_domain
        .attach_stream_with_spinner(src_window_id, stream)
        .await?;

    // The attach above is what created the pane, so this is the first point
    // at which it can be found. Used for two unrelated things below, hence
    // the single lookup.
    let pane = mux
        .iter_panes()
        .into_iter()
        .find(|p| p.domain_id() == domain.domain_id());

    // Apply the term_config to the resulting pane.
    if src_window_id.is_some() {
        if let Some(pane) = &pane {
            pane.set_config(term_config);
        }
    }

    // Flag the tab as elevated, which is what makes the tab bar draw its
    // admin indicator: the tab's border gets painted in the tab's own text
    // colour (see `fancy_tab_bar.rs`). Nothing else sets this flag, so
    // without this call `Tab::is_elevated` stays false for every tab and
    // that entire rendering path is unreachable -- which is exactly why no
    // indicator was appearing despite being implemented.
    let tab = match pane
        .as_ref()
        .and_then(|pane| mux.resolve_pane_id(pane.pane_id()))
    {
        Some((_domain_id, _window_id, tab_id)) => match mux.get_tab(tab_id) {
            Some(tab) => {
                tab.set_elevated(true);
                Some(tab)
            }
            None => {
                log::warn!(
                    "elevated tab {} vanished before it could be flagged as elevated; \
                     it will render without the admin indicator",
                    tab_id
                );
                None
            }
        },
        None => {
            log::warn!(
                "could not resolve the elevated pane to a tab; the tab will render \
                 without the admin indicator"
            );
            None
        }
    };

    Ok(tab)
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
