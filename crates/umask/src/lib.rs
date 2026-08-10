#![warn(clippy::undocumented_unsafe_blocks)]
//! `umask(2)` is a Unix-only concept; OnlyTerm is Windows-only, so this is
//! now just an inert placeholder kept for API compatibility with the
//! callers that construct it during startup.

/// Unfortunately, novice unix users can sometimes be running
/// with an overly permissive umask so we take care to install
/// a more restrictive mask while we might be creating things
/// in the filesystem.
/// This struct locks down the umask for its lifetime, restoring
/// the prior umask when it is dropped.
///
/// On Windows (the only platform this fork targets) there is no
/// umask concept, so this is a no-op.
pub struct UmaskSaver {}

// `UmaskSaver::new()` is not a plain value constructor: on Unix it used to
// mutate the process-wide umask as a side effect and rely on RAII (`Drop`)
// to restore the prior mask. Keeping an explicit `UmaskSaver::new()` call
// (rather than a `Default` impl) preserves that call-site shape even though
// it is now a no-op on Windows.
#[allow(clippy::new_without_default)]
impl UmaskSaver {
    pub fn new() -> Self {
        Self {}
    }
}
