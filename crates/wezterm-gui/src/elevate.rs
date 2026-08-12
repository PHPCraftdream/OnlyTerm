//! Elevated window spawn helper for OnlyTerm.
//!
//! On Windows, a non-elevated process cannot spawn an elevated child that
//! shares its existing ConPTY handle. Elevation is only reachable via
//! `ShellExecuteExW` with `lpVerb = "runas"`, which triggers a UAC prompt
//! and produces a process at a different integrity level that cannot share
//! the calling process's PTY/console the normal way.
//!
//! This module provides a helper to spawn a new elevated OnlyTerm window
//! running a specific shell command, with the result indicating success,
//! user cancellation, or failure.
//!
//! Called from the "New Tab Options" dialog's admin path
//! (`crates/wezterm-gui/src/termwindow/newtab_options.rs`), off the GUI
//! thread via `promise::spawn::spawn_into_new_thread` -- this function's
//! own blocking `ShellExecuteExW` call cannot run on the GUI thread's
//! cooperative executor without freezing the whole application for as
//! long as the UAC prompt is up.

use config::ProcessPriority;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;

/// Result of attempting to spawn an elevated window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ElevateResult {
    /// Successfully spawned elevated window.
    Success,
    /// User cancelled the UAC prompt (ERROR_CANCELLED / 1223).
    UserCancelled,
    /// Failed to spawn elevated window with an error message.
    Failed(String),
}

