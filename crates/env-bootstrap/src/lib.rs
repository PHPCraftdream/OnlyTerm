#![warn(clippy::undocumented_unsafe_blocks)]
pub mod ringlog;
pub use ringlog::setup_logger;

pub fn set_onlyterm_executable() {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            std::env::set_var("ONLYTERM_EXECUTABLE_DIR", dir);
        }
        std::env::set_var("ONLYTERM_EXECUTABLE", exe);
    }
}

fn register_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let payload = info.payload();
        // `panic!("literal")` produces a `&'static str` payload, but
        // `panic!("{}", x)` / any `format!`-built message (the common case
        // for panics from dependencies, e.g. wgpu's `default_error_handler`)
        // produces a `String` payload instead -- downcasting only to `&str`
        // silently discarded the real message for every such panic and
        // logged a useless "!?" placeholder instead (confirmed: this is
        // exactly what happened for a real wgpu-error-turned-panic crash,
        // losing the one piece of information -- which wgpu error kind --
        // that would have explained it).
        let message: &str = if let Some(s) = payload.downcast_ref::<&str>() {
            s
        } else if let Some(s) = payload.downcast_ref::<String>() {
            s.as_str()
        } else {
            "<non-string panic payload>"
        };
        let bt = backtrace::Backtrace::new();
        if let Some(loc) = info.location() {
            log::error!(
                "panic at {}:{}:{} - {}\n{:?}",
                loc.file(),
                loc.line(),
                loc.column(),
                message,
                bt
            );
        } else {
            log::error!("panic - {}\n{:?}", message, bt);
        }
        default_hook(info);
    }));
}

/// Keeps the process logger alive through normal shutdown and performs the
/// final worker flush when dropped. The global `log` logger itself is leaked
/// by `set_boxed_logger`, so relying on the logger's `Drop` implementation
/// would never drain the GUI diagnostic worker.
pub struct LogGuard;

impl Drop for LogGuard {
    fn drop(&mut self) {
        log::logger().flush();
    }
}

pub fn bootstrap() -> LogGuard {
    config::assign_version_info(
        onlyterm_version::onlyterm_version(),
        onlyterm_version::onlyterm_target_triple(),
        onlyterm_version::onlyterm_commit_hash(),
        onlyterm_version::onlyterm_commit_count(),
        onlyterm_version::onlyterm_build_time(),
    );
    setup_logger();
    register_panic_hook();

    // First line of every per-PID log file, deliberately: a crash dump
    // (or a WER Application Error entry) names the binary and a raw
    // address, not which commit produced it. Matching a dump to source
    // meant, more than once, hashing the retired binary on disk and
    // grepping `git log` for which commit was HEAD when it was built. The
    // log this process is about to fill in already carries that answer,
    // as long as it's the very first thing written to it.
    log::info!(
        "OnlyTerm {} | commit #{} {} | built {} | {}",
        onlyterm_version::onlyterm_version(),
        onlyterm_version::onlyterm_commit_count(),
        onlyterm_version::onlyterm_commit_hash(),
        onlyterm_version::onlyterm_build_time(),
        onlyterm_version::onlyterm_target_triple(),
    );

    set_onlyterm_executable();

    // Remove this env var to avoid weirdness with some vim configurations.
    // onlyterm never sets WINDOWID and we don't want to inherit it from a
    // parent process.
    std::env::remove_var("WINDOWID");
    // Avoid vte shell integration kicking in if someone started
    // onlyterm or the mux server from inside gnome terminal.
    // <https://github.com/wezterm/wezterm/issues/2237>
    std::env::remove_var("VTE_VERSION");

    // Sice folks don't like to reboot or sign out if they `chsh`,
    // SHELL may be stale. Rather than using a stale value, unset
    // it so that pty::CommandBuilder::get_shell will resolve the
    // shell from the password database instead.
    std::env::remove_var("SHELL");
    LogGuard
}
