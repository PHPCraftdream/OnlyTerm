#![warn(clippy::undocumented_unsafe_blocks)]
#[global_allocator]
static GLOBAL_ALLOC: sefer_alloc::SeferAlloc = sefer_alloc::SeferAlloc::new();

use clap::*;
use config::configuration;
use mux::activity::Activity;
use mux::domain::{Domain, LocalDomain};
use mux::{Mux, MuxNotification};
use portable_pty::cmdbuilder::CommandBuilder;
use std::ffi::OsString;
use std::process::Command;
use std::sync::Arc;
use std::thread;
use wezterm_gui_subcommands::*;
use wezterm_mux_server_impl::update_mux_domains_for_server;
use wezterm_uds::UnixStream;

#[derive(Debug, Parser)]
#[command(
    about = "OnlyTerm - Terminal Emulator (fork of WezTerm)\nhttp://github.com/wezterm/wezterm",
    version = config::wezterm_version(),
    trailing_var_arg = true,
)]
struct Opt {
    /// Skip loading the wezterm config (ktav) file
    #[arg(long, short = 'n')]
    skip_config: bool,

    /// Specify the configuration file to use, overrides the normal
    /// configuration file resolution
    #[arg(
        long,
        value_parser,
        conflicts_with = "skip_config",
        value_hint=ValueHint::FilePath,
    )]
    config_file: Option<OsString>,

    /// Override specific configuration values
    #[arg(
        long = "config",
        name = "name=value",
        value_parser=clap::builder::ValueParser::new(name_equals_value),
        number_of_values = 1)]
    config_override: Vec<(String, String)>,

    /// Detach from the foreground and become a background process
    #[arg(long = "daemonize")]
    daemonize: bool,

    /// Single-pane hosting mode: process exits when pane exits.
    /// In this mode, the process does NOT daemonize and does NOT
    /// spawn a Unix socket listener. Instead, it uses stdin/stdout
    /// for mux protocol communication.
    #[arg(long = "single-pane")]
    single_pane: bool,

    /// Specify the current working directory for the initially
    /// spawned program
    #[arg(long = "cwd", value_parser, value_hint=ValueHint::DirPath)]
    cwd: Option<OsString>,

    /// Windows process priority class for the initially spawned program.
    /// One of: Idle, BelowNormal, Normal, AboveNormal, High, Realtime
    /// (matches config::keyassignment::ProcessPriority's variant names).
    #[arg(long = "priority", value_parser)]
    priority: Option<String>,

    /// Instead of executing your shell, run PROG.
    /// For example: `wezterm start -- bash -l` will spawn bash
    /// as if it were a login shell.
    #[arg(value_parser, value_hint=ValueHint::CommandWithArguments, num_args=1..)]
    prog: Vec<OsString>,
}

fn main() {
    if let Err(err) = run() {
        wezterm_blob_leases::clear_storage();
        log::error!("{:#}", err);
        std::process::exit(1);
    }
    wezterm_blob_leases::clear_storage();
}

