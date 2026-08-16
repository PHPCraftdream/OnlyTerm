#![warn(clippy::undocumented_unsafe_blocks)]
// Don't create a new standard console window when launched from the windows GUI.
#![cfg_attr(not(test), windows_subsystem = "windows")]

#[global_allocator]
static GLOBAL_ALLOC: sefer_alloc::SeferAlloc = sefer_alloc::SeferAlloc::new();

use crate::customglyph::BlockKey;
use crate::glyphcache::GlyphCache;
use crate::utilsprites::RenderMetrics;
use ::window::*;
use anyhow::{anyhow, Context};
use clap::builder::ValueParser;
use clap::{Parser, ValueHint};
use config::{ConfigHandle, SerialDomain};
use mux::activity::Activity;
use mux::domain::{Domain, LocalDomain};
use mux::Mux;
use portable_pty::cmdbuilder::CommandBuilder;
use std::borrow::Cow;
use std::env::current_dir;
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use termwiz::cell::CellAttributes;
use termwiz::surface::{Line, SEQ_ZERO};
use unicode_normalization::UnicodeNormalization;
use wezterm_bidi::Direction;
use wezterm_client::domain::ClientDomain;
use wezterm_font::shaper::PresentationWidth;
use wezterm_font::FontConfiguration;
use wezterm_gui_subcommands::*;
use wezterm_mux_server_impl::update_mux_domains;
use wezterm_toast_notification::*;

mod colorease;
mod commands;
mod customglyph;
mod download;
mod elevate;
mod frontend;
mod glyphcache;
mod gui_api;
mod inputmap;
mod overlay;
mod quad;
mod renderstate;
mod renderthread;
mod resize_increment_calculator;
mod scrollbar;
mod selection;
mod shapecache;
mod spawn;
mod startup_chooser;
mod stats;
mod tabbar;
mod termwindow;
mod unicode_names;
mod update;
mod utilsprites;

#[cfg(feature = "dhat-heap")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

pub use selection::SelectionMode;
pub use termwindow::{set_window_class, set_window_position, TermWindow, ICON_DATA};

#[derive(Debug, Parser)]
#[command(
    about = "OnlyTerm - Terminal Emulator (fork of WezTerm)\nhttp://github.com/wezterm/wezterm",
    version = config::wezterm_version()
)]
struct Opt {
    /// Skip loading the wezterm config (ktav) file
    #[arg(long, short = 'n')]
    skip_config: bool,

    /// Specify the configuration file to use, overrides the normal
    /// configuration file resolution
    #[arg(
        long = "config-file",
        value_parser,
        conflicts_with = "skip_config",
        value_hint=ValueHint::FilePath,
    )]
    config_file: Option<OsString>,

    /// Override specific configuration values
    #[arg(
        long = "config",
        name = "name=value",
        value_parser=ValueParser::new(name_equals_value),
        number_of_values = 1)]
    config_override: Vec<(String, String)>,

    /// On Windows, whether to attempt to attach to the parent
    /// process console to display logging output
    #[arg(long = "attach-parent-console")]
    #[allow(dead_code)]
    attach_parent_console: bool,

    #[command(subcommand)]
    cmd: Option<SubCommand>,
}

#[derive(Debug, Parser, Clone)]
enum SubCommand {
    #[command(
        name = "start",
        about = "Start the GUI, optionally running an alternative program [aliases: -e]"
    )]
    Start(StartCommand),

    /// Start the GUI in blocking mode. You shouldn't see this, but you
    /// may see it in shell completions because of this open clap issue:
    /// <https://github.com/clap-rs/clap/issues/1335>
    #[command(short_flag_alias = 'e', hide = true)]
    BlockingStart(StartCommand),

    #[command(name = "serial", about = "Open a serial port")]
    Serial(SerialCommand),

    #[command(name = "connect", about = "Connect to OnlyTerm multiplexer")]
    Connect(ConnectCommand),

    #[command(name = "ls-fonts", about = "Display information about fonts")]
    LsFonts(LsFontsCommand),

    #[command(name = "show-keys", about = "Show key assignments")]
    ShowKeys(ShowKeysCommand),
}

