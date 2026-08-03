//! A Domain represents an instance of a multiplexer.
//! For example, the gui frontend has its own domain,
//! and we can connect to a domain hosted by a mux server
//! that may be local, running "remotely" inside a WSL
//! container or actually remote, running on the other end
//! of an ssh session somewhere.

use crate::localpane::LocalPane;
use crate::pane::{alloc_pane_id, Pane, PaneId};
use crate::tab::{SplitRequest, Tab, TabId};
use crate::window::WindowId;
use crate::Mux;
use anyhow::{bail, Context, Error};
use async_trait::async_trait;
use config::keyassignment::RotationDirection;
use config::{configuration, ExecDomain, SerialDomain, ValueOrFunc, WslDomain};
use downcast_rs::{impl_downcast, Downcast};
use parking_lot::Mutex;
use portable_pty::{native_pty_system, CommandBuilder, ExitStatus, MasterPty, PtySize, PtySystem};
use std::ffi::OsString;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use wezterm_term::TerminalSize;

static DOMAIN_ID: ::std::sync::atomic::AtomicUsize = ::std::sync::atomic::AtomicUsize::new(0);
pub type DomainId = usize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DomainState {
    Detached,
    Attached,
}

pub fn alloc_domain_id() -> DomainId {
    DOMAIN_ID.fetch_add(1, ::std::sync::atomic::Ordering::Relaxed)
}

#[derive(Debug, Clone, PartialEq)]
pub enum SplitSource {
    Spawn {
        command: Option<CommandBuilder>,
        command_dir: Option<String>,
    },
    MovePane(PaneId),
}

#[async_trait(?Send)]
pub trait Domain: Downcast + Send + Sync {
    /// Spawn a new command within this domain
    async fn spawn(
        &self,
        size: TerminalSize,
        command: Option<CommandBuilder>,
        command_dir: Option<String>,
        window: WindowId,
    ) -> anyhow::Result<Arc<Tab>> {
        let pane = self
            .spawn_pane(size, command, command_dir)
            .await
            .context("spawn")?;

        let tab = Arc::new(Tab::new(&size));
        tab.assign_pane(&pane);

        let mux = Mux::get();
        mux.add_tab_and_active_pane(&tab)?;
        mux.add_tab_to_window(&tab, window)?;

        Ok(tab)
    }

    async fn split_pane(
        &self,
        source: SplitSource,
        tab: TabId,
        pane_id: PaneId,
        split_request: SplitRequest,
    ) -> anyhow::Result<Arc<dyn Pane>> {
        let mux = Mux::get();
        let tab = match mux.get_tab(tab) {
            Some(t) => t,
            None => anyhow::bail!("Invalid tab id {}", tab),
        };

        let pane_index = match tab
            .iter_panes_ignoring_zoom()
            .iter()
            .find(|p| p.pane.pane_id() == pane_id)
        {
            Some(p) => p.index,
            None => anyhow::bail!("invalid pane id {}", pane_id),
        };

        let split_size = match tab.compute_split_size(pane_index, split_request) {
            Some(s) => s,
            None => anyhow::bail!("invalid pane index {}", pane_index),
        };

        let pane = match source {
            SplitSource::Spawn {
                command,
                command_dir,
            } => {
                self.spawn_pane(split_size.second, command, command_dir)
                    .await?
            }
            SplitSource::MovePane(src_pane_id) => {
                let (_domain, _window, src_tab) = mux
                    .resolve_pane_id(src_pane_id)
                    .ok_or_else(|| anyhow::anyhow!("pane {} not found", src_pane_id))?;
                let src_tab = match mux.get_tab(src_tab) {
                    Some(t) => t,
                    None => anyhow::bail!("Invalid tab id {}", src_tab),
                };

                let pane = src_tab.remove_pane(src_pane_id).ok_or_else(|| {
                    anyhow::anyhow!("pane {} not found in its containing tab!?", src_pane_id)
                })?;

                if src_tab.is_dead() {
                    mux.remove_tab(src_tab.tab_id());
                }

                pane
            }
        };

        // pane_index may have changed if src_pane was also in the same tab
        let final_pane_index = match tab
            .iter_panes_ignoring_zoom()
            .iter()
            .find(|p| p.pane.pane_id() == pane_id)
        {
            Some(p) => p.index,
            None => anyhow::bail!("invalid pane id {}", pane_id),
        };

        tab.split_and_insert(final_pane_index, split_request, Arc::clone(&pane))?;
        Ok(pane)
    }