fn run() -> anyhow::Result<()> {
    env_bootstrap::bootstrap();

    //stats::Stats::init()?;
    // `config::designate_this_as_the_main_thread()` used to be called here
    // to initialize the rhai event-callback state so that
    // `with_rhai_config_on_main_thread` (used only by the removed
    // `mux-startup` rhai call site) would not panic. With no remaining
    // call sites into the rhai event-callback bridge, that designation is
    // no longer needed here.
    let _saver = umask::UmaskSaver::new();

    let opts = Opt::parse();

    config::common_init(
        opts.config_file.as_ref(),
        &opts.config_override,
        opts.skip_config,
    )?;

    let config = config::configuration();

    config.update_ulimit()?;
    if let Some(value) = &config.default_ssh_auth_sock {
        std::env::set_var("SSH_AUTH_SOCK", value);
    }

    // Single-pane mode: skip daemonization and use stdin/stdout for mux protocol
    if opts.single_pane {
        return run_single_pane_mode(opts);
    }

    if opts.daemonize {
        // We can't literally daemonize on Windows, but we can spawn
        // another copy of ourselves in the background.
        let mut cmd = Command::new(std::env::current_exe().unwrap());

        if opts.skip_config {
            cmd.arg("-n");
        }
        if let Some(f) = &opts.config_file {
            cmd.arg("--config-file");
            cmd.arg(f);
        }
        for (name, value) in &opts.config_override {
            cmd.arg("--config");
            cmd.arg(format!("{name}={value}"));
        }
        if let Some(cwd) = opts.cwd {
            cmd.arg("--cwd");
            cmd.arg(cwd);
        }
        if !opts.prog.is_empty() {
            cmd.arg("--");
            for a in &opts.prog {
                cmd.arg(a);
            }
        }

        use std::os::windows::process::CommandExt;
        cmd.stdout(config.daemon_options.open_stdout()?);
        cmd.stderr(config.daemon_options.open_stderr()?);

        cmd.creation_flags(winapi::um::winbase::DETACHED_PROCESS);
        let child = cmd.spawn();
        drop(child);
        return Ok(());
    }

    // Remove some environment variables that aren't super helpful or
    // that are potentially misleading when we're starting up the
    // server.
    // We may potentially want to look into starting/registering
    // a session of some kind here as well in the future.
    for name in &[
        "OLDPWD",
        "PWD",
        "SHLVL",
        "ONLYTERM_PANE",
        "ONLYTERM_UNIX_SOCKET",
        "_",
    ] {
        std::env::remove_var(name);
    }
    for name in &config::configuration().mux_env_remove {
        std::env::remove_var(name);
    }

    wezterm_blob_leases::register_storage(Arc::new(
        wezterm_blob_leases::simple_tempdir::SimpleTempDir::new_in(&*config::CACHE_DIR)?,
    ))?;

    let need_builder = !opts.prog.is_empty() || opts.cwd.is_some();

    let cmd = if need_builder {
        let mut builder = if opts.prog.is_empty() {
            CommandBuilder::new_default_prog()
        } else {
            CommandBuilder::from_argv(opts.prog)
        };
        if let Some(cwd) = opts.cwd {
            builder.cwd(cwd);
        }
        Some(builder)
    } else {
        None
    };

    let domain: Arc<dyn Domain> = Arc::new(LocalDomain::new("local")?);
    let mux = Arc::new(mux::Mux::new(Some(domain.clone())));
    Mux::set_mux(&mux);

    let executor = promise::spawn::SimpleExecutor::new();

    spawn_listener().map_err(|e| {
        log::error!("problem spawning listeners: {:?}", e);
        e
    })?;

    let activity = Activity::new();

    promise::spawn::spawn(async move {
        if let Err(err) = async_run(cmd).await {
            terminate_with_error(err);
        }
        drop(activity);
    })
    .detach();

    loop {
        executor.tick()?;
    }
}

// `mux-startup` was a rhai event-callback hook fired here; with the
// scripting layer removed there is nothing left to notify, so mux startup
// no longer needs to trigger anything beyond what already happens below.

async fn async_run(cmd: Option<CommandBuilder>) -> anyhow::Result<()> {
    let mux = Mux::get();
    let config = config::configuration();

    update_mux_domains_for_server(&config)?;
    let _config_subscription = config::subscribe_to_config_reload(move || {
        promise::spawn::spawn_into_main_thread(async move {
            if let Err(err) = update_mux_domains_for_server(&config::configuration()) {
                log::error!("Error updating mux domains: {:#}", err);
            }
        })
        .detach();
        true
    });

    let domain = mux.default_domain();

    let have_panes_in_domain = mux
        .iter_panes()
        .iter()
        .any(|p| p.domain_id() == domain.domain_id());

    if !have_panes_in_domain {
        let workspace = None;
        let position = None;
        let window_id = mux.new_empty_window(workspace, position);
        domain.attach(Some(*window_id)).await?;

        let _tab = mux
            .default_domain()
            .spawn(config.initial_size(0, None), cmd, None, *window_id)
            .await?;
    }
    Ok(())
}