async fn async_run_serial(opts: SerialCommand) -> anyhow::Result<()> {
    let serial_domain = SerialDomain {
        name: format!("Serial Port {}", opts.port),
        port: Some(opts.port.clone()),
        baud: opts.baud,
    };

    let start_command = StartCommand {
        always_new_process: true,
        class: opts.class,
        cwd: None,
        no_auto_connect: true,
        position: opts.position,
        workspace: None,
        domain: Some(serial_domain.name.clone()),
        ..Default::default()
    };

    let cmd = None;

    let domain: Arc<dyn Domain> = Arc::new(LocalDomain::new_serial_domain(serial_domain)?);
    let mux = Mux::get();
    mux.add_domain(&domain);

    let should_publish = false;
    async_run_terminal_gui(cmd, start_command, should_publish).await
}

fn run_serial(config: config::ConfigHandle, opts: SerialCommand) -> anyhow::Result<()> {
    if let Some(cls) = opts.class.as_ref() {
        crate::set_window_class(cls);
    }
    if let Some(pos) = opts.position.as_ref() {
        set_window_position(pos.clone());
    }

    build_initial_mux(&config, None, None)?;

    let gui = crate::frontend::try_new()?;

    promise::spawn::spawn(async {
        if let Err(err) = async_run_serial(opts).await {
            terminate_with_error(err);
        }
    })
    .detach();

    maybe_show_configuration_error_window();
    gui.run_forever()
}

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

    for (idx, tab_conf) in layout.tabs.iter().enumerate() {
        let options = layout.tab_options(tab_conf);
        // A tab's own `root_dir` wins over the layout-wide one, which in
        // turn wins over `config.default_cwd` that `build_prog` applies --
        // the same override order `run_terminal_gui` already uses for the
        // plain `--cwd` flag vs. `config.default_cwd`.
        let cwd = tab_conf.root_dir.as_ref().or(layout.root_dir.as_ref());
        let shell_argv = options.shell.map(|shell| shell.argv());

        let tab = if options.admin {
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
        } else if config.per_tab_process_isolation {
            // Same isolation the interactive "new tab" path gets. Without
            // this branch a layout's tabs stayed inside the GUI process even
            // with the option on, so one window could hold isolated tabs
            // opened by hand and non-isolated ones from the layout -- and the
            // long-lived tabs a startup layout creates are precisely the ones
            // worth containing.
            //
            // The tab's `vars` travel to the hosting process as a real
            // environment (see `UnixDomain::proxy_env`), not as command-line
            // arguments, so they are not exposed to other processes.
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
            // Run as its own task rather than awaiting inline, and that is
            // not a style choice. An `async fn` keeps its state *inside*
            // whichever future awaits it, so holding this one (which in turn
            // holds `attach_with_spinner`) inside `spawn_startup_layout` grew
            // that future past what the main thread's stack could carry: the
            // GUI died during startup with "thread 'main' has overflowed its
            // stack", after the hosting child had already come up fine.
            // `Box::pin` around the call is NOT enough in a debug build --
            // the future is still materialised on the stack before it is
            // moved into the box. Spawning leaves only a small `Task` handle
            // here and lets the executor own the future, which is exactly
            // what the interactive caller in spawn.rs already does.
            let term_config = Arc::new(config::TermConfig::with_config(config.clone()));
            promise::spawn::spawn(async move {
                crate::spawn::spawn_single_pane_tab(
                    spawn,
                    crate::spawn::SpawnWhere::NewTab,
                    size,
                    Some(window_id),
                    term_config,
                )
                .await
            })
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

fn cell_pixel_dims(config: &ConfigHandle, dpi: f64) -> anyhow::Result<(usize, usize)> {
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

async fn async_run_terminal_gui(
    cmd: Option<CommandBuilder>,
    opts: StartCommand,
    should_publish: bool,
) -> anyhow::Result<()> {
    let unix_socket_path = config::RUNTIME_DIR.join(format!("gui-sock-{}", std::process::id()));
    std::env::set_var("ONLYTERM_UNIX_SOCKET", unix_socket_path.clone());
    wezterm_blob_leases::register_storage(Arc::new(
        wezterm_blob_leases::simple_tempdir::SimpleTempDir::new_in(&*config::CACHE_DIR)?,
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
        wezterm_mux_server_impl::local::LocalListener::with_domain(&config::UnixDomain {
            socket_path: Some(unix_socket_path.clone()),
            ..Default::default()
        })?;
    std::thread::spawn(move || {
        let name_holder;
        if should_publish {
            name_holder = wezterm_client::discovery::publish_gui_sock_path(
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

fn build_initial_mux(
    config: &ConfigHandle,
    default_domain_name: Option<&str>,
    default_workspace_name: Option<&str>,
) -> anyhow::Result<Arc<Mux>> {
    let domain: Arc<dyn Domain> = Arc::new(LocalDomain::new("local")?);
    setup_mux(domain, config, default_domain_name, default_workspace_name)
}

fn run_terminal_gui(opts: StartCommand, default_domain_name: Option<String>) -> anyhow::Result<()> {
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

fn fatal_toast_notification(title: &str, message: &str) {
    persistent_toast_notification(title, message);
    // We need a short delay otherwise the notification
    // will not show
    #[cfg(windows)]
    std::thread::sleep(std::time::Duration::new(2, 0));
}

fn notify_on_panic() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if let Some(s) = info.payload().downcast_ref::<&str>() {
            fatal_toast_notification("OnlyTerm panic", s);
        }
        default_hook(info);
    }));
}

pub(crate) fn terminate_with_error_message(err: &str) -> ! {
    log::error!("{}; terminating", err);
    fatal_toast_notification("OnlyTerm Error", err);
    std::process::exit(1);
}

pub(crate) fn terminate_with_error(err: anyhow::Error) -> ! {
    let mut err_text = format!("{err:#}");

    let warnings = config::configuration_warnings_and_errors();
    if !warnings.is_empty() {
        let err = warnings.join("\n");
        err_text = format!("{err_text}\nConfiguration Error: {err}");
    }

    terminate_with_error_message(&err_text)
}

fn main() {
    #[cfg(feature = "dhat-heap")]
    let _profiler = dhat::Profiler::new_heap();

    // `config::designate_this_as_the_main_thread()` used to be called here
    // to initialize the rhai event-callback state so that
    // `with_rhai_config_on_main_thread`/`run_immediate_with_rhai_config`
    // (used only by the removed rhai call sites) would not panic. With no
    // remaining wezterm-gui call sites into the rhai event-callback bridge,
    // that designation is no longer needed here.
    config::assign_error_callback(mux::connui::show_configuration_error_message);
    notify_on_panic();
    if let Err(e) = run() {
        terminate_with_error(e);
    }
    Mux::shutdown();
    frontend::shutdown();
}

fn maybe_show_configuration_error_window() {
    // Errors only. This opens a whole extra window, which is the right
    // response to "your config didn't load, here's why" and much too much
    // for "you set an option that no longer does anything": a deprecation
    // notice would greet the user with a second window on every single
    // launch until they edited their config. Warnings still reach the log
    // and the CLI subcommands that print them.
    if let Some(err) = config::configuration_error() {
        mux::connui::show_configuration_error_message(&err);
    }

    for warning in config::configuration_warnings_and_errors() {
        log::warn!("{warning}");
    }
}

fn run_show_keys(config: config::ConfigHandle, cmd: &ShowKeysCommand) -> anyhow::Result<()> {
    let map = crate::inputmap::InputMap::new(&config);
    if cmd.ktav {
        map.dump_config(cmd.key_table.as_deref());
    } else {
        map.show_keys();
    }
    Ok(())
}

pub fn run_ls_fonts(config: config::ConfigHandle, cmd: &LsFontsCommand) -> anyhow::Result<()> {
    use wezterm_font::parser::ParsedFont;

    if let Err(err) = config::configuration_result() {
        log::error!("{}", err);
        return Ok(());
    }

    // Disable the normal config error UI window, as we don't have
    // a fully baked GUI environment running
    config::assign_error_callback(|err| eprintln!("{}", err));

    // `configuration_result()` above only surfaces hard errors. Deprecated
    // (but still parseable) fields such as the old `child_process_timeout_ms`
    // are reported as warnings instead, and since this subcommand has no
    // GUI to pop up `maybe_show_configuration_error_window`'s toast, print
    // them directly so the CLI user actually sees them.
    for warning in config::configuration_warnings_and_errors() {
        eprintln!("{}", warning);
    }

    let font_config = Rc::new(wezterm_font::FontConfiguration::new(
        Some(config.clone()),
        config.dpi.unwrap_or_else(::window::default_dpi) as usize,
    )?);

    let render_metrics = crate::utilsprites::RenderMetrics::new(&font_config)?;

    let bidi_hint = if config.bidi_enabled {
        Some(config.bidi_direction)
    } else {
        None
    };

    let unicode_version = config.unicode_version();

    let text = match (&cmd.text, &cmd.codepoints) {
        (Some(text), _) => Some(text.to_string()),
        (_, Some(codepoints)) => {
            let mut s = String::new();
            for cp in codepoints.split(",") {
                let cp = u32::from_str_radix(cp, 16)
                    .with_context(|| format!("{cp} is not a hex number"))?;
                let c = char::from_u32(cp)
                    .ok_or_else(|| anyhow!("{cp} is not a valid unicode codepoint value"))?;
                s.push(c);
            }
            Some(s)
        }
        _ => None,
    };

    if let Some(text) = &text {
        // Emulate the effect of output normalization
        let text = if config.normalize_output_to_unicode_nfc {
            text.nfc().collect()
        } else {
            text.to_string()
        };

        let line = Line::from_text(
            &text,
            &CellAttributes::default(),
            SEQ_ZERO,
            Some(&unicode_version),
        );
        let cell_clusters = line.cluster(bidi_hint);

        let mut glyph_cache = GlyphCache::new_in_memory(&font_config, 256)?;

        for cluster in cell_clusters {
            let style = font_config.match_style(&config, &cluster.attrs);
            let font = font_config.resolve_font(style)?;
            let presentation_width = PresentationWidth::with_cluster(&cluster);
            let infos = font
                .blocking_shape(
                    &cluster.text,
                    Some(cluster.presentation),
                    cluster.direction,
                    None,
                    Some(&presentation_width),
                )
                .unwrap();

            // We must grab the handles after shaping, so that we get the
            // revised list that includes system fallbacks!
            let handles = font.clone_handles();

            let mut iter = infos.iter().peekable();

            let mut byte_lens = vec![];
            for c in cluster.text.chars() {
                let len = c.len_utf8();
                for _ in 0..len {
                    byte_lens.push(len);
                }
            }
            println!("{:?}", cluster.direction);

            while let Some(info) = iter.next() {
                let idx = cluster.byte_to_cell_idx(info.cluster as usize);
                let followed_by_space = match line.get_cell(idx + 1) {
                    Some(cell) => cell.str() == " ",
                    None => false,
                };

                let text = if cluster.direction == Direction::LeftToRight {
                    if let Some(next) = iter.peek() {
                        line.columns_as_str(idx..cluster.byte_to_cell_idx(next.cluster as usize))
                    } else {
                        let last_idx = cluster.byte_to_cell_idx(cluster.text.len() - 1);
                        line.columns_as_str(idx..last_idx + 1)
                    }
                } else {
                    let info_len = byte_lens[info.cluster as usize];
                    let last_idx = cluster.byte_to_cell_idx(info.cluster as usize + info_len - 1);
                    line.columns_as_str(idx..last_idx + 1)
                };

                let parsed = &handles[info.font_idx];
                let escaped = format!("{}", text.escape_unicode());
                let mut is_custom = false;

                let cached_glyph = glyph_cache.cached_glyph(
                    info,
                    style,
                    followed_by_space,
                    &font,
                    &render_metrics,
                    info.num_cells,
                )?;

                let mut texture = cached_glyph.texture.clone();

                if config.custom_block_glyphs {
                    if let Some(block) = info.only_char.and_then(BlockKey::from_char) {
                        texture.replace(glyph_cache.cached_block(block, &render_metrics)?);
                        println!(
                            "{:2} {:4} {:12} drawn by wezterm because custom_block_glyphs=true: {:?}",
                            info.cluster, text, escaped, block
                        );
                        is_custom = true;
                    }
                }

                if !is_custom {
                    let glyph_name = parsed
                        .glyph_name(info.glyph_pos)
                        .map(|name| format!("{},", name))
                        .unwrap_or_else(String::new);

                    println!(
                        "{:2} {:4} {:12} x_adv={:<2} cells={:<2} glyph={}{:<4} {}\n{:38}{}",
                        info.cluster,
                        text,
                        escaped,
                        cached_glyph.x_advance.get(),
                        info.num_cells,
                        glyph_name,
                        info.glyph_pos,
                        parsed.ktav_name(),
                        "",
                        parsed.handle.diagnostic_string()
                    );
                }

                if cmd.rasterize_ascii {
                    let mut glyph = String::new();

                    if let Some(texture) = &cached_glyph.texture {
                        use ::window::bitmaps::ImageTexture;
                        if let Some(tex) = texture.texture.downcast_ref::<ImageTexture>() {
                            for y in texture.coords.min_y()..texture.coords.max_y() {
                                for &px in tex.image.borrow().horizontal_pixel_range(
                                    texture.coords.min_x() as usize,
                                    texture.coords.max_x() as usize,
                                    y as usize,
                                ) {
                                    let px = u32::from_be(px);
                                    let (b, g, r, a) = (
                                        (px >> 8) as u8,
                                        (px >> 16) as u8,
                                        (px >> 24) as u8,
                                        (px & 0xff) as u8,
                                    );
                                    // Use regular RGB for other terminals, but then
                                    // set RGBA for wezterm
                                    glyph.push_str(&format!(
                                "\x1b[38:2::{r}:{g}:{b}m\x1b[38:6::{r}:{g}:{b}:{a}m\u{2588}\x1b[0m"
                            ));
                                }
                                glyph.push('\n');
                            }
                        }
                    }

                    if !is_custom {
                        println!(
                            "bearing: x={} y={}, offset: x={} y={}",
                            cached_glyph.bearing_x.get(),
                            cached_glyph.bearing_y.get(),
                            cached_glyph.x_offset.get(),
                            cached_glyph.y_offset.get(),
                        );
                    }
                    println!("{glyph}");
                }
            }
        }
        return Ok(());
    }

    println!("Primary font:");
    let default_font = font_config.default_font()?;
    println!(
        "{}",
        ParsedFont::ktav_fallback(&default_font.clone_handles())
    );
    println!();

    for rule in &config.font_rules {
        println!();

        let mut condition = "When".to_string();
        if let Some(intensity) = &rule.intensity {
            condition.push_str(&format!(" Intensity={:?}", intensity));
        }
        if let Some(underline) = &rule.underline {
            condition.push_str(&format!(" Underline={:?}", underline));
        }
        if let Some(italic) = &rule.italic {
            condition.push_str(&format!(" Italic={:?}", italic));
        }
        if let Some(blink) = &rule.blink {
            condition.push_str(&format!(" Blink={:?}", blink));
        }
        if let Some(rev) = &rule.reverse {
            condition.push_str(&format!(" Reverse={:?}", rev));
        }
        if let Some(strikethrough) = &rule.strikethrough {
            condition.push_str(&format!(" Strikethrough={:?}", strikethrough));
        }
        if let Some(invisible) = &rule.invisible {
            condition.push_str(&format!(" Invisible={:?}", invisible));
        }

        println!("{}:", condition);
        let font = font_config.resolve_font(&rule.font)?;
        println!("{}", ParsedFont::ktav_fallback(&font.clone_handles()));
        println!();
    }

    println!("Title font:");
    let title_font = font_config.title_font()?;
    println!("{}", ParsedFont::ktav_fallback(&title_font.clone_handles()));
    println!();

    if cmd.list_system {
        let font_dirs = font_config.list_fonts_in_font_dirs();
        println!(
            "{} fonts found in your font_dirs + built-in fonts:",
            font_dirs.len()
        );
        for font in font_dirs {
            let pixel_sizes = if font.pixel_sizes.is_empty() {
                "".to_string()
            } else {
                format!(" pixel_sizes={:?}", font.pixel_sizes)
            };
            println!(
                "{} -- {}{}{}",
                font.ktav_name(),
                font.aka(),
                font.handle.diagnostic_string(),
                pixel_sizes
            );
        }

        match font_config.list_system_fonts() {
            Ok(sys_fonts) => {
                println!(
                    "{} system fonts found using {:?}:",
                    sys_fonts.len(),
                    config.font_locator
                );
                for font in sys_fonts {
                    let pixel_sizes = if font.pixel_sizes.is_empty() {
                        "".to_string()
                    } else {
                        format!(" pixel_sizes={:?}", font.pixel_sizes)
                    };
                    println!(
                        "{} -- {}{}{}",
                        font.ktav_name(),
                        font.aka(),
                        font.handle.diagnostic_string(),
                        pixel_sizes
                    );
                }
            }
            Err(err) => log::error!("Unable to list system fonts: {}", err),
        }
    }

    Ok(())
}

fn run() -> anyhow::Result<()> {
    // Inform the system of our AppUserModelID.
    // Without this, our toast notifications won't be correctly
    // attributed to our application.
    #[cfg(windows)]
    {
        // SAFETY: `SetCurrentProcessExplicitAppUserModelID` takes a PCWSTR that
        // must point to a NUL-terminated UTF-16 string valid for the call's
        // duration. `wide_string` appends a trailing NUL and the returned
        // `Vec<u16>` lives until the end of this statement, so the borrowed
        // pointer is valid. The call only affects the current process.
        unsafe {
            ::windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID(
                ::windows::core::PCWSTR(wide_string("org.wezfurlong.onlyterm").as_ptr()),
            )
            .unwrap();
        }
    }

    let opts = Opt::parse();

    // This is a bit gross.
    // In order to not to automatically open a standard windows console when
    // we run, we use the windows_subsystem attribute at the top of this
    // source file.  That comes at the cost of causing the help output
    // to disappear if we are actually invoked from a console.
    // This AttachConsole call will attach us to the console of the parent
    // in that situation, but since we were launched as a windows subsystem
    // application we will be running asynchronously from the shell in
    // the command window, which means that it will appear to the user
    // that we hung at the end, when in reality the shell is waiting for
    // input but didn't know to re-draw the prompt.
    // SAFETY: `AttachConsole` attaches the calling process to the console of
    // the given process id; `ATTACH_PARENT_PROCESS` is the sentinel (DWORD)-1
    // meaning "the parent process". No pointers are dereferenced and the single
    // `u32` argument is a value type, so there is no aliasing or lifetime concern.
    #[cfg(windows)]
    unsafe {
        if opts.attach_parent_console {
            winapi::um::wincon::AttachConsole(winapi::um::wincon::ATTACH_PARENT_PROCESS);
        }
    };

    env_bootstrap::bootstrap();
    // Startup-latency diagnostics: every `log::info!("startup: ...")` line
    // in this crate (grep for `"startup:` to find them all) is a checkpoint,
    // and the per-PID log file's own timestamps (see `env_bootstrap::ringlog`,
    // `%H:%M:%S%.3f`) give millisecond deltas between them without needing a
    // separate clock. Kept permanently (not gated behind a debug flag, since
    // `Info` is already the default log level) so a future startup-latency
    // regression can be diagnosed the same way this one was: by reading a
    // single per-PID log file instead of profiling from scratch.
    log::info!("startup: logger ready");

    stats::Stats::init()?;
    let _saver = umask::UmaskSaver::new();

    config::common_init(
        opts.config_file.as_ref(),
        &opts.config_override,
        opts.skip_config,
    )?;
    log::info!("startup: config loaded");
    let config = config::configuration();
    if let Some(value) = &config.default_ssh_auth_sock {
        std::env::set_var("SSH_AUTH_SOCK", value);
    }

    let sub = match opts.cmd.as_ref().cloned() {
        Some(SubCommand::BlockingStart(start)) => {
            // Act as if the normal start subcommand was used,
            // except that we always start a new instance.
            // This is needed for compatibility, because many tools assume
            // that "$TERMINAL -e $COMMAND" blocks until the command finished.
            SubCommand::Start(StartCommand {
                always_new_process: true,
                ..start
            })
        }
        Some(sub) => sub,
        None => {
            // Need to fake an argv0
            let mut argv = vec!["onlyterm-gui".to_string()];
            for a in &config.default_gui_startup_args {
                argv.push(a.clone());
            }
            SubCommand::try_parse_from(&argv).with_context(|| {
                format!(
                    "parsing the default_gui_startup_args config: {:?}",
                    config.default_gui_startup_args
                )
            })?
        }
    };

    match sub {
        SubCommand::Start(start) => {
            log::trace!("Using configuration: {:#?}\nopts: {:#?}", config, opts);
            let res = run_terminal_gui(start, None);
            wezterm_blob_leases::clear_storage();
            res
        }
        SubCommand::BlockingStart(_) => unreachable!(),
        SubCommand::Serial(serial) => run_serial(config, serial),
        SubCommand::Connect(connect) => run_terminal_gui(
            StartCommand {
                domain: Some(connect.domain_name.clone()),
                class: connect.class,
                workspace: connect.workspace,
                position: connect.position,
                prog: connect.prog,
                new_tab: connect.new_tab,
                always_new_process: true,
                attach: true,
                _cmd: false,
                no_auto_connect: false,
                cwd: None,
                start_conf: None,
                choose_tab: false,
            },
            Some(connect.domain_name),
        ),
        SubCommand::LsFonts(cmd) => run_ls_fonts(config, &cmd),
        SubCommand::ShowKeys(cmd) => run_show_keys(config, &cmd),
    }
}