    async fn spawn_pane(
        &self,
        size: TerminalSize,
        command: Option<CommandBuilder>,
        command_dir: Option<String>,
    ) -> anyhow::Result<Arc<dyn Pane>>;

    /// The mux will call this method on the domain of the pane that
    /// is being moved to give the domain a chance to handle the movement.
    /// If this method returns Ok(None), then the mux will handle the
    /// movement itself by mutating its local Tabs and Windows.
    async fn remote_move_pane_to_new_tab(
        &self,
        _pane_id: PaneId,
        _window_id: Option<WindowId>,
        _workspace_for_new_window: Option<String>,
    ) -> anyhow::Result<Option<(Arc<Tab>, WindowId)>> {
        Ok(None)
    }

    /// The mux will call this method on the domain of the panes that are being
    /// rotated to give the domain a chance to handle the movement. If this
    /// method returns Ok(false), then the mux will handle the movement itself
    /// by mutating its local Tabs and Windows.
    async fn remote_rotate_panes(
        &self,
        _pane_id: PaneId,
        _direction: RotationDirection,
    ) -> anyhow::Result<bool> {
        Ok(false)
    }

    /// The mux will call this method on the domain of the pane that is being
    /// swapped to give the domain a chance to handle the movement. If this
    /// method returns Ok(false), then the mux will handle the movement itself
    /// by mutating its local Tabs and Windows.
    async fn remote_swap_active_pane_with_index(
        &self,
        _active_pane_id: PaneId,
        _with_pane_index: usize,
        _keep_focus: bool,
    ) -> anyhow::Result<bool> {
        Ok(false)
    }

    /// Returns false if the `spawn` method will never succeed.
    /// There are some internal placeholder domains that are
    /// pre-created with local UI that we do not want to allow
    /// to show in the launcher/menu as launchable items.
    fn spawnable(&self) -> bool {
        true
    }

    /// Returns true if the `detach` method can be used
    /// to detach the domain, preserving the associated
    /// panes, or false if the `detach` method will never
    /// succeed
    fn detachable(&self) -> bool;

    /// Returns the domain id, which is useful for obtaining
    /// a handle on the domain later.
    fn domain_id(&self) -> DomainId;

    /// Returns the name of the domain.
    /// Should be a short identifier.
    fn domain_name(&self) -> &str;

    /// Returns a label describing the domain.
    async fn domain_label(&self) -> String {
        self.domain_name().to_string()
    }

    /// Re-attach to any tabs that might be pre-existing in this domain
    async fn attach(&self, window_id: Option<WindowId>) -> anyhow::Result<()>;

    /// Detach all tabs
    fn detach(&self) -> anyhow::Result<()>;

    /// Indicates the state of the domain
    fn state(&self) -> DomainState;
}
impl_downcast!(Domain);

pub struct LocalDomain {
    pty_system: Mutex<Box<dyn PtySystem + Send>>,
    id: DomainId,
    name: String,
}

impl LocalDomain {
    pub fn new(name: &str) -> Result<Self, Error> {
        Ok(Self::with_pty_system(name, native_pty_system()))
    }

    fn resolve_exec_domain(&self) -> Option<ExecDomain> {
        config::configuration()
            .exec_domains
            .iter()
            .find(|ed| ed.name == self.name)
            .cloned()
    }