/// Convert a rust string to a NUL-terminated UTF-16 wide string.
fn wide_string(s: &str) -> Vec<u16> {
    OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

/// Construct a command line string suitable for passing as lpParameters
/// to ShellExecuteExW when spawning an elevated OnlyTerm instance.
///
/// The format is: `start -- <shell> <arg1> <arg2> ...`
///
/// Arguments are properly quoted if they contain spaces or special characters.
fn construct_start_command_line(shell_args: &[String]) -> String {
    if shell_args.is_empty() {
        return String::from("start --");
    }

    let mut result = String::from("start --");

    for arg in shell_args {
        result.push(' ');
        // Simple quoting: wrap in quotes if contains spaces or embedded
        // quotes (the embedded-quote case must trigger quoting too, not
        // just escaping -- an unquoted `"` is not valid inside an
        // unquoted argument on the Windows command line).
        if arg.contains(' ') || arg.contains('\t') || arg.contains('"') || arg.is_empty() {
            result.push('"');
            // Escape existing quotes
            for c in arg.chars() {
                if c == '"' {
                    result.push('\\');
                }
                result.push(c);
            }
            result.push('"');
        } else {
            result.push_str(arg);
        }
    }

    result
}

/// Spawn an elevated OnlyTerm window running the specified shell command.
///
/// This function:
/// 1. Gets the path to the current OnlyTerm executable.
/// 2. Constructs a `start -- <shell_args>` command line.
/// 3. Calls `ShellExecuteExW` with `lpVerb = "runas"` to trigger UAC.
/// 4. Returns an `ElevateResult` indicating the outcome.
///
/// # Important Notes
///
/// - This function MUST be called off the GUI thread on a real OS
///   thread (e.g., via `promise::spawn::spawn_into_new_thread`, NOT
///   `promise::spawn::spawn`, which runs on the GUI thread's own
///   cooperative executor) because `ShellExecuteExW("runas")` blocks
///   until the user responds to the UAC prompt.
/// - The spawned elevated window is completely independent of the current
///   process/window and cannot share the PTY.
/// - If the user cancels the UAC prompt, this returns `UserCancelled`
///   (distinct from a hard failure).
///
/// # Parameters
///
/// * `shell_args` - The shell command and arguments to run in the elevated
///   window (e.g., `["cmd.exe", "/k", "echo elevated"]`).
/// * `priority` - The process priority class for the elevated window.
///   Note: This is for future use; the current ShellExecuteExW-based
///   implementation does not support setting priority, so this parameter
///   is accepted but not yet applied. A future implementation may switch
///   to a more sophisticated approach that allows priority setting.
///
/// # Returns
///
/// * `ElevateResult::Success` - Elevated window spawned successfully.
/// * `ElevateResult::UserCancelled` - User cancelled the UAC prompt.
/// * `ElevateResult::Failed(message)` - Failed to spawn with an error message.
pub fn spawn_elevated_window(shell_args: &[String], _priority: ProcessPriority) -> ElevateResult {
    use winapi::shared::winerror::ERROR_CANCELLED;
    use winapi::um::shellapi::ShellExecuteExW;
    use winapi::um::winuser::SW_SHOW;

    // Get the path to the current OnlyTerm executable
    let exe_path = match std::env::current_exe() {
        Ok(path) => path,
        Err(err) => {
            return ElevateResult::Failed(format!("Failed to resolve current executable: {}", err))
        }
    };

    let exe_path_str = match exe_path.to_str() {
        Some(s) => s,
        None => {
            return ElevateResult::Failed("Current executable path is not valid UTF-8".to_string())
        }
    };

    // Construct the command line parameters
    let parameters = construct_start_command_line(shell_args);

    // Convert to wide strings
    let exe_wide = wide_string(exe_path_str);
    let operation = wide_string("runas");
    let params_wide = wide_string(&parameters);

    // Prepare SHELLEXECUTEINFOW
    // SAFETY: SHELLEXECUTEINFOW is a plain C struct of integers/pointers;
    // an all-zero bit pattern is a valid value for every field, and every
    // field this code actually reads (cbSize, lpVerb, lpFile,
    // lpParameters, nShow) is explicitly set below before the struct is
    // passed to ShellExecuteExW.
    let mut sei = unsafe { std::mem::zeroed::<winapi::um::shellapi::SHELLEXECUTEINFOW>() };
    sei.cbSize = std::mem::size_of::<winapi::um::shellapi::SHELLEXECUTEINFOW>() as u32;
    sei.lpVerb = operation.as_ptr();
    sei.lpFile = exe_wide.as_ptr();
    sei.lpParameters = params_wide.as_ptr();
    sei.nShow = SW_SHOW;

    // Call ShellExecuteExW
    // SAFETY: All pointers are valid NUL-terminated wide strings that
    // outlive this synchronous call. cbSize is correctly set to the
    // struct size. Other fields are zero-initialized or set as above.
    let result = unsafe { ShellExecuteExW(&mut sei) };

    if result == 0 {
        // Failed - check for ERROR_CANCELLED (user cancelled UAC prompt)
        // SAFETY: GetLastError() takes no arguments and has no preconditions;
        // it just reads the calling thread's last-error TLS slot.
        let error_code = unsafe { winapi::um::errhandlingapi::GetLastError() };
        if error_code == ERROR_CANCELLED {
            ElevateResult::UserCancelled
        } else {
            let error_msg = format!("ShellExecuteExW failed: error {}", error_code);
            log::error!("spawn_elevated_window: {}", error_msg);
            ElevateResult::Failed(error_msg)
        }
    } else {
        ElevateResult::Success
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wide_string() {
        let s = wide_string("hello");
        assert_eq!(
            s,
            vec!['h' as u16, 'e' as u16, 'l' as u16, 'l' as u16, 'o' as u16, 0]
        );
    }

    #[test]
    fn test_construct_start_command_line_empty() {
        let result = construct_start_command_line(&[]);
        assert_eq!(result, "start --");
    }

    #[test]
    fn test_construct_start_command_line_single() {
        let result = construct_start_command_line(&["cmd.exe".to_string()]);
        assert_eq!(result, "start -- cmd.exe");
    }

    #[test]
    fn test_construct_start_command_line_multiple() {
        let result =
            construct_start_command_line(&["powershell.exe".to_string(), "-NoLogo".to_string()]);
        assert_eq!(result, "start -- powershell.exe -NoLogo");
    }

    #[test]
    fn test_construct_start_command_line_with_spaces() {
        let result =
            construct_start_command_line(&["C:\\Program Files\\Git\\bin\\bash.exe".to_string()]);
        assert_eq!(result, "start -- \"C:\\Program Files\\Git\\bin\\bash.exe\"");
    }

    #[test]
    fn test_construct_start_command_line_with_quotes() {
        let result = construct_start_command_line(&["path\"with\"quotes".to_string()]);
        assert_eq!(result, "start -- \"path\\\"with\\\"quotes\"");
    }
}
