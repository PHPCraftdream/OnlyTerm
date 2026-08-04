pub mod connection;
pub mod event;
mod extra_constants;
mod ime;
mod keyboard_layout;
mod keycodes;
pub mod watchdog;
mod wgl;
pub mod window;

pub use self::window::*;
pub use connection::*;
pub use event::*;
pub use ime::*;

/// Convert a rust string to a windows wide string
pub fn wide_string(s: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

/// Returns true if we are running in an RDP session.
/// See <https://docs.microsoft.com/en-us/windows/win32/termserv/detecting-the-terminal-services-environment>
pub fn is_running_in_rdp_session() -> bool {
    use winapi::shared::minwindef::DWORD;
    use winapi::um::processthreadsapi::{GetCurrentProcessId, ProcessIdToSessionId};
    use winapi::um::winuser::{GetSystemMetrics, SM_REMOTESESSION};
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;

    // SAFETY: GetSystemMetrics is the documented Win32 metrics API; SM_REMOTESESSION
    // is a valid index and the call takes no pointer arguments, so it is safe to call
    // here. A non-zero result means the session is remote (RDP).
    if unsafe { GetSystemMetrics(SM_REMOTESESSION) } != 0 {
        return true;
    }

    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let terminal_server =
        match hklm.open_subkey("SYSTEM\\CurrentControlSet\\Control\\Terminal Server\\") {
            Ok(k) => k,
            Err(_) => return false,
        };

    let glass_session_id: DWORD = match terminal_server.get_value("GlassSessionId") {
        Ok(sess) => sess,
        Err(_) => return false,
    };

    // SAFETY: GetCurrentProcessId takes no arguments and returns the calling
    // process's real DWORD PID (not a pseudo-handle - that's GetCurrentProcess);
    // ProcessIdToSessionId is the documented session-id API and receives a valid
    // PID plus a pointer to our local `current_session` DWORD in which to write.
    unsafe {
        let mut current_session = 0;
        if ProcessIdToSessionId(GetCurrentProcessId(), &mut current_session) != 0 {
            // If we're not the glass session then we're a remote session
            current_session != glass_session_id
        } else {
            false
        }
    }
}