    fn resolve_wsl_domain(&self) -> Option<WslDomain> {
        let config = config::configuration();

        // `Config::wsl_domains()` falls back to `WslDomain::default_domains()`
        // when the user hasn't explicitly configured `wsl_domains`, and that
        // fallback shells out to `wsl.exe -l -v` synchronously (see
        // `crates/config/src/wsl.rs`). That subprocess call can take hundreds
        // of milliseconds to a few seconds (LxssManager/WSL2 VM cold start),
        // and this function is on the hot path of every single pane spawn
        // via `build_command`/`fixup_command` -- including for the plain
        // "local" domain, whose name can never match an auto-discovered
        // "WSL:<distro>" entry anyway (task #328: this call alone accounted
        // for ~850ms of `build_command`'s time when spawning a local pane).
        //
        // Only pay for the (possibly expensive) enumeration when there is a
        // real chance of a match:
        //  - the user explicitly listed `wsl_domains` in their config, which
        //    is already in-memory and cheap (`Config::wsl_domains()` just
        //    clones the `Vec` in that case), or
        //  - `self.name` uses the "WSL:" prefix that auto-discovery always
        //    assigns (see `WslDomain::default_domains`), which is the only
        //    way a `LocalDomain` can end up with such a name.
        // Any other domain name (e.g. "local", exec domains, serial domains)
        // structurally cannot be a WSL domain, so skip the lookup entirely.
        if config.wsl_domains.is_none() && !self.name.starts_with("WSL:") {
            return None;
        }

        config
            .wsl_domains()
            .iter()
            .find(|d| d.name == self.name)
            .cloned()
    }

    pub fn with_pty_system(name: &str, pty_system: Box<dyn PtySystem + Send>) -> Self {
        let id = alloc_domain_id();
        Self {
            pty_system: Mutex::new(pty_system),
            id,
            name: name.to_string(),
        }
    }

    pub fn new_wsl(wsl: WslDomain) -> Result<Self, Error> {
        Self::new(&wsl.name)
    }

    pub fn new_exec_domain(exec_domain: ExecDomain) -> anyhow::Result<Self> {
        Self::new(&exec_domain.name)
    }

    pub fn new_serial_domain(serial_domain: SerialDomain) -> anyhow::Result<Self> {
        let port = serial_domain.port.as_ref().unwrap_or(&serial_domain.name);
        let mut serial = portable_pty::serial::SerialTty::new(&port);
        if let Some(baud) = serial_domain.baud {
            serial.set_baud_rate(baud as u32);
        }
        let pty_system = Box::new(serial);
        Ok(Self::with_pty_system(&serial_domain.name, pty_system))
    }

    #[cfg(unix)]
    fn is_conpty(&self) -> bool {
        false
    }

    #[cfg(windows)]
    fn is_conpty(&self) -> bool {
        let pty_system = self.pty_system.lock();
        let pty_system: &dyn PtySystem = &**pty_system;
        pty_system
            .downcast_ref::<portable_pty::win::conpty::ConPtySystem>()
            .is_some()
    }

