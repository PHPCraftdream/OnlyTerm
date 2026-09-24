#[cfg(windows)]
pub mod windows;
#[cfg(windows)]
pub use self::windows::*;

// Platform-specific backends removed: macOS, X11, Wayland are not used on Windows
// xdg_desktop_portal, x_and_wayland, xkeysyms are Unix-specific and no longer needed

pub mod parameters;
