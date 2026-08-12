pub fn wezterm_version() -> &'static str {
    // See build.rs
    env!("ONLYTERM_CI_TAG")
}

pub fn wezterm_target_triple() -> &'static str {
    // See build.rs
    env!("ONLYTERM_TARGET_TRIPLE")
}

pub fn wezterm_commit_hash() -> &'static str {
    // See build.rs
    env!("ONLYTERM_CI_COMMIT_HASH")
}

pub fn wezterm_commit_count() -> &'static str {
    // See build.rs
    env!("ONLYTERM_CI_COMMIT_COUNT")
}

pub fn wezterm_build_time() -> &'static str {
    // See build.rs
    env!("ONLYTERM_CI_BUILD_TIME")
}