    async fn fixup_command(&self, cmd: &mut CommandBuilder) -> anyhow::Result<()> {
        if let Some(wsl) = self.resolve_wsl_domain() {
            let mut args: Vec<OsString> = cmd.get_argv().clone();

            if args.is_empty() {
                if let Some(def_prog) = &wsl.default_prog {
                    for arg in def_prog {
                        args.push(arg.into());
                    }
                }
            }

            let mut argv: Vec<OsString> = vec![
                "wsl.exe".into(),
                "--distribution".into(),
                wsl.distribution
                    .as_deref()
                    .unwrap_or(wsl.name.as_str())
                    .into(),
            ];

            if let Some(cwd) = cmd.get_cwd() {
                argv.push("--cd".into());
                argv.push(cwd.into());
            }

            if let Some(user) = &wsl.username {
                argv.push("--user".into());
                argv.push(user.into());
            }

            if !args.is_empty() {
                argv.push("--exec".into());
                for arg in args {
                    argv.push(arg);
                }
            }

            // TODO: process env list and update WLSENV so that they
            // get passed through

            cmd.clear_cwd();
            *cmd.get_argv_mut() = argv;
        } else if let Some(ed) = self.resolve_exec_domain() {
            // `ed.fixup_command` used to name a rhai function dispatched
            // here (via `with_rhai_config_on_main_thread`/
            // `emit_async_callback`) to rewrite the command being spawned
            // in this exec domain. With the scripting layer removed there
            // is no handler left to call, so the command built above is
            // used unmodified -- this is the same "no handler registered"
            // default the bridge itself already documented.
            let _ = &ed.fixup_command;
        } else if Path::new("/.flatpak-info").exists() {
            // We're running inside a flatpak sandbox.
            // Run the command outside the sandbox via flatpak-spawn
            let mut args = vec![
                "flatpak-spawn".to_string(),
                "--host".to_string(),
                "--watch-bus".to_string(),
            ];
            if let Some(cwd) = cmd.get_cwd() {
                args.push(format!("--directory={}", Path::new(cwd).display()));
            }

            let is_default_prog = cmd.is_default_prog();

            // Note: ONLYTERM_UNIX_SOCKET, ONLYTERM_CONFIG_(FILE|DIR) and other env
            // vars are not included in this.
            // We can't include them: their paths are only meaningful in the sandbox
            // and cannot be reasonably accessed from outside it in the shell.
            for (k, v) in cmd.iter_extra_env_as_str() {
                args.push(format!("--env={k}={v}"));
            }

            for arg in cmd.get_argv() {
                args.push(
                    arg.to_str()
                        .ok_or_else(|| anyhow::anyhow!("command argument is not utf8"))?
                        .to_string(),
                );
            }

            if is_default_prog {
                // We can't read $SHELL from inside the sandbox, so ask the host.
                let output = std::process::Command::new("flatpak-spawn")
                    .args(["--host", "sh", "-c", "echo $SHELL"])
                    .output()?;
                let shell = String::from_utf8_lossy(&output.stdout);

                args.push(shell.trim().to_string());
                // Assume we can pass `-l` for a login shell
                args.push("-l".to_string());
            }

            // Avoid setting up the controlling tty as that is not compatible
            // with flatpak:
            // <https://github.com/flatpak/flatpak/issues/3697>
            // <https://github.com/flatpak/flatpak/issues/3285>
            cmd.set_controlling_tty(false);

            // Re-apply to the builder
            cmd.get_argv_mut().clear();
            for arg in args {
                cmd.get_argv_mut().push(arg.into());
            }
            cmd.clear_cwd();
            log::trace!("made: {cmd:#?}");
        } else if let Some(dir) = cmd.get_cwd() {
            // I'm not normally a fan of existence checking, but not checking here
            // can be painful; in the case where a tab is local but has connected
            // to a remote system and that remote has used OSC 7 to set a path
            // that doesn't exist on the local system, process spawning can fail.
            // Another situation is `sudo -i` has the pane with set to a cwd
            // that is not accessible to the user.
            if let Err(err) = Path::new(&dir).read_dir() {
                log::warn!(
                    "Directory {:?} is not readable and will not be \
                     used for the command we are spawning: {:#}",
                    dir,
                    err
                );
                cmd.clear_cwd();
            }
        }
        Ok(())
    }

    async fn build_command(
        &self,
        command: Option<CommandBuilder>,
        command_dir: Option<String>,
        pane_id: PaneId,
    ) -> anyhow::Result<CommandBuilder> {
        let config = configuration();

        let wsl = self.resolve_wsl_domain();
        let default_prog = wsl
            .as_ref()
            .map(|wsl| wsl.default_prog.as_ref())
            .unwrap_or(config.default_prog.as_ref());

        let mut cmd = match command {
            Some(mut cmd) => {
                config.apply_cmd_defaults(&mut cmd, default_prog, config.default_cwd.as_ref());
                cmd
            }
            None => config.build_prog(
                None,
                default_prog,
                wsl.as_ref()
                    .map(|wsl| wsl.default_cwd.as_ref())
                    .unwrap_or(config.default_cwd.as_ref()),
            )?,
        };
        if let Some(dir) = command_dir {
            cmd.cwd(dir);
        }
        if let Ok(sock) = std::env::var("ONLYTERM_UNIX_SOCKET") {
            cmd.env("ONLYTERM_UNIX_SOCKET", sock);
        }
        cmd.env("ONLYTERM_PANE", pane_id.to_string());
        if let Some(agent) = Mux::get().agent.as_ref() {
            cmd.env("SSH_AUTH_SOCK", agent.path());
        }
        self.fixup_command(&mut cmd).await?;
        Ok(cmd)
    }
}

