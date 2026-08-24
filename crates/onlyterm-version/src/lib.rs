pub fn onlyterm_version() -> &'static str {
    // See build.rs
    env!("ONLYTERM_CI_TAG")
}

pub fn onlyterm_target_triple() -> &'static str {
    // See build.rs
    env!("ONLYTERM_TARGET_TRIPLE")
}

pub fn onlyterm_commit_hash() -> &'static str {
    // See build.rs
    env!("ONLYTERM_CI_COMMIT_HASH")
}

pub fn onlyterm_commit_count() -> &'static str {
    // See build.rs
    env!("ONLYTERM_CI_COMMIT_COUNT")
}

pub fn onlyterm_build_time() -> &'static str {
    // See build.rs
    env!("ONLYTERM_CI_BUILD_TIME")
}
