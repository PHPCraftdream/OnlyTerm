#![warn(clippy::undocumented_unsafe_blocks)]
// Portions of this file are derived from code that is
// Copyright © 2015 Sebastian Thiel
// <https://github.com/Byron/open-rs>

/// Open a local text file for editing.
///
/// Distinct from `open_url` because our config files use a `.ktav`
/// extension that nothing registers a handler for: handing that straight
/// to the shell's "open" verb gets you an "how do you want to open this
/// file?" dialog.
///
/// Fire-and-forget, like the rest of this module -- the launch happens on
/// a background thread and there is no result to wait for.
pub fn open_text_file(path: &std::path::Path) {
    let path = path.to_string_lossy().to_string();

    // Deliberately `CreateProcess` (via `Command`) rather than
    // `ShellExecuteW`: the shell API kept returning `SE_ERR_FNF` (2)
    // here regardless of whether notepad was named bare or by absolute
    // path, and since it launches through the shell there is no error
    // to inspect beyond that opaque code. Spawning the editor directly
    // is both simpler and diagnosable -- `spawn()` returns a real
    // `io::Error` naming what failed.
    //
    // notepad.exe ships with every Windows install, so unlike a
    // file-association lookup for our `.ktav` extension this cannot
    // come up empty.
    std::thread::spawn(move || {
        if let Err(err) = std::process::Command::new("notepad.exe").arg(&path).spawn() {
            log::error!("failed to launch notepad.exe for {path:?}: {err:#}");
        }
    });
}

fn shell_execute(url: String, with: Option<String>) {
    use std::os::windows::ffi::OsStrExt;
    use winapi::um::shellapi::ShellExecuteW;
    /// Convert a rust string to a windows wide string
    fn wide_string(s: &str) -> Vec<u16> {
        std::ffi::OsStr::new(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }
    std::thread::spawn(move || {
        let operation = wide_string("open");

        let original = url.clone();
        let url = wide_string(&url);
        let with = with.map(|s| wide_string(&s));

        let (app, path) = match with {
            Some(app) => (app.as_ptr(), url.as_ptr()),
            None => (url.as_ptr(), std::ptr::null()),
        };

        // SAFETY: ShellExecuteW is the documented Win32 shell API for opening a
        // URL/document. The string arguments (`lpOperation`, `lpFile`, and when
        // present `lpParameters`) are NUL-terminated UTF-16 buffers produced by
        // `wide_string`, which remain valid for the duration of this synchronous
        // call. `hwnd`, `lpDirectory` and (in the plain open-URL case) `lpParameters`
        // are NULL, which MSDN explicitly permits, and `SW_SHOW` is a valid
        // window-show constant.
        let result = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                operation.as_ptr(),
                app,
                path,
                std::ptr::null(),
                winapi::um::winuser::SW_SHOW,
            )
        };

        // ShellExecuteW returns a fake HINSTANCE that is only an error code
        // when it is <= 32. Nothing can be done about a failure from this
        // detached thread, but saying so beats the previous behaviour of
        // discarding it: a caller whose launch silently did nothing had no
        // way at all to find out why.
        if result as usize <= 32 {
            log::error!(
                "ShellExecuteW failed with code {} for {:?}",
                result as usize,
                original
            );
        }
    });
}

pub fn open_url(url: &str) {
    shell_execute(url.to_string(), None);
}

pub fn open_with(url: &str, app: &str) {
    shell_execute(url.to_string(), Some(app.to_string()));
}