/// Allows sharing the writer between the Pane and the Terminal.
/// This could potentially be eliminated in the future if we can
/// teach the Pane impl to reference the writer in the Termninal,
/// but the Pane trait returns a RefMut and that makes it a bit
/// awkward at the moment.
///
/// This is a non-blocking, thread-backed wrapper over the real
/// (blocking) pty writer, in the same spirit as `wezterm_term`'s private
/// `ThreadedWriter`: `write`/`flush` here just enqueue onto an unbounded
/// channel and return immediately; a single detached background thread
/// (one per pane, spawned in `WriterWrapper::new`) drains the channel and
/// performs the real, potentially-blocking writes.
///
/// Why this exists: `Pane::writer()` (`crates/mux/src/localpane.rs`) hands
/// out a lock guard directly over a `WriterWrapper` clone, and roughly a
/// dozen call sites across `wezterm-gui` (paste, `SendString`, IME
/// composition, character-picker insertion, quick-select, ...) call
/// `pane.writer().write_all(...)` synchronously from the GUI thread. The
/// old implementation here was a direct, blocking pass-through
/// (`self.writer.lock().write(buf)` = a real `WriteFile`/pipe write): if
/// the child process wasn't reading its stdin (e.g. a full pipe buffer),
/// any of those call sites could block the GUI thread forever and freeze
/// every window in the process -- the same class of bug fixed for
/// `LocalPane::kill()`'s soft-interrupt write, just not limited to
/// `kill()`.
///
/// Every clone of a `WriterWrapper` shares the same `Sender` and so
/// enqueues onto the *same* background thread/queue. This matters because
/// `LocalDomain::spawn_pane` and `TmuxDomain`'s pane spawn (the two
/// constructors of a `WriterWrapper`) both hand one clone to the `Pane`
/// impl (`LocalPane.writer`) and a second clone into `Terminal::new`
/// (wrapped in `wezterm_term`'s own internal writer machinery): keeping
/// them on one shared thread/queue preserves whatever relative ordering
/// they already had (there was never a *strict* ordering guarantee
/// between the two independent paths, and this change doesn't need to
/// invent one -- see `TerminalState::new_with_nonblocking_writer`, which
/// `Terminal::new_with_nonblocking_writer` uses here specifically to
/// avoid wrapping this already-non-blocking writer in a second, redundant
/// thread/queue of its own).
///
/// Queue depth is intentionally unbounded, matching `ThreadedWriter`:
/// bounding it would only trade an already-vanishingly-unlikely
/// unbounded-memory-growth risk for a very real risk of turning a
/// slow/stuck child process back into a mechanism that can block a caller
/// (once a bounded channel is full, `send` either blocks or the caller
/// has to drop data) -- exactly what this type exists to avoid.
///
/// A real write failure (the pty is gone or broken) can now only be
/// observed asynchronously, on the background thread, well after
/// `write`/`flush` already returned `Ok` to the caller. That failure is
/// not silently swallowed: it's logged once (further failures are not
/// re-logged, since once the real writer is broken every subsequent
/// write will fail the same way and the pane's process exiting will
/// naturally surface via the existing, independent `is_dead()` /
/// `child_waiter` machinery in `LocalPane` -- there is no need for this
/// type to also reach back into the pane to mark it dead).
#[derive(Clone)]
pub(crate) struct WriterWrapper {
    sender: std::sync::mpsc::Sender<WriterWrapperMessage>,
}

