use super::*;

#[test]
fn test_env() {
    let mut cmd = CommandBuilder::new("dummy");
    let package_authors = cmd.get_env("CARGO_PKG_AUTHORS");
    println!("package_authors: {:?}", package_authors);
    assert!(package_authors == Some(OsStr::new("Wez Furlong")));

    cmd.env("foo key", "foo value");
    cmd.env("bar key", "bar value");

    let iterated_envs = cmd.iter_extra_env_as_str().collect::<Vec<_>>();
    println!("iterated_envs: {:?}", iterated_envs);
    assert!(iterated_envs == vec![("bar key", "bar value"), ("foo key", "foo value")]);

    {
        let mut cmd = cmd.clone();
        cmd.env_remove("foo key");

        let iterated_envs = cmd.iter_extra_env_as_str().collect::<Vec<_>>();
        println!("iterated_envs: {:?}", iterated_envs);
        assert!(iterated_envs == vec![("bar key", "bar value")]);
    }

    {
        let mut cmd = cmd.clone();
        cmd.env_remove("bar key");

        let iterated_envs = cmd.iter_extra_env_as_str().collect::<Vec<_>>();
        println!("iterated_envs: {:?}", iterated_envs);
        assert!(iterated_envs == vec![("foo key", "foo value")]);
    }

    {
        let mut cmd = cmd.clone();
        cmd.env_clear();

        let iterated_envs = cmd.iter_extra_env_as_str().collect::<Vec<_>>();
        println!("iterated_envs: {:?}", iterated_envs);
        assert!(iterated_envs.is_empty());
    }
}

#[cfg(windows)]
#[test]
fn test_env_case_insensitive_override() {
    let mut cmd = CommandBuilder::new("dummy");
    cmd.env("Cargo_Pkg_Authors", "Not OnlyTerm");
    assert!(cmd.get_env("cargo_pkg_authors") == Some(OsStr::new("Not OnlyTerm")));

    cmd.env_remove("cARGO_pKG_aUTHORS");
    assert!(cmd.get_env("CARGO_PKG_AUTHORS").is_none());
}

#[cfg(windows)]
#[test]
fn environment_with_empty_name_still_spawns_with_regular_variables() {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use winapi::shared::minwindef::FALSE;
    use winapi::um::processthreadsapi::{
        CreateProcessW, GetExitCodeProcess, TerminateProcess, PROCESS_INFORMATION, STARTUPINFOW,
    };
    use winapi::um::synchapi::WaitForSingleObject;
    use winapi::um::winbase::{CREATE_NO_WINDOW, CREATE_UNICODE_ENVIRONMENT};

    let mut command = CommandBuilder::new("cmd.exe");
    command.args(["/d", "/c", "exit", "%ONLYTERM_ENV_BLOCK_EXIT%"]);
    command.env("", "should be dropped");
    command.env("ONLYTERM_ENV_BLOCK_EXIT", "23");
    let (mut exe, mut command_line) = command.cmdline().unwrap();
    let mut environment = command.environment_block();
    // SAFETY: These WinAPI structs contain only zero-valid fields.
    let mut startup: STARTUPINFOW = unsafe { std::mem::zeroed() };
    // SAFETY: PROCESS_INFORMATION is a zero-valid, exclusively owned output.
    let mut info: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
    startup.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
    // SAFETY: Command/environment buffers are mutable, NUL-terminated and live
    // for the synchronous call; CreateProcessW initializes `info` on success.
    let created = unsafe {
        CreateProcessW(
            exe.as_mut_ptr(),
            command_line.as_mut_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            FALSE,
            CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT,
            environment.as_mut_ptr().cast(),
            std::ptr::null(),
            &mut startup,
            &mut info,
        )
    };
    assert_ne!(
        created,
        0,
        "CreateProcessW failed: {}",
        std::io::Error::last_os_error()
    );
    eprintln!("Owned environment test PID {}", info.dwProcessId);
    // SAFETY: Successful CreateProcessW transferred both returned handles to us.
    let process = unsafe { OwnedHandle::from_raw_handle(info.hProcess as _) };
    // SAFETY: The separately owned thread handle is no longer needed.
    drop(unsafe { OwnedHandle::from_raw_handle(info.hThread as _) });
    // SAFETY: This is the original handle for the exact PID we launched above.
    let waited = unsafe { WaitForSingleObject(process.as_raw_handle() as _, 5000) };
    if waited != 0 {
        // SAFETY: Only this test's captured child is terminated, through its handle.
        unsafe { TerminateProcess(process.as_raw_handle() as _, 1) };
        panic!(
            "Owned environment test PID {} did not exit",
            info.dwProcessId
        );
    }
    let mut status = 0;
    // SAFETY: The process handle and writable DWORD output remain valid.
    let queried = unsafe { GetExitCodeProcess(process.as_raw_handle() as _, &mut status) };
    assert_ne!(queried, 0);
    assert_eq!(
        status, 23,
        "The child must receive its configured environment value"
    );
}

#[cfg(windows)]
#[test]
fn test_search_path_empty_pathext_entries() {
    // Regression test for wezterm/wezterm#6499: a PATHEXT containing
    // empty segments (e.g. from a stray `;;` separator) used to panic
    // in `search_path` due to indexing `&ext[1..]` on an empty string.
    let mut cmd = CommandBuilder::new("dummy");
    cmd.env("PATH", std::env::temp_dir());
    cmd.env("PATHEXT", ";;.EXE;.CMD;;");

    // Must not panic regardless of PATHEXT contents; the returned value
    // is allowed to be the bare exe name when nothing is found on PATH.
    let resolved = cmd.search_path(OsStr::new("cmd"));
    assert!(!resolved.is_empty());
}
