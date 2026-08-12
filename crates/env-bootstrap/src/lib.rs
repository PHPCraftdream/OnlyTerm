#![warn(clippy::undocumented_unsafe_blocks)]
pub mod ringlog;
pub use ringlog::setup_logger;

pub fn set_wezterm_executable() {
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
        let payload = payload.downcast_ref::<&str>().unwrap_or(&"!?");
        let bt = backtrace::Backtrace::new();
        if let Some(loc) = info.location() {
            log::error!(
                "panic at {}:{}:{} - {}\n{:?}",
                loc.file(),
                loc.line(),
                loc.column(),
                payload,
                bt
            );
        } else {
            log::error!("panic - {}\n{:?}", payload, bt);
        }
        default_hook(info);
    }));
}

pub fn bootstrap() {
    config::assign_version_info(
        wezterm_version::wezterm_version(),
        wezterm_version::wezterm_target_triple(),
        wezterm_version::wezterm_commit_hash(),
        wezterm_version::wezterm_commit_count(),
        wezterm_version::wezterm_build_time(),
    );
    setup_logger();
    register_panic_hook();

    set_wezterm_executable();

    // Remove this env var to avoid weirdness with some vim configurations.
    // wezterm never sets WINDOWID and we don't want to inherit it from a
    // parent process.
    std::env::remove_var("WINDOWID");
    // Avoid vte shell integration kicking in if someone started
    // wezterm or the mux server from inside gnome terminal.
    // <https://github.com/wezterm/wezterm/issues/2237>
    std::env::remove_var("VTE_VERSION");

    // Sice folks don't like to reboot or sign out if they `chsh`,
    // SHELL may be stale. Rather than using a stale value, unset
    // it so that pty::CommandBuilder::get_shell will resolve the
    // shell from the password database instead.
    std::env::remove_var("SHELL");
}