enum WriterWrapperMessage {
    Data(Vec<u8>),
    Flush,
}

impl WriterWrapper {
    pub fn new(mut writer: Box<dyn Write + Send>) -> Self {
        let (sender, receiver) = std::sync::mpsc::channel::<WriterWrapperMessage>();

        let builder = std::thread::Builder::new().name("pane-writer".into());
        if let Err(err) = builder.spawn(move || {
            let mut failed = false;
            while let Ok(msg) = receiver.recv() {
                if failed {
                    // The real writer already failed once; every
                    // subsequent write will fail the same way (broken
                    // pipe / gone pty). Keep draining the channel so
                    // senders never block or error out on a closed
                    // channel, but don't attempt more real I/O or spam
                    // the log.
                    continue;
                }
                let result = match msg {
                    WriterWrapperMessage::Data(buf) => writer.write_all(&buf),
                    WriterWrapperMessage::Flush => writer.flush(),
                };
                if let Err(err) = result {
                    log::error!(
                        "pane writer thread: write to pty failed, pty is likely \
                         gone; further writes to this pane will be silently \
                         discarded (the pane's process exiting will surface \
                         normally via the usual exit-status path): {:#}",
                        err
                    );
                    failed = true;
                }
            }
        }) {
            // Spawning a thread should essentially never fail (it means
            // the OS is out of resources); if it does, fall back to a
            // wrapper with no live receiver, so writes/flushes below
            // still return promptly (as a `BrokenPipe` error) instead of
            // panicking, rather than trying to do a blocking write here.
            log::error!(
                "Failed to spawn pane-writer thread; pane writes will fail: {:#}",
                err
            );
        }

        Self { sender }
    }
}

impl std::io::Write for WriterWrapper {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.sender
            .send(WriterWrapperMessage::Data(buf.to_vec()))
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::BrokenPipe, err))?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.sender
            .send(WriterWrapperMessage::Flush)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::BrokenPipe, err))?;
        Ok(())
    }
}

/// Stand-in `MasterPty` for the case where `PtySystem::openpty()` itself
/// failed, so there is no real pty (unlike `FailedSpawnPty`, which still
/// wraps a genuine, successfully-created pty whose *command spawn* failed).
/// This exists purely so `LocalPane::new` has something to hold; every
/// operation is a harmless no-op/empty-result since nothing is ever going to
/// read from or write through it (the pane's writer is a `std::io::sink()`
/// and its "child" is `FailedProcessSpawn`, both wired up by the
/// `openpty()`-failure branch in `LocalDomain::spawn_pane`).
pub(crate) struct NoPty {}

impl portable_pty::MasterPty for NoPty {
    fn resize(&self, _new_size: PtySize) -> anyhow::Result<()> {
        Ok(())
    }
    fn get_size(&self) -> anyhow::Result<PtySize> {
        Ok(PtySize::default())
    }
    fn try_clone_reader(&self) -> anyhow::Result<Box<dyn std::io::Read + Send + 'static>> {
        Ok(Box::new(std::io::empty()))
    }
    fn take_writer(&self) -> anyhow::Result<Box<dyn std::io::Write + Send + 'static>> {
        Ok(Box::new(std::io::sink()))
    }

    #[cfg(unix)]
    fn process_group_leader(&self) -> Option<i32> {
        None
    }

    #[cfg(unix)]
    fn as_raw_fd(&self) -> Option<std::os::fd::RawFd> {
        None
    }

    #[cfg(unix)]
    fn tty_name(&self) -> Option<std::path::PathBuf> {
        None
    }
}

/// Wraps the underlying pty; we use this as a marker for when
/// the spawn attempt failed in order to hold the pane open
pub(crate) struct FailedSpawnPty {
    inner: Mutex<Box<dyn MasterPty>>,
}

