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
use std::time::Instant;

/// Result of attempting to spawn an elevated single-pane tab.
#[derive(Debug)]
#[allow(dead_code)]
pub enum ElevatedSinglePaneResult {
    /// The elevated child connected and authenticated; here is the live stream.
    #[allow(dead_code)]
    Success(wezterm_uds::UnixStream),
    /// User declined the UAC prompt (ERROR_CANCELLED / 1223).
    UserCancelled,
    /// Spawn, connect, or handshake failed, with a human-readable reason.
    #[allow(dead_code)]
    Failed(String),
}

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

/// Quote a single command-line argument for Windows shell parsing.
/// Wraps in double quotes and escapes embedded quotes with backslashes.
fn quote_arg(arg: &str) -> String {
    if arg.contains(' ') || arg.contains('\t') || arg.contains('"') || arg.is_empty() {
        let mut result = String::new();
        result.push('"');
        for c in arg.chars() {
            if c == '"' {
                result.push('\\');
            }
            result.push(c);
        }
        result.push('"');
        result
    } else {
        arg.to_string()
    }
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
        result.push_str(&quote_arg(arg));
    }

    result
}

/// Construct a command line string for spawning an elevated single-pane child.
///
/// The format is: `--single-pane --connect-ws-port <port> --token <token> [--cwd <dir>]
/// [--priority <name>] [-- <shell> <arg1> <arg2> ...]`
///
/// Arguments are properly quoted if they contain spaces or special characters.
fn construct_single_pane_command_line(
    port: u16,
    token: &str,
    cwd: Option<&std::path::Path>,
    priority: ProcessPriority,
    shell_args: &[String],
) -> String {
    let mut result = format!("--single-pane --connect-ws-port {} --token {}", port, token);

    if let Some(cwd) = cwd {
        if let Some(cwd_str) = cwd.to_str() {
            result.push_str(" --cwd ");
            result.push_str(&quote_arg(cwd_str));
        }
    }

    #[cfg(windows)]
    if priority != ProcessPriority::Normal {
        result.push_str(" --priority ");
        result.push_str(&format!("{:?}", priority));
    }

    if !shell_args.is_empty() {
        result.push_str(" --");
        for arg in shell_args {
            result.push(' ');
            result.push_str(&quote_arg(arg));
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

/// Spawn an elevated single-pane tab and wait for the WebSocket rendezvous.
///
/// This function:
/// 1. Binds a WebSocket rendezvous listener (`RendezvousListener::bind()`).
/// 2. Resolves the path to `onlyterm-mux-server.exe`.
/// 3. Constructs a `--single-pane --connect-ws-port <port> --token <token>`
///    command line (plus optional `--cwd`, `--priority`, and shell args).
/// 4. Calls `ShellExecuteExW` with `lpVerb = "runas"` to trigger UAC.
/// 5. Waits for the elevated child to connect back to the rendezvous listener.
/// 6. Returns the connected stream, or an error if the user cancelled UAC
///    or if spawn/connect/handshake fails.
///
/// # Important Notes
///
/// - This function MUST be called off the GUI thread on a real OS
///   thread (e.g., via `promise::spawn::spawn_into_new_thread`, NOT
///   `promise::spawn::spawn`) because:
///   - `ShellExecuteExW("runas")` blocks until the user responds to UAC.
///   - `RendezvousListener::accept` blocks on the calling thread while
///     polling for the connection.
/// - The elevated child runs as `onlyterm-mux-server.exe` with `--single-pane`
///   mode, connecting back to the GUI via the rendezvous channel (WebSocket on
///   loopback). Its pane is rendered inside the existing non-elevated GUI window
///   via the mux protocol once connected, so the child itself is launched with
///   `SW_HIDE` to avoid a confusing console-window flash.
/// - This function is currently unused and will be wired in by a follow-up task
///   that integrates it into the "New Tab Options" dialog's admin path.
///
/// # Parameters
///
/// * `shell_args` - The shell command and arguments to run in the elevated tab
///   (e.g., `["cmd.exe", "/k", "echo elevated"]`).
/// * `cwd` - Optional working directory for the spawned program.
/// * `priority` - Process priority class for the elevated child.
/// * `connect_timeout` - Maximum time to wait for the elevated child to connect
///   after successful spawn (applies to the WebSocket handshake wait, not the UAC
///   prompt wait itself).
///
/// # Returns
///
/// * `ElevatedSinglePaneResult::Success(stream)` - Elevated child spawned,
///   connected, and authenticated; `stream` carries the mux protocol.
/// * `ElevatedSinglePaneResult::UserCancelled` - User cancelled the UAC prompt.
/// * `ElevatedSinglePaneResult::Failed(message)` - Spawn, connect, or handshake
///   failed with an error message.
#[allow(dead_code)] // Wired in by a follow-up task
pub fn spawn_elevated_single_pane(
    shell_args: &[String],
    cwd: Option<&std::path::Path>,
    priority: ProcessPriority,
    connect_timeout: std::time::Duration,
) -> ElevatedSinglePaneResult {
    use winapi::shared::winerror::ERROR_CANCELLED;
    use winapi::um::errhandlingapi::GetLastError;
    use winapi::um::handleapi::CloseHandle;
    use winapi::um::shellapi::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS};
    use winapi::um::synchapi::WaitForSingleObject;
    use winapi::um::winbase::WAIT_OBJECT_0;
    use winapi::um::winuser::SW_HIDE;

    // Step 1: Bind the rendezvous listener FIRST, before spawning anything.
    // See `RendezvousListener::bind`'s doc comment for why this ordering matters.
    let listener = match wezterm_elevated_transport::RendezvousListener::bind() {
        Ok(listener) => listener,
        Err(err) => {
            log::error!(
                "spawn_elevated_single_pane: failed to bind rendezvous listener: {:#}",
                err
            );
            return ElevatedSinglePaneResult::Failed(format!(
                "Failed to bind rendezvous listener: {:#}",
                err
            ));
        }
    };

    let port = listener.port();
    let token = listener.token().to_string();

    // Step 2: Resolve the mux-server executable path.
    // Use the same strategy as `crates/wezterm-gui/src/spawn.rs`.
    let mux_server_path = match std::env::current_exe() {
        Ok(path) => path.with_file_name("onlyterm-mux-server.exe"),
        Err(err) => {
            log::error!(
                "spawn_elevated_single_pane: failed to resolve current executable: {}",
                err
            );
            return ElevatedSinglePaneResult::Failed(format!(
                "Failed to resolve current executable: {}",
                err
            ));
        }
    };

    let mux_server_path_str = match mux_server_path.to_str() {
        Some(s) => s,
        None => {
            log::error!("spawn_elevated_single_pane: mux-server path is not valid UTF-8");
            return ElevatedSinglePaneResult::Failed(
                "Mux-server path is not valid UTF-8".to_string(),
            );
        }
    };

    // Step 3: Construct the command line parameters.
    let parameters = construct_single_pane_command_line(port, &token, cwd, priority, shell_args);

    // Convert to wide strings
    let mux_server_wide = wide_string(mux_server_path_str);
    let operation = wide_string("runas");
    let params_wide = wide_string(&parameters);

    // Step 4: Prepare SHELLEXECUTEINFOW with SEE_MASK_NOCLOSEPROCESS (to get hProcess)
    // and SW_HIDE (console subsystem with no UI should not flash a window).
    // SAFETY: SHELLEXECUTEINFOW is a plain C struct of integers/pointers;
    // an all-zero bit pattern is a valid value for every field, and every
    // field this code actually reads (cbSize, lpVerb, lpFile, lpParameters,
    // nShow, fMask) is explicitly set below before the struct is passed to
    // ShellExecuteExW.
    let mut sei = unsafe { std::mem::zeroed::<winapi::um::shellapi::SHELLEXECUTEINFOW>() };
    sei.cbSize = std::mem::size_of::<winapi::um::shellapi::SHELLEXECUTEINFOW>() as u32;
    sei.fMask = SEE_MASK_NOCLOSEPROCESS;
    sei.lpVerb = operation.as_ptr();
    sei.lpFile = mux_server_wide.as_ptr();
    sei.lpParameters = params_wide.as_ptr();
    sei.nShow = SW_HIDE;

    // Call ShellExecuteExW
    // SAFETY: All pointers are valid NUL-terminated wide strings that
    // outlive this synchronous call. cbSize is correctly set to the
    // struct size. Other fields are zero-initialized or set as above.
    let result = unsafe { ShellExecuteExW(&mut sei) };

    if result == 0 {
        // Failed - check for ERROR_CANCELLED (user cancelled UAC prompt)
        // SAFETY: GetLastError() takes no arguments and has no preconditions;
        // it just reads the calling thread's last-error TLS slot.
        let error_code = unsafe { GetLastError() };
        if error_code == ERROR_CANCELLED {
            return ElevatedSinglePaneResult::UserCancelled;
        } else {
            let error_msg = format!("ShellExecuteExW failed: error {}", error_code);
            log::error!("spawn_elevated_single_pane: {}", error_msg);
            return ElevatedSinglePaneResult::Failed(error_msg);
        }
    }

    // Success: we now own the process handle (must CloseHandle it on every return path)
    let h_process = sei.hProcess;

    // Build the child_exited closure for RendezvousListener::accept.
    // This uses WaitForSingleObject with timeout=0 to check liveness without blocking.
    let child_exited = || {
        // SAFETY: h_process is a valid process handle returned by ShellExecuteExW.
        // WaitForSingleObject with timeout 0 returns immediately: WAIT_OBJECT_0 if
        // the process has exited, WAIT_TIMEOUT if still running.
        let wait_result = unsafe { WaitForSingleObject(h_process, 0) };
        wait_result == WAIT_OBJECT_0
    };

    // Step 7: Wait for the elevated child to connect via WebSocket rendezvous.
    let deadline = Instant::now() + connect_timeout;
    let stream = match listener.accept(deadline, child_exited) {
        Ok(stream) => stream,
        Err(err) => {
            log::error!(
                "spawn_elevated_single_pane: failed to accept rendezvous connection: {:#}",
                err
            );
            let error_msg = format!("{:#}", err);
            // SAFETY: h_process is valid and we own it.
            unsafe { CloseHandle(h_process) };
            return ElevatedSinglePaneResult::Failed(error_msg);
        }
    };

    // SAFETY: h_process is valid and we own it.
    unsafe { CloseHandle(h_process) };

    ElevatedSinglePaneResult::Success(stream)
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

    #[test]
    fn test_construct_single_pane_command_line_minimal() {
        let result = construct_single_pane_command_line(
            12345,
            "test-token-abc123",
            None,
            ProcessPriority::Normal,
            &[],
        );
        assert_eq!(
            result,
            "--single-pane --connect-ws-port 12345 --token test-token-abc123"
        );
    }

    #[test]
    fn test_construct_single_pane_command_line_with_cwd() {
        let result = construct_single_pane_command_line(
            12345,
            "test-token-abc123",
            Some(std::path::Path::new("C:\\Users\\Test")),
            ProcessPriority::Normal,
            &[],
        );
        assert_eq!(
            result,
            "--single-pane --connect-ws-port 12345 --token test-token-abc123 --cwd \"C:\\Users\\Test\""
        );
    }

    #[test]
    #[cfg(windows)]
    fn test_construct_single_pane_command_line_with_priority() {
        let result = construct_single_pane_command_line(
            12345,
            "test-token-abc123",
            None,
            ProcessPriority::High,
            &[],
        );
        assert_eq!(
            result,
            "--single-pane --connect-ws-port 12345 --token test-token-abc123 --priority High"
        );
    }

    #[test]
    fn test_construct_single_pane_command_line_with_shell_args() {
        let result = construct_single_pane_command_line(
            12345,
            "test-token-abc123",
            None,
            ProcessPriority::Normal,
            &[
                "cmd.exe".to_string(),
                "/k".to_string(),
                "echo hello".to_string(),
            ],
        );
        assert_eq!(
            result,
            "--single-pane --connect-ws-port 12345 --token test-token-abc123 -- cmd.exe /k \"echo hello\""
        );
    }

    #[test]
    fn test_construct_single_pane_command_line_full() {
        let result = construct_single_pane_command_line(
            12345,
            "test-token-abc123",
            Some(std::path::Path::new("C:\\Program Files\\App")),
            ProcessPriority::High,
            &["powershell.exe".to_string(), "-NoProfile".to_string()],
        );
        #[cfg(windows)]
        assert_eq!(
            result,
            "--single-pane --connect-ws-port 12345 --token test-token-abc123 --cwd \"C:\\Program Files\\App\" --priority High -- powershell.exe -NoProfile"
        );
        #[cfg(not(windows))]
        assert_eq!(
            result,
            "--single-pane --connect-ws-port 12345 --token test-token-abc123 --cwd \"C:\\Program Files\\App\" -- powershell.exe -NoProfile"
        );
    }

    #[test]
    fn test_quote_arg_simple() {
        assert_eq!(quote_arg("simple"), "simple");
    }

    #[test]
    fn test_quote_arg_with_spaces() {
        assert_eq!(quote_arg("path with spaces"), "\"path with spaces\"");
    }

    #[test]
    fn test_quote_arg_with_tabs() {
        assert_eq!(quote_arg("path\twith\ttabs"), "\"path\twith\ttabs\"");
    }

    #[test]
    fn test_quote_arg_with_quotes() {
        assert_eq!(
            quote_arg("path\"with\"quotes"),
            "\"path\\\"with\\\"quotes\""
        );
    }

    #[test]
    fn test_quote_arg_empty() {
        assert_eq!(quote_arg(""), "\"\"");
    }
}
