// TODO(#415): `prefer_swrast` exists solely to steer the EGL/WGL GL-context
// setup in `egl/state.rs` and `os/windows/wgl.rs` towards Mesa/SWRAST. Both
// of those callers (and the GL context machinery they belong to) are slated
// for removal in #415. The RDP special-case below existed because "using
// OpenGL in RDP has problematic behavior upon disconnect" (task #413 GL
// removal map); now that GL is gone from the config layer, that rationale
// no longer applies, and `FrontEndSelection::Software` (the other input to
// this function) has been removed entirely. Keep this stubbed to `false`
// until #415 deletes the GL-context code that reads it.
pub(crate) fn prefer_swrast() -> bool {
    false
}
