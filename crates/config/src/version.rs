use std::sync::OnceLock;

static VERSION: OnceLock<&'static str> = OnceLock::new();
static TRIPLE: OnceLock<&'static str> = OnceLock::new();
static COMMIT_HASH: OnceLock<&'static str> = OnceLock::new();
static COMMIT_COUNT: OnceLock<&'static str> = OnceLock::new();
static BUILD_TIME: OnceLock<&'static str> = OnceLock::new();

pub fn assign_version_info(
    version: &'static str,
    triple: &'static str,
    commit_hash: &'static str,
    commit_count: &'static str,
    build_time: &'static str,
) {
    VERSION.set(version).unwrap();
    TRIPLE.set(triple).unwrap();
    COMMIT_HASH.set(commit_hash).unwrap();
    COMMIT_COUNT.set(commit_count).unwrap();
    BUILD_TIME.set(build_time).unwrap();
}

pub fn onlyterm_version() -> &'static str {
    VERSION
        .get()
        .unwrap_or(&"someone forgot to call assign_version_info")
}

pub fn onlyterm_target_triple() -> &'static str {
    TRIPLE
        .get()
        .unwrap_or(&"someone forgot to call assign_version_info")
}

pub fn onlyterm_commit_hash() -> &'static str {
    COMMIT_HASH
        .get()
        .unwrap_or(&"someone forgot to call assign_version_info")
}

pub fn onlyterm_commit_count() -> &'static str {
    COMMIT_COUNT
        .get()
        .unwrap_or(&"someone forgot to call assign_version_info")
}

pub fn onlyterm_build_time() -> &'static str {
    BUILD_TIME
        .get()
        .unwrap_or(&"someone forgot to call assign_version_info")
}

/// WSL detection (checking `uname` for "microsoft") only ever mattered
/// for a Linux binary of onlyterm running *inside* WSL; OnlyTerm only
/// ships a native Windows binary, which is never itself "under WSL" in
/// that sense.
pub fn running_under_wsl() -> bool {
    false
}