fn terminate_with_error(err: anyhow::Error) -> ! {
    log::error!("{:#}; terminating", err);
    std::process::exit(1);
}

pub fn spawn_listener() -> anyhow::Result<()> {
    let config = configuration();
    for unix_dom in &config.unix_domains {
        std::env::set_var("ONLYTERM_UNIX_SOCKET", unix_dom.socket_path());
        let mut listener = wezterm_mux_server_impl::local::LocalListener::with_domain(unix_dom)?;
        thread::spawn(move || {
            listener.run();
        });
    }

    Ok(())
}

/// Run in single-pane mode: one process = one pane, exit when pane exits.
/// Uses stdin/stdout for mux protocol communication.
fn run_single_pane_mode(opts: Opt) -> anyhow::Result<()> {
    let _config = config::configuration();

    // Remove environment variables that aren't helpful
    for name in &[
        "OLDPWD",
        "PWD",
        "SHLVL",
        "ONLYTERM_PANE",
        "ONLYTERM_UNIX_SOCKET",
        "_",
    ] {
        std::env::remove_var(name);
    }
    for name in &config::configuration().mux_env_remove {
        std::env::remove_var(name);
    }

    wezterm_blob_leases::register_storage(Arc::new(
        wezterm_blob_leases::simple_tempdir::SimpleTempDir::new_in(&*config::CACHE_DIR)?,
    ))?;

    // Build command: use opts.prog or default to cmd.exe on Windows
    let cmd_builder = if opts.prog.is_empty() {
        CommandBuilder::new_default_prog()
    } else {
        CommandBuilder::from_argv(opts.prog)
    };
    let mut cmd_builder = cmd_builder;
    if let Some(cwd) = opts.cwd {
        cmd_builder.cwd(cwd);
    }
    #[cfg(windows)]
    if let Some(priority) = &opts.priority {
        let flag = match priority.as_str() {
            "Idle" => 0x00000040,
            "BelowNormal" => 0x00004000,
            "Normal" => 0x00000020,
            "AboveNormal" => 0x00008000,
            "High" => 0x00000080,
            "Realtime" => 0x00000100,
            other => anyhow::bail!("unknown --priority value: {other}"),
        };
        cmd_builder.set_priority_class(flag);
    }
    let cmd = Some(cmd_builder);

    let domain: Arc<dyn Domain> = Arc::new(LocalDomain::new("local")?);
    let mux = Arc::new(mux::Mux::new(Some(domain.clone())));
    Mux::set_mux(&mux);

    let executor = promise::spawn::SimpleExecutor::new();

    // Must happen before spawn_stdio_listener() wraps the inherited handle
    // as a UnixStream -- see init_winsock's doc comment.
    init_winsock();

    // Spawn a listener that processes mux protocol on stdin/stdout
    // This is similar to how LocalListener works, but uses stdin/stdout
    // instead of Unix sockets
    spawn_stdio_listener();

    let activity = Activity::new();

    promise::spawn::spawn(async move {
        if let Err(err) = async_run_single_pane(cmd).await {
            terminate_with_error(err);
        }
        drop(activity);
    })
    .detach();

    loop {
        executor.tick()?;
    }
}

/// Initializes Winsock in this process. Daemon mode gets this "for free"
/// because `filedescriptor::socketpair()` (called on the GUI/parent side)
/// calls `WSAStartup` itself -- but Winsock initialization is per-process,
/// not inherited across a process boundary along with a socket handle.
/// Single-pane mode's child process never calls `socketpair()` itself (it
/// only inherits an already-created handle as stdin), so without this call
/// its first real socket I/O (inside `dispatch::process`) fails with
/// "Either the application has not called WSAStartup, or WSAStartup
/// failed" (os error 10093) -- confirmed by an actual manual end-to-end
/// run, not just reasoned about.
fn init_winsock() {
    use std::sync::Once;
    use winapi::um::winsock2::{WSAStartup, WSADATA};
    static START: Once = Once::new();
    START.call_once(|| {
        // SAFETY: WSAStartup is the standard Winsock initialization call;
        // `0x202` requests version 2.2; `&mut data` is a valid out-pointer
        // to a `repr(C)` struct that's valid when zero-initialized. Same
        // pattern as `filedescriptor::windows::socketpair::init_winsock`.
        unsafe {
            let mut data: WSADATA = std::mem::zeroed();
            let ret = WSAStartup(0x202, &mut data);
            assert_eq!(ret, 0, "failed to initialize winsock");
        }
    });
}