impl portable_pty::MasterPty for FailedSpawnPty {
    fn resize(&self, new_size: PtySize) -> anyhow::Result<()> {
        self.inner.lock().resize(new_size)
    }
    fn get_size(&self) -> anyhow::Result<PtySize> {
        self.inner.lock().get_size()
    }
    fn try_clone_reader(&self) -> anyhow::Result<Box<dyn std::io::Read + Send + 'static>> {
        self.inner.lock().try_clone_reader()
    }
    fn take_writer(&self) -> anyhow::Result<Box<dyn std::io::Write + Send + 'static>> {
        self.inner.lock().take_writer()
    }

    #[cfg(unix)]
    fn process_group_leader(&self) -> Option<i32> {
        None
    }

    #[cfg(unix)]
    fn as_raw_fd(&self) -> Option<std::os::fd::RawFd> {
        None
    }

    #[cfg(unix)]
    fn tty_name(&self) -> Option<std::path::PathBuf> {
        None
    }
}

/// A fake child process for the case where the spawn attempt
/// failed. It reports as immediately terminated.
#[derive(Debug)]
pub(crate) struct FailedProcessSpawn {}

impl portable_pty::Child for FailedProcessSpawn {
    fn try_wait(&mut self) -> std::io::Result<Option<ExitStatus>> {
        Ok(Some(ExitStatus::with_exit_code(1)))
    }

    fn wait(&mut self) -> std::io::Result<ExitStatus> {
        Ok(ExitStatus::with_exit_code(1))
    }

    fn process_id(&self) -> Option<u32> {
        None
    }

    #[cfg(windows)]
    fn as_raw_handle(&self) -> Option<std::os::windows::io::RawHandle> {
        None
    }
}

impl portable_pty::ChildKiller for FailedProcessSpawn {
    fn kill(&mut self) -> std::io::Result<()> {
        Ok(())
    }
    fn clone_killer(&self) -> Box<dyn portable_pty::ChildKiller + Send + Sync> {
        Box::new(FailedProcessSpawn {})
    }
}

