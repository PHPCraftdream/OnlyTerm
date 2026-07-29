#![cfg(unix)]
use anyhow::Context;
use libc::pid_t;
use std::io::Write;
use std::os::unix::io::{AsRawFd, IntoRawFd, RawFd};

enum Fork {
    #[allow(dead_code)]
    Child(pid_t),
    Parent(pid_t),
}

fn fork() -> anyhow::Result<Fork> {
    // SAFETY: `fork` is a standard POSIX call. The child inherits a copy of
    // the address space; only async-signal-safe functions should be called
    // between fork and exec in a multi-threaded process. Here we only call
    // `getpid`, which is async-signal-safe.
    let pid = unsafe { libc::fork() };

    if pid == 0 {
        // We are the child
        // SAFETY: `getpid` has no preconditions.
        let pid = unsafe { libc::getpid() };
        Ok(Fork::Child(pid))
    } else if pid < 0 {
        let err: anyhow::Error = std::io::Error::last_os_error().into();
        Err(err.context("fork"))
    } else {
        // We are the parent
        Ok(Fork::Parent(pid))
    }
}

fn setsid() -> anyhow::Result<()> {
    // SAFETY: `setsid` creates a new session; it is a standard POSIX call
    // with no memory-safety preconditions.
    let pid = unsafe { libc::setsid() };
    if pid == -1 {
        let err: anyhow::Error = std::io::Error::last_os_error().into();
        Err(err.context("setsid"))
    } else {
        Ok(())
    }
}

fn lock_pid_file(config: &config::ConfigHandle) -> anyhow::Result<std::fs::File> {
    let pid_file = config.daemon_options.pid_file();
    let pid_file_dir = pid_file
        .parent()
        .ok_or_else(|| anyhow::anyhow!("{} has no parent?", pid_file.display()))?;
    std::fs::create_dir_all(&pid_file_dir).with_context(|| {
        format!(
            "while creating directory structure: {}",
            pid_file_dir.display()
        )
    })?;
    let file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(&pid_file)
        .with_context(|| format!("opening pid file {}", pid_file.display()))?;
    config::set_sticky_bit(&pid_file);
    // SAFETY: `file.as_raw_fd()` is a valid fd; LOCK_EX | LOCK_NB are valid
    // flock flags for a non-blocking exclusive lock.
    let res = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if res != 0 {
        let err = std::io::Error::last_os_error();
        anyhow::bail!("unable to lock pid file {}: {}", pid_file.display(), err);
    }

    // SAFETY: `file.as_raw_fd()` is a valid fd; truncating to 0 is safe.
    unsafe { libc::ftruncate(file.as_raw_fd(), 0) };

    Ok(file)
}

pub fn daemonize(config: &config::ConfigHandle) -> anyhow::Result<Option<RawFd>> {
    let pid_file = if !config::running_under_wsl() {
        // pid file locking is only partly functional when running under
        // WSL 1; it is possible for the pid file to exist after a reboot
        // and for attempts to open and lock it to fail when there are no
        // other processes that might possibly hold a lock on it.
        // So, we only use a pid file when not under WSL.

        Some(lock_pid_file(config)?)
    } else {
        None
    };
    let stdout = config.daemon_options.open_stdout()?;
    let stderr = config.daemon_options.open_stderr()?;
    let devnull = std::fs::File::open("/dev/null").context("opening /dev/null for read")?;

    match fork()? {
        Fork::Parent(pid) => {
            let mut status = 0;
            // SAFETY: `pid` is a valid child process id returned by fork;
            // `&mut status` is a valid out-pointer.
            unsafe { libc::waitpid(pid, &mut status, 0) };
            std::process::exit(0);
        }
        Fork::Child(_) => {}
    }

    setsid()?;
    match fork()? {
        Fork::Parent(_) => {
            std::process::exit(0);
        }
        Fork::Child(_) => {}
    }

    let pid_file_fd = pid_file.map(|mut pid_file| {
        // SAFETY: `getpid` has no preconditions.
        writeln!(pid_file, "{}", unsafe { libc::getpid() }).ok();
        // Leak it so that the descriptor remains open for the duration
        // of the process runtime
        let fd = pid_file.into_raw_fd();

        // Since we will always re-exec, we need to clear FD_CLOEXEC
        // in order for the pidfile to be inherited in our newly
        // exec'd self
        set_cloexec(fd, false);

        fd
    });

    // SAFETY: `devnull`, `stdout`, `stderr` are valid fds; the STDIN/OUT/ERR
    // constants are valid destination fds.
    unsafe { libc::dup2(devnull.as_raw_fd(), libc::STDIN_FILENO) };
    unsafe { libc::dup2(stdout.as_raw_fd(), libc::STDOUT_FILENO) };
    unsafe { libc::dup2(stderr.as_raw_fd(), libc::STDERR_FILENO) };

    Ok(pid_file_fd)
}

pub fn set_cloexec(fd: RawFd, enable: bool) {
    // SAFETY: `fd` is a valid open file descriptor. F_GETFD reads flags,
    // F_SETFD writes flags.
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFD);
        if flags == -1 {
            return;
        }

        let flags = if enable {
            flags | libc::FD_CLOEXEC
        } else {
            flags & !libc::FD_CLOEXEC
        };

        libc::fcntl(fd, libc::F_SETFD, flags);
    }
}
