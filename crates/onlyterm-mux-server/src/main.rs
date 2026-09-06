#![warn(clippy::undocumented_unsafe_blocks)]
#[global_allocator]
static GLOBAL_ALLOC: sefer_alloc::SeferAlloc = sefer_alloc::SeferAlloc::new();

use anyhow::Context;
use clap::*;
use config::configuration;
use mux::activity::Activity;
use mux::domain::{Domain, LocalDomain};
use mux::pane::PaneId;
use mux::{Mux, MuxNotification};
use onlyterm_gui_subcommands::*;
use onlyterm_mux_server_impl::sessionhandler::PduPolicy;
use onlyterm_mux_server_impl::update_mux_domains_for_server;
use onlyterm_uds::UnixStream;
use portable_pty::cmdbuilder::CommandBuilder;
use std::ffi::OsString;
use std::future::Future;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;

#[derive(Debug, Parser)]
#[command(
    about = "OnlyTerm - Terminal Emulator (fork of OnlyTerm)\nhttp://github.com/wezterm/wezterm",
    version = config::onlyterm_version(),
    trailing_var_arg = true,
)]
struct Opt {
    /// Skip loading the onlyterm config (ktav) file
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

    /// Single-pane hosting mode: process exits when the pane exits, or
    /// when its one client disconnects (whichever happens first).
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

    /// WebSocket rendezvous port for elevated single-pane mode.
    /// Must be used together with --token. Connects to 127.0.0.1:PORT
    /// instead of using stdin/stdout for mux protocol.
    #[arg(long = "connect-ws-port", value_parser)]
    connect_ws_port: Option<u16>,

    /// Authentication token for WebSocket rendezvous.
    /// Must be used together with --connect-ws-port.
    #[arg(long = "token", value_parser)]
    token: Option<String>,

    /// PID of the parent GUI process to supervise.
    /// When provided, the child will watch for the parent's termination
    /// and exit when the parent dies. This is used for elevated single-pane
    /// mode to ensure the elevated child dies with its non-elevated parent.
    #[arg(long = "supervise-pid", value_parser)]
    supervise_pid: Option<u32>,

    /// Instead of executing your shell, run PROG.
    /// For example: `onlyterm start -- bash -l` will spawn bash
    /// as if it were a login shell.
    #[arg(value_parser, value_hint=ValueHint::CommandWithArguments, num_args=1..)]
    prog: Vec<OsString>,
}

fn main() {
    if let Err(err) = run() {
        onlyterm_blob_leases::clear_storage();
        log::error!("{:#}", err);
        std::process::exit(1);
    }
    onlyterm_blob_leases::clear_storage();
}