#[async_trait(?Send)]
impl Domain for LocalDomain {
    async fn spawn_pane(
        &self,
        size: TerminalSize,
        command: Option<CommandBuilder>,
        command_dir: Option<String>,
    ) -> anyhow::Result<Arc<dyn Pane>> {
        let pane_id = alloc_pane_id();
        let cmd = self
            .build_command(command, command_dir, pane_id)
            .await
            .context("build_command")?;

        let command_line = cmd
            .as_unix_command_line()
            .unwrap_or_else(|err| format!("error rendering command line: {:?}", err));
        let command_description = format!(
            "\"{}\" in domain \"{}\"",
            if command_line.is_empty() {
                cmd.get_shell()
            } else {
                command_line
            },
            self.name
        );

        // `openpty()` itself can fail (eg. task #326: on some Windows
        // versions, falling back to the in-box ConPTY because a sideloaded
        // `conpty.dll`/`OpenConsole.exe` is missing produces a pseudo-console
        // handle that `CreateProcessW` then rejects with
        // `ERROR_INVALID_HANDLE`). That failure used to propagate via `?`
        // all the way up through `Domain::spawn`/`async_run_terminal_gui` to
        // the top-level error handler in `wezterm-gui`, which tears down the
        // whole process after a toast notification -- silently destroying
        // whatever window had already been created, rather than showing the
        // user anything resembling the normal failed-command-spawn pane
        // below. Handle it the same way instead: synthesize a dead pane
        // that has no real pty at all and just displays the error, so the
        // user always ends up looking at an explanation instead of a
        // vanished window or (with no window yet created) an unexplained
        // process exit.
        let pair = match self
            .pty_system
            .lock()
            .openpty(crate::terminal_size_to_pty_size(size)?)
        {
            Ok(pair) => pair,
            Err(err) => {
                let writer = WriterWrapper::new(Box::new(std::io::sink()));
                let mut terminal = wezterm_term::Terminal::new_with_nonblocking_writer(
                    size,
                    std::sync::Arc::new(config::TermConfig::new()),
                    "WezTerm",
                    config::wezterm_version(),
                    Box::new(writer.clone()),
                );
                if self.is_conpty() {
                    terminal.enable_conpty_quirks();
                }

                // Push the message straight into the terminal rather than
                // through `writer`, as the failed-command-spawn branch
                // below does. That branch's writer is the master end of a
                // real pty, so what it writes comes back around through the
                // pseudo console and lands on screen; here there is no pty
                // at all and the writer is a `sink()`, so writing to it
                // would silently discard the one thing this pane exists to
                // say.
                terminal.advance_bytes(format!(
                    "failed to create a pseudo console: {err:#}\r\n"
                ));

                let pane: Arc<dyn Pane> = Arc::new(LocalPane::new(
                    pane_id,
                    terminal,
                    Box::new(FailedProcessSpawn {}),
                    Box::new(NoPty {}),
                    Box::new(writer),
                    self.id,
                    command_description,
                ));
                let mux = Mux::get();
                mux.add_pane(&pane)?;
                return Ok(pane);
            }
        };

        let child_result = pair.slave.spawn_command(cmd);
        let mut writer = WriterWrapper::new(pair.master.take_writer()?);

        // `WriterWrapper` is already non-blocking (see its doc comment),
        // so use `new_with_nonblocking_writer` here rather than `new`: the
        // latter would wrap this writer in a second, redundant
        // `ThreadedWriter` of its own, putting `Terminal`'s internal
        // writes and this pane's `writer()` writes on two independent
        // background threads instead of the single shared one.
        let mut terminal = wezterm_term::Terminal::new_with_nonblocking_writer(
            size,
            std::sync::Arc::new(config::TermConfig::new()),
            "WezTerm",
            config::wezterm_version(),
            Box::new(writer.clone()),
        );
        if self.is_conpty() {
            terminal.enable_conpty_quirks();
        }

        let pane: Arc<dyn Pane> = match child_result {
            Ok(child) => Arc::new(LocalPane::new(
                pane_id,
                terminal,
                child,
                pair.master,
                Box::new(writer),
                self.id,
                command_description,
            )),
            Err(err) => {
                // Show the error to the user in the new pane
                write!(writer, "{err:#}").ok();

                // and return a dummy pane that has exited
                Arc::new(LocalPane::new(
                    pane_id,
                    terminal,
                    Box::new(FailedProcessSpawn {}),
                    Box::new(FailedSpawnPty {
                        inner: Mutex::new(pair.master),
                    }),
                    Box::new(writer),
                    self.id,
                    command_description,
                ))
            }
        };

        let mux = Mux::get();
        mux.add_pane(&pane)?;

        Ok(pane)
    }

    fn domain_id(&self) -> DomainId {
        self.id
    }

    fn domain_name(&self) -> &str {
        &self.name
    }

    async fn domain_label(&self) -> String {
        if let Some(ed) = self.resolve_exec_domain() {
            match &ed.label {
                Some(ValueOrFunc::Value(wezterm_dynamic::Value::String(s))) => s.to_string(),
                // `ValueOrFunc::Func` used to name a rhai function
                // dispatched here (via `with_rhai_config_on_main_thread`/
                // `emit_async_callback`) to compute the label. With the
                // scripting layer removed there is no handler left to
                // call, so this now always takes the same fallback the
                // old code took when the call errored: the domain's name.
                Some(ValueOrFunc::Func(_label_func)) => self.name.to_string(),
                _ => self.name.to_string(),
            }
        } else if let Some(wsl) = self.resolve_wsl_domain() {
            wsl.distribution.unwrap_or_else(|| self.name.to_string())
        } else {
            self.name.to_string()
        }
    }

    async fn attach(&self, _window_id: Option<WindowId>) -> anyhow::Result<()> {
        Ok(())
    }

    fn detachable(&self) -> bool {
        false
    }

    fn detach(&self) -> anyhow::Result<()> {
        bail!("detach not implemented for LocalDomain");
    }

    fn state(&self) -> DomainState {
        DomainState::Attached
    }
}
