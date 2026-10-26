//! Picks the editor "OnlyTerm folder settings" opens run files in.

use std::path::PathBuf;

/// Notepad++ when it is installed, otherwise Notepad.
pub fn run_file_editor() -> PathBuf {
    notepad_plus_plus().unwrap_or_else(|| PathBuf::from("notepad.exe"))
}

/// Notepad++'s executable: from its App Paths or install-dir registration
/// (per-user first, both registry views), else its default install folders.
#[cfg(windows)]
fn notepad_plus_plus() -> Option<PathBuf> {
    use windows::Win32::System::Registry::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};

    const APP_PATH: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\notepad++.exe";
    const INSTALL_DIR: &str = r"SOFTWARE\Notepad++";

    let registered = [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE]
        .iter()
        .flat_map(|&root| {
            [
                registry_string(root, APP_PATH).map(PathBuf::from),
                registry_string(root, INSTALL_DIR)
                    .map(|dir| PathBuf::from(dir).join("notepad++.exe")),
            ]
        })
        .flatten();
    let default_dirs = ["ProgramW6432", "ProgramFiles", "ProgramFiles(x86)"]
        .iter()
        .filter_map(std::env::var_os)
        .map(|dir| PathBuf::from(dir).join(r"Notepad++\notepad++.exe"));

    registered.chain(default_dirs).find(|path| path.is_file())
}

#[cfg(not(windows))]
fn notepad_plus_plus() -> Option<PathBuf> {
    None
}

/// The default (unnamed) string value of `root\subkey`, read from the 64-bit
/// view, then the 32-bit one.
#[cfg(windows)]
fn registry_string(root: windows::Win32::System::Registry::HKEY, subkey: &str) -> Option<String> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{
        RegGetValueW, RRF_RT_REG_SZ, RRF_SUBKEY_WOW6432KEY, RRF_SUBKEY_WOW6464KEY,
    };

    let subkey: Vec<u16> = subkey.encode_utf16().chain(Some(0)).collect();
    for &view in [RRF_SUBKEY_WOW6464KEY, RRF_SUBKEY_WOW6432KEY].iter() {
        let mut buf = vec![0u16; 1024];
        let mut len = (buf.len() * std::mem::size_of::<u16>()) as u32;
        // SAFETY: `subkey` is NUL-terminated UTF-16 that outlives the call; a
        // null value name selects the default value. `buf` is writable for
        // `len` bytes, and `len` is a valid u32 the call updates in place.
        // With RRF_RT_REG_SZ the result is NUL-terminated within `len` bytes.
        let status = unsafe {
            RegGetValueW(
                root,
                PCWSTR(subkey.as_ptr()),
                PCWSTR::null(),
                RRF_RT_REG_SZ | view,
                None,
                Some(buf.as_mut_ptr().cast()),
                Some(&mut len),
            )
        };
        if status == ERROR_SUCCESS {
            let chars = (len as usize / std::mem::size_of::<u16>()).min(buf.len());
            let value = String::from_utf16_lossy(&buf[..chars]);
            let value = value.trim_end_matches('\0').trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}