fn run() -> anyhow::Result<()> {
    let _log_guard = env_bootstrap::bootstrap();

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

    onlyterm_blob_leases::register_storage(Arc::new(
        onlyterm_blob_leases::simple_tempdir::SimpleTempDir::new_in(&*config::CACHE_DIR)?,
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
        let mut listener = onlyterm_mux_server_impl::local::LocalListener::with_domain(unix_dom)?;
        thread::spawn(move || {
            listener.run();
        });
    }

    Ok(())
}

/// Run in single-pane mode: one process = one pane.
/// Uses stdin/stdout for mux protocol communication, or WebSocket rendezvous
/// when --connect-ws-port/--token are provided.
///
/// The process exits as soon as it has nothing left to host, which means
/// *either* of two conditions -- not just the first one:
///
/// 1. the pane exits (the shell terminated, or the client killed it), or
/// 2. the mux protocol client goes away (clean disconnect, or a protocol
///    error such as a failed attach).
///
/// Condition 2 matters because there is no listener here: the single
/// client that connected can never be replaced, so a pane that outlives it
/// is unreachable forever. Before this process exits on that path it kills
/// the pane, so the hosted shell cannot survive as an orphan -- which for
/// an elevated tab would mean a live elevated shell the user's own
/// non-elevated tools cannot terminate.
/// Spawn a parent-watcher thread that monitors the parent process and exits
/// when the parent dies.
///
/// This is used for elevated single-pane mode, where the elevated child cannot
/// be assigned to a job object by its medium-integrity parent. Instead, the
/// child opens a handle to its parent (which is allowed across integrity
/// boundaries) and waits for the parent to terminate.
///
/// # Why this runs in a dedicated thread
///
/// The entire point of this watchdog is to survive a wedged main loop. If the
/// main async executor is blocked or dead, this thread must still be able to
/// terminate the process. Therefore, this thread shares no state with the main
/// loop - it simply opens a handle and waits on it, with no locks or shared
/// mutable state.
///
/// # Why OpenProcess happens immediately at startup
///
/// Windows reuses process IDs. If we resolved the PID lazily later, the PID
/// might already belong to an unrelated process by that time. At that point,
/// we'd be waiting on a stranger and would outlive our real parent anyway.
/// By opening the handle at startup, we capture a reference to the specific
/// process that spawned us before its PID can be reused.
///
/// # Failure handling
///
/// - No `--supervise-pid` given: Not an error - the non-elevated path doesn't
///   pass this flag, and it doesn't need this mechanism (t582 handles that
///   case via job objects). We simply don't spawn the watcher thread.
/// - OpenProcess fails: Log a warning and continue without parent watching.
///   This can happen if the parent has already exited, or if there's a
///   permission issue (unlikely since elevated processes can open lower-
///   integrity processes).
/// - Parent already gone at startup: Log and exit immediately. Continuing
///   would mean we're an orphan with no client to serve.
#[cfg(windows)]
fn spawn_parent_watcher(supervise_pid: Option<u32>) {
    use winapi::shared::minwindef::DWORD;
    use winapi::um::errhandlingapi::GetLastError;
    use winapi::um::handleapi::CloseHandle;
    use winapi::um::processthreadsapi::OpenProcess;
    use winapi::um::synchapi::WaitForSingleObject;
    use winapi::um::winbase::{INFINITE, WAIT_OBJECT_0};
    use winapi::um::winnt::SYNCHRONIZE;

    let Some(parent_pid) = supervise_pid else {
        // No --supervise-pid: non-elevated path or older invocation.
        // No need to spawn a watcher thread.
        return;
    };

    /// Carries the parent's process handle onto the watcher thread.
    ///
    /// A Windows HANDLE is an index into a process-wide table, not something
    /// owned by the thread that obtained it, so using it from another thread
    /// is sound -- but it is spelled as a raw pointer, which the compiler
    /// must assume is not `Send`.
    struct ParentHandle(winapi::um::winnt::HANDLE);
    // SAFETY: see above -- the handle is valid process-wide, and exactly one
    // thread owns this value at a time because it is moved, not shared.
    unsafe impl Send for ParentHandle {}

    // Resolve the pid to a handle HERE, on the calling thread, before the
    // watcher thread even exists. Windows recycles process ids, so every
    // moment between the parent handing us its pid and us pinning it down is
    // a window in which that pid could come to mean some unrelated process --
    // and a watcher waiting on a stranger would never fire, leaving exactly
    // the orphan this function exists to prevent. Deferring the call into the
    // spawned thread would widen that window by however long the scheduler
    // takes to run it, for no benefit.
    //
    // SAFETY: OpenProcess is a standard Windows API call; SYNCHRONIZE is the
    // minimal access right needed to wait on the handle.
    let h_parent = unsafe { OpenProcess(SYNCHRONIZE, false as i32, parent_pid as DWORD) };

    if h_parent.is_null() {
        // SAFETY: GetLastError takes no arguments and has no preconditions;
        // it just reads the calling thread's last-error TLS slot.
        let error_code = unsafe { GetLastError() };
        log::warn!(
            "Parent-watcher: OpenProcess({}) failed with error {}. \
             This typically means the parent has already exited.",
            parent_pid,
            error_code
        );
        // Parent already gone before we finished starting: exit rather than
        // carry on. There is no client left to serve, and continuing would
        // leave a shell -- an elevated one, on that path -- running with
        // nothing able to reach it. That is the orphan case, arriving early.
        std::process::exit(1);
    }

    let h_parent = ParentHandle(h_parent);

    std::thread::spawn(move || {
        let h_parent = h_parent.0;

        log::info!(
            "Parent-watcher: opened handle to parent PID {}, waiting...",
            parent_pid
        );

        // Wait for the parent to signal (terminate).
        // SAFETY: h_parent is a valid process handle returned by OpenProcess.
        // WaitForSingleObject blocks until the handle is signaled (process exits)
        // or the timeout expires. INFINITE means wait forever.
        let wait_result = unsafe { WaitForSingleObject(h_parent, INFINITE) };

        // SAFETY: h_parent is a valid process handle returned by OpenProcess and
        // we have sole ownership of it; closing it here is correct after we're
        // done waiting.
        unsafe { CloseHandle(h_parent) };

        match wait_result {
            WAIT_OBJECT_0 => {
                // Parent has exited. Terminate ourselves.
                log::info!(
                    "Parent-watcher: parent PID {} has terminated, exiting",
                    parent_pid
                );
                std::process::exit(0);
            }
            _ => {
                // Unexpected wait result (shouldn't happen with INFINITE timeout)
                // SAFETY: GetLastError takes no arguments and has no preconditions;
                // it just reads the calling thread's last-error TLS slot.
                let error_code = unsafe { GetLastError() };
                log::error!(
                    "Parent-watcher: WaitForSingleObject returned unexpected result {} (GetLastError={}). \
                     This should not happen with INFINITE timeout. Continuing without parent supervision.",
                    wait_result,
                    error_code
                );
                // Don't exit - we may still be serving a client. Log the error
                // and continue running.
            }
        }
    });
}

fn run_single_pane_mode(opts: Opt) -> anyhow::Result<()> {
    // Validate that WebSocket flags are used together
    let ws_rendezvous = match (&opts.connect_ws_port, &opts.token) {
        (Some(_), None) => anyhow::bail!("--connect-ws-port requires --token to be specified"),
        (None, Some(_)) => anyhow::bail!("--token requires --connect-ws-port to be specified"),
        (Some(port), Some(token)) => Some((*port, token.clone())),
        (None, None) => None,
    };

    // Spawn parent-watcher thread early, before any async work starts.
    // This ensures we're watching the parent even if startup stalls.
    #[cfg(windows)]
    spawn_parent_watcher(opts.supervise_pid);

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

    onlyterm_blob_leases::register_storage(Arc::new(
        onlyterm_blob_leases::simple_tempdir::SimpleTempDir::new_in(&*config::CACHE_DIR)?,
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

    // Obtain the mux protocol stream: either via WebSocket rendezvous
    // (elevated mode) or via inherited stdin (non-elevated mode)
    let (stream, policy) = if let Some((port, token)) = ws_rendezvous {
        // WebSocket rendezvous path: connect back to the GUI
        // IMPORTANT: Use ElevatedSinglePaneAllowList to restrict PDU types.
        // This is a security boundary: any process able to reach the rendezvous
        // channel must not be able to spawn arbitrary elevated processes or
        // otherwise escape the single-pane sandbox. See the `onlyterm-elevated-transport`
        // module doc for full design context.
        log::info!("connecting to WebSocket rendezvous at 127.0.0.1:{}", port);
        let stream = onlyterm_elevated_transport::connect_and_bridge(port, &token)
            .context("failed to connect via WebSocket rendezvous")?;
        log::info!("WebSocket rendezvous connected successfully");
        (stream, PduPolicy::ElevatedSinglePaneAllowList)
    } else {
        // Non-elevated stdio path: wrap inherited stdin as a socket.
        // Must happen before wrapping the inherited handle as a UnixStream --
        // see init_winsock's doc comment.
        // Use Unrestricted policy because the stream can only be reached by a
        // process that inherited the exact socket handle from its parent GUI.
        init_winsock();
        let stream = wrap_stdin_as_stream();
        (stream, PduPolicy::Unrestricted)
    };

    // Dispatch the mux protocol on the stream (schedules the async future).
    // The returned task doubles as this process's "the client is still
    // here" signal: there is no listener in single-pane mode, so the one
    // client that connected (elevated path) or inherited the socket
    // (non-elevated path) can never be replaced. When that task finishes,
    // this process has nobody left to serve.
    let dispatch_task = dispatch_stream(stream, policy);

    // Filled in by the lifecycle task below and consumed by the tick loop:
    // that is how the executor loop learns that it is time to stop.
    //
    // Reporting the outcome back out through `run()`/`main()` rather than
    // calling `std::process::exit` from inside the task is deliberate: it
    // lets `main` run `onlyterm_blob_leases::clear_storage()`, which removes
    // the temp dir registered under `CACHE_DIR` at the top of this
    // function. One leaked temp dir per tab process would otherwise pile
    // up for as long as the user's session lives.
    let outcome: Arc<Mutex<Option<anyhow::Result<()>>>> = Arc::new(Mutex::new(None));

    promise::spawn::spawn({
        let outcome = Arc::clone(&outcome);
        async move {
            let result = async_run_single_pane(cmd, dispatch_task).await;
            outcome.lock().unwrap().replace(result);
        }
    })
    .detach();

    loop {
        executor.tick()?;
        // The tick above may have been the one that completed the
        // lifecycle task; check before parking in `recv()` again, which
        // would otherwise block forever (the `SimpleExecutor`'s sender
        // lives in a global, so its queue never disconnects).
        if let Some(result) = outcome.lock().unwrap().take() {
            return result;
        }
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

/// Wraps the inherited stdin handle (a Winsock SOCKET from the parent's
/// socketpair) as a `UnixStream`. Used for the non-elevated single-pane
/// transport.
///
/// When the parent calls `filedescriptor::socketpair()`, it gets two
/// Winsock SOCKET handles: one it keeps (wrapped as a UnixStream), the
/// other it hands to this child process as stdin/stdout via
/// `cmd.stdin(b.as_stdio()?)`. On Windows, a Winsock SOCKET and a
/// Windows HANDLE are the same underlying value, so the raw handle
/// this process inherited as stdin IS that socket.
fn wrap_stdin_as_stream() -> UnixStream {
    use std::os::windows::io::{AsRawHandle, FromRawSocket};

    let stdin_handle = std::io::stdin().as_raw_handle() as std::os::windows::io::RawSocket;

    // SAFETY: stdin_handle is the Winsock SOCKET inherited from the parent
    // via socketpair() + Stdio (see above); this process took sole
    // ownership of it at spawn time and never uses stdin for anything
    // else, so wrapping it as a UnixStream here does not alias any other
    // owner of the handle.
    unsafe { UnixStream::from_raw_socket(stdin_handle) }
}

/// Schedules the mux protocol dispatcher for the given stream, returning
/// the task that drives it.
///
/// `dispatch::process_async_with_policy` is an `async fn`: calling it only constructs a
/// `Future` and runs none of its body until something actually polls it.
/// The original version of this function did `let _ =
/// dispatch::process(stream);` inside a bare `thread::spawn`, which builds
/// the future and immediately drops it unpolled -- the mux protocol never
/// actually ran, silently. `spawn_into_main_thread` (the same primitive
/// `LocalListener::run` in `onlyterm-mux-server-impl/src/local.rs` uses for
/// every accepted daemon-mode connection) is what actually drives it, by
/// scheduling it onto the `SimpleExecutor` that `run_single_pane_mode`'s
/// `executor.tick()` loop is already polling.
///
/// The task is returned rather than detached, and the dispatcher's error
/// is propagated rather than logged and swallowed, because in single-pane
/// mode "the dispatcher finished" *is* the shutdown trigger for the whole
/// process (see `async_run_single_pane`), and whether it finished cleanly
/// (client closed the connection) or with an error (eg: a failed attach)
/// is what decides this process's exit status.
///
/// Note that `Task` cancels on drop: dropping the returned task tears the
/// dispatcher down and closes the stream, which surfaces to the GUI as an
/// ordinary connection close.
fn dispatch_stream(
    stream: UnixStream,
    policy: onlyterm_mux_server_impl::sessionhandler::PduPolicy,
) -> promise::spawn::Task<anyhow::Result<()>> {
    promise::spawn::spawn_into_main_thread(async move {
        let stream = smol::Async::new(stream)
            .map_err(|e| anyhow::anyhow!("failed to wrap stream: {:#}", e))?;
        onlyterm_mux_server_impl::dispatch::process_async_with_policy(stream, policy).await
    })
}

/// Which of the two "there is nothing left to host" conditions fired
/// first, and how it turned out.
#[derive(Debug)]
enum SinglePaneExit {
    /// `MuxNotification::PaneRemoved` fired for the hosted pane: the shell
    /// exited, or the client asked for the pane to be killed.
    PaneExited(anyhow::Result<()>),
    /// The mux protocol dispatcher finished: the one and only client this
    /// process will ever have is gone, either cleanly (EOF) or with an
    /// error (protocol/IO failure, eg: a rejected attach handshake).
    ClientGone(anyhow::Result<()>),
}

/// Waits for whichever single-pane shutdown trigger fires first.
///
/// Split out from `async_run_single_pane` so that the ordering logic is
/// testable without a live pty: the interesting property is that *either*
/// input winning ends the wait, including the case where one of them was
/// already complete before the wait even started.
async fn wait_for_single_pane_shutdown<P, C>(pane_exited: P, client_gone: C) -> SinglePaneExit
where
    P: Future<Output = anyhow::Result<()>>,
    C: Future<Output = anyhow::Result<()>>,
{
    smol::future::race(
        async move { SinglePaneExit::PaneExited(pane_exited.await) },
        async move { SinglePaneExit::ClientGone(client_gone.await) },
    )
    .await
}

/// Kills this process's one pane and removes it from the mux, mirroring
/// the `Pdu::KillPane` handler in `onlyterm-mux-server-impl`'s
/// `SessionHandler` (which is the only other place that ends a pane's life
/// from inside a mux server).
///
/// `Mux::remove_pane` already calls `Pane::kill` on the pane it removes;
/// calling `kill()` explicitly first mirrors that handler and is harmless
/// (`LocalPane::kill` no-ops once its `killed` flag is set).
///
/// On Windows this is belt-and-braces in the common case rather than the
/// primary reaping mechanism: `pty::win::pseudocon::spawn_command` puts
/// every pty child into a Job Object with
/// `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, and this process holds the only
/// handle to that job (the job is created after `CreateProcessW`, is not
/// inheritable, and the child is merely *assigned* to it, so no other
/// process holds a handle). Process exit therefore closes the last handle
/// and takes the shell -- and any grandchildren it spawned -- down with
/// us. But that setup is explicitly allowed to fail (it logs a warning and
/// continues without a job), and in *that* case nothing else would reap
/// the shell, so killing the pane before exiting is what keeps the "no
/// orphaned elevated shells" guarantee unconditional.
fn kill_hosted_pane(pane_id: PaneId) {
    let mux = Mux::get();
    match mux.get_pane(pane_id) {
        Some(pane) => {
            pane.kill();
            mux.remove_pane(pane_id);
        }
        None => {
            log::debug!("pane {} is already gone; nothing to kill", pane_id);
        }
    }
}

/// Whether a dispatcher that ended with an error merely lost its client,
/// as opposed to actually failing.
///
/// `dispatch::process_async_with_policy` already maps a graceful EOF to
/// `Ok(())`, but a peer that goes away by *exiting* -- which is the normal
/// way a tab closes, and what happens whenever the GUI itself goes down --
/// usually surfaces on Windows as `ConnectionReset`/`ConnectionAborted` on
/// the next read or write rather than as a graceful EOF. Treating those as
/// failures would log an ERROR and give a non-zero exit status on every
/// ordinary tab close, drowning out the disconnects that genuinely are
/// failures (eg: a rejected attach handshake, which is exactly the case
/// that left an orphaned elevated shell behind).
fn is_ordinary_disconnect(err: &anyhow::Error) -> bool {
    match err.root_cause().downcast_ref::<std::io::Error>() {
        Some(err) => matches!(
            err.kind(),
            std::io::ErrorKind::UnexpectedEof
                | std::io::ErrorKind::ConnectionReset
                | std::io::ErrorKind::ConnectionAborted
                | std::io::ErrorKind::BrokenPipe
        ),
        None => false,
    }
}

/// Async portion of single-pane mode: spawn the one pane this process
/// exists to host, then shut the process down as soon as either the pane
/// or the client goes away.
///
/// The pane spawn deliberately runs to completion *before* the two
/// shutdown triggers are raced against each other. Racing `client_gone`
/// against the spawn itself would drop a half-finished spawn future
/// mid-`await`, potentially leaving a freshly created pty child that
/// nothing tracks. Deferring the race costs nothing, because "the client
/// is gone" is a sticky condition rather than an edge: `client_gone` is
/// the dispatcher's `Task`, which holds its output until it is awaited. A
/// client that disconnected while the pane was still being created is
/// therefore observed the instant the race starts -- no polling, no
/// sleeping, and no window in which the event can be missed. (That
/// ordering is not hypothetical: the bug this exists to fix was a client
/// whose attach failed immediately after the transport handshake, ie.
/// while the pane was still being spawned.)
async fn async_run_single_pane(
    cmd: Option<CommandBuilder>,
    client_gone: impl Future<Output = anyhow::Result<()>>,
) -> anyhow::Result<()> {
    let mux = Mux::get();
    let config = config::configuration();

    let domain = mux.default_domain();

    // Spawn exactly ONE pane.
    //
    // The `Activity` guard is scoped to just the spawn: it suppresses
    // `Mux::prune_dead_windows` so that the window created here cannot be
    // pruned as "empty" in the instant before its tab exists. Holding it
    // for the whole life of the process (as this used to, by keeping it
    // alive until this function returned) suppresses pruning *forever* --
    // which silently disables the `exit_behavior = Hold / CloseOnCleanExit`
    // paths in `mux::pty_reader::read_from_pane_pty`, since those report a
    // dead child by calling `prune_dead_windows()` rather than
    // `remove_pane()`. The `PaneRemoved` notification awaited below would
    // then never fire for a shell that exited on its own.
    let pane_id = {
        let _activity = Activity::new();

        let workspace = None;
        let position = None;
        let window_id = mux.new_empty_window(workspace, position);
        domain.attach(Some(*window_id)).await?;

        let _tab = domain
            .spawn(config.initial_size(0, None), cmd, None, *window_id)
            .await?;

        mux.iter_panes()
            .first()
            .map(|p| p.pane_id())
            .ok_or_else(|| anyhow::anyhow!("No pane created"))?
    };

    // Subscribe to pane removal notification to know when the pane exits.
    // There is no `.await` between reading `pane_id` above and subscribing
    // here, and every path that removes a pane does so from a task on this
    // same single-threaded executor, so the notification cannot be missed.
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

    let pane_exited = async move {
        pane_exited_rx
            .recv()
            .await
            .map_err(|e| anyhow::anyhow!("Pane exit channel error: {:?}", e))
    };

    match wait_for_single_pane_shutdown(pane_exited, client_gone).await {
        SinglePaneExit::PaneExited(result) => {
            log::info!("hosted pane {} exited; shutting down", pane_id);
            result
        }
        SinglePaneExit::ClientGone(result) => {
            // Nobody can ever attach to this process again, so the pane it
            // hosts is now unreachable. Left running it would be an
            // invisible orphan holding a live shell -- and for an elevated
            // tab, a live *elevated* shell that the user's own
            // (non-elevated) tools cannot even terminate.
            log::info!(
                "mux protocol client is gone; killing hosted pane {} and shutting down",
                pane_id
            );
            kill_hosted_pane(pane_id);
            // A client that merely went away is an ordinary shutdown
            // (exit 0); a dispatcher failure is not, and `main` turns it
            // into a non-zero exit after logging it.
            match result {
                Ok(()) => Ok(()),
                Err(err) if is_ordinary_disconnect(&err) => {
                    log::debug!("client disconnected: {:#}", err);
                    Ok(())
                }
                Err(err) => Err(err).context("mux protocol dispatch failed"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use promise::spawn::block_on;
    use std::future::{pending, ready};

    /// The ordering that the orphaned-elevated-process bug actually hit:
    /// the dispatcher finished (a failed attach) while the pane was still
    /// being created, so by the time anything waits for a shutdown trigger
    /// the "client is gone" condition has *already* happened. It must
    /// still be observed -- an edge-triggered signal would have been lost
    /// here, and the process would then wait forever for a perfectly
    /// healthy pane to die.
    #[test]
    fn an_already_disconnected_client_is_not_missed() {
        let outcome = block_on(wait_for_single_pane_shutdown(
            pending(),
            ready(Err(anyhow::anyhow!("attach failed"))),
        ));
        match outcome {
            SinglePaneExit::ClientGone(Err(err)) => {
                assert!(
                    err.to_string().contains("attach failed"),
                    "the dispatcher's error must be propagated, got: {:#}",
                    err
                );
            }
            other => panic!("expected ClientGone(Err(..)), got {:?}", other),
        }
    }

    /// A client that disconnects cleanly while the pane is still alive is
    /// still a shutdown trigger, but not an error: the process exits 0.
    #[test]
    fn a_clean_client_disconnect_ends_the_wait_without_an_error() {
        let outcome = block_on(wait_for_single_pane_shutdown(pending(), ready(Ok(()))));
        match outcome {
            SinglePaneExit::ClientGone(Ok(())) => {}
            other => panic!("expected ClientGone(Ok(())), got {:?}", other),
        }
    }

    /// The original, pre-existing exit condition must keep working: the
    /// pane going away ends the wait even though the client is still
    /// connected (its future never completes).
    #[test]
    fn a_pane_exit_ends_the_wait_while_the_client_is_still_connected() {
        let outcome = block_on(wait_for_single_pane_shutdown(ready(Ok(())), pending()));
        match outcome {
            SinglePaneExit::PaneExited(Ok(())) => {}
            other => panic!("expected PaneExited(Ok(())), got {:?}", other),
        }
    }

    /// A peer that vanished (its process exited, taking the socket with
    /// it) must not be reported as a failure: that is what an ordinary tab
    /// close looks like from here. A protocol-level failure -- the case
    /// that actually left an orphaned elevated shell behind -- must still
    /// be reported as one.
    #[test]
    fn only_transport_level_disconnects_count_as_ordinary() {
        for kind in [
            std::io::ErrorKind::UnexpectedEof,
            std::io::ErrorKind::ConnectionReset,
            std::io::ErrorKind::ConnectionAborted,
            std::io::ErrorKind::BrokenPipe,
        ] {
            // Wrapped in context the same way `dispatch` wraps it, so
            // this exercises the `root_cause()` unwrapping too.
            let err = anyhow::Error::new(std::io::Error::new(kind, "peer went away"))
                .context("reading Pdu from client");
            assert!(
                is_ordinary_disconnect(&err),
                "{:?} should be treated as an ordinary disconnect",
                kind
            );
        }

        let err = anyhow::anyhow!("PDU Spawn is not permitted on this channel");
        assert!(
            !is_ordinary_disconnect(&err),
            "a protocol-level rejection is a real failure, not a disconnect"
        );

        let err = anyhow::Error::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "corrupt PDU",
        ));
        assert!(
            !is_ordinary_disconnect(&err),
            "a decode failure is a real failure, not a disconnect"
        );
    }

    /// The pane-side channel breaking (its sender dropped without ever
    /// signalling) must surface as an error rather than hanging: the
    /// process can no longer observe its own pane, so continuing to run
    /// would be exactly the orphan state this whole path exists to
    /// prevent.
    #[test]
    fn a_broken_pane_channel_surfaces_as_an_error() {
        let (tx, rx) = smol::channel::bounded::<()>(1);
        drop(tx);
        let pane_exited = async move {
            rx.recv()
                .await
                .map_err(|e| anyhow::anyhow!("Pane exit channel error: {:?}", e))
        };
        let outcome = block_on(wait_for_single_pane_shutdown(pane_exited, pending()));
        match outcome {
            SinglePaneExit::PaneExited(Err(err)) => {
                assert!(
                    err.to_string().contains("Pane exit channel error"),
                    "unexpected error: {:#}",
                    err
                );
            }
            other => panic!("expected PaneExited(Err(..)), got {:?}", other),
        }
    }
}
