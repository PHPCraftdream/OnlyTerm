use std::sync::OnceLock;

static VERSION: OnceLock<&'static str> = OnceLock::new();
static TRIPLE: OnceLock<&'static str> = OnceLock::new();

pub fn assign_version_info(version: &'static str, triple: &'static str) {
    VERSION.set(version).unwrap();
    TRIPLE.set(triple).unwrap();
}

pub fn wezterm_version() -> &'static str {
    VERSION
        .get()
        .unwrap_or(&"someone forgot to call assign_version_info")
}

pub fn wezterm_target_triple() -> &'static str {
    TRIPLE
        .get()
        .unwrap_or(&"someone forgot to call assign_version_info")
}

/// WSL detection (checking `uname` for "microsoft") only ever mattered
/// for a Linux binary of wezterm running *inside* WSL; OnlyTerm only
/// ships a native Windows binary, which is never itself "under WSL" in
/// that sense.
pub fn running_under_wsl() -> bool {
    false
}