/// Schedules the mux protocol dispatcher for the socketpair handle this
/// process inherited as stdin when spawned via `proxy_command`.
///
/// `dispatch::process` is an `async fn`: calling it only constructs a
/// `Future` and runs none of its body until something actually polls it.
/// The original version of this function did `let _ =
/// dispatch::process(stream);` inside a bare `thread::spawn`, which builds
/// the future and immediately drops it unpolled -- the mux protocol never
/// actually ran, silently. `spawn_into_main_thread` (the same primitive
/// `LocalListener::run` in `wezterm-mux-server-impl/src/local.rs` uses for
/// every accepted daemon-mode connection) is what actually drives it, by
/// scheduling it onto the `SimpleExecutor` that `run_single_pane_mode`'s
/// `executor.tick()` loop is already polling.
fn spawn_stdio_listener() {
    use std::os::windows::io::{AsRawHandle, FromRawSocket};

    // When the parent calls `filedescriptor::socketpair()`, it gets two
    // Winsock SOCKET handles: one it keeps (wrapped as a UnixStream), the
    // other it hands to this child process as stdin/stdout via
    // `cmd.stdin(b.as_stdio()?)`. On Windows, a Winsock SOCKET and a
    // Windows HANDLE are the same underlying value, so the raw handle
    // this process inherited as stdin IS that socket.
    let stdin_handle = std::io::stdin().as_raw_handle() as std::os::windows::io::RawSocket;

    // SAFETY: stdin_handle is the Winsock SOCKET inherited from the parent
    // via socketpair() + Stdio (see above); this process took sole
    // ownership of it at spawn time and never uses stdin for anything
    // else, so wrapping it as a UnixStream here does not alias any other
    // owner of the handle.
    let stream = unsafe { UnixStream::from_raw_socket(stdin_handle) };

    promise::spawn::spawn_into_main_thread(async move {
        if let Err(err) = wezterm_mux_server_impl::dispatch::process(stream).await {
            log::error!("{:#}", err);
        }
    })
    .detach();
}

/// Async portion of single-pane mode.
async fn async_run_single_pane(cmd: Option<CommandBuilder>) -> anyhow::Result<()> {
    let mux = Mux::get();
    let config = config::configuration();

    let domain = mux.default_domain();

    // Spawn exactly ONE pane
    let workspace = None;
    let position = None;
    let window_id = mux.new_empty_window(workspace, position);
    domain.attach(Some(*window_id)).await?;

    let _tab = mux
        .default_domain()
        .spawn(config.initial_size(0, None), cmd, None, *window_id)
        .await?;

    let pane_id = mux
        .iter_panes()
        .first()
        .map(|p| p.pane_id())
        .ok_or_else(|| anyhow::anyhow!("No pane created"))?;

    // Subscribe to pane removal notification to know when the pane exits
    let (pane_exited_tx, pane_exited_rx) = smol::channel::bounded::<()>(1);
    let pane_id_copy = pane_id;
    mux.subscribe(move |notification| {
        if let MuxNotification::PaneRemoved(ref id) = notification {
            if *id == pane_id_copy {
                let _ = pane_exited_tx.try_send(());
            }
        }
        true // Return true to keep the subscription active
    });

    // Wait for the pane to exit
    pane_exited_rx
        .recv()
        .await
        .map_err(|e| anyhow::anyhow!("Pane exit channel error: {:?}", e))
}
