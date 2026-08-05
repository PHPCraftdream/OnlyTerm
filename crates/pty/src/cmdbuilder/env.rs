#[cfg(feature = "serde_support")]
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::ffi::OsString;
#[cfg(unix)]
use std::path::Path;

/// Used to deal with Windows having case-insensitive environment variables.
#[derive(Clone, Debug, PartialEq, PartialOrd)]
#[cfg_attr(feature = "serde_support", derive(Serialize, Deserialize))]
pub(super) struct EnvEntry {
    /// Whether or not this environment variable came from the base environment,
    /// as opposed to having been explicitly set by the caller.
    pub(super) is_from_base_env: bool,

    /// For case-insensitive platforms, the environment variable key in its preferred casing.
    pub(super) preferred_key: OsString,

    /// The environment variable value.
    pub(super) value: OsString,
}

impl EnvEntry {
    pub(super) fn map_key(k: OsString) -> OsString {
        if cfg!(windows) {
            // Best-effort lowercase transformation of an os string
            match k.to_str() {
                Some(s) => s.to_lowercase().into(),
                None => k,
            }
        } else {
            k
        }
    }
}

#[cfg(unix)]
pub(super) fn get_shell() -> String {
    use nix::unistd::{access, AccessFlags};
    use std::ffi::CStr;
    use std::str;

    // SAFETY: `getuid` returns the current process's uid; `getpwuid` is a
    // standard libc function that returns a pointer to a static passwd entry
    // (or NULL if not found). The returned pointer is valid until the next
    // libc call that modifies the passwd database.
    let ent = unsafe { libc::getpwuid(libc::getuid()) };
    if !ent.is_null() {
        // SAFETY: `ent` is non-null (checked above) and was returned by
        // getpwuid, so `(*ent).pw_shell` points to a valid NUL-terminated string.
        let shell = unsafe { CStr::from_ptr((*ent).pw_shell) };
        match shell.to_str().map(str::to_owned) {
            Err(err) => {
                log::warn!(
                    "passwd database shell could not be \
                     represented as utf-8: {err:#}, \
                     falling back to /bin/sh"
                );
            }
            Ok(shell) => {
                if let Err(err) = access(Path::new(&shell), AccessFlags::X_OK) {
                    log::warn!(
                        "passwd database shell={shell:?} which is \
                         not executable ({err:#}), falling back to /bin/sh"
                    );
                } else {
                    return shell;
                }
            }
        }
    }
    "/bin/sh".into()
}

pub(super) fn get_base_env() -> BTreeMap<OsString, EnvEntry> {
    let mut env: BTreeMap<OsString, EnvEntry> = std::env::vars_os()
        .map(|(key, value)| {
            (
                EnvEntry::map_key(key.clone()),
                EnvEntry {
                    is_from_base_env: true,
                    preferred_key: key,
                    value,
                },
            )
        })
        .collect();

    #[cfg(unix)]
    {
        let key = EnvEntry::map_key("SHELL".into());
        // Only set the value of SHELL if it isn't already set
        if !env.contains_key(&key) {
            env.insert(
                EnvEntry::map_key("SHELL".into()),
                EnvEntry {
                    is_from_base_env: true,
                    preferred_key: "SHELL".into(),
                    value: get_shell().into(),
                },
            );
        }
    }

    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStringExt;
        use winapi::um::processenv::ExpandEnvironmentStringsW;
        use winreg::enums::{RegType, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
        use winreg::types::FromRegValue;
        use winreg::{RegKey, RegValue};

        fn reg_value_to_string(value: &RegValue) -> anyhow::Result<OsString> {
            match value.vtype {
                RegType::REG_EXPAND_SZ => {
                    // Reinterpret the raw bytes as native-endian u16 (UTF-16LE
                    // on Windows). Using chunks_exact avoids the alignment UB
                    // that a raw pointer cast (*const u8 as *const u16) would
                    // introduce when the Vec<u8> backing allocation is not
                    // guaranteed to be 2-byte aligned.
                    let src: Vec<u16> = value
                        .bytes
                        .chunks_exact(2)
                        .map(|c| u16::from_ne_bytes([c[0], c[1]]))
                        .collect();
                    // SAFETY: ExpandEnvironmentStringsW is a standard win32 FFI
                    // call. Passing a NULL output buffer with 0 capacity is the
                    // documented way to query the required buffer size.
                    let size =
                        unsafe { ExpandEnvironmentStringsW(src.as_ptr(), std::ptr::null_mut(), 0) };
                    let mut buf = vec![0u16; size as usize + 1];
                    // SAFETY: `buf` is a valid writable buffer of `buf.len()`
                    // u16 elements; `src` is a valid wide-string input.
                    unsafe {
                        ExpandEnvironmentStringsW(src.as_ptr(), buf.as_mut_ptr(), buf.len() as u32)
                    };

                    let mut buf = buf.as_slice();
                    while let Some(0) = buf.last() {
                        buf = &buf[0..buf.len() - 1];
                    }
                    Ok(OsString::from_wide(buf))
                }
                _ => Ok(OsString::from_reg_value(value)?),
            }
        }

        if let Ok(sys_env) = RegKey::predef(HKEY_LOCAL_MACHINE)
            .open_subkey("System\\CurrentControlSet\\Control\\Session Manager\\Environment")
        {
            for (name, value) in sys_env.enum_values().flatten() {
                if name.eq_ignore_ascii_case("username") {
                    continue;
                }
                if let Ok(value) = reg_value_to_string(&value) {
                    log::trace!("adding SYS env: {:?} {:?}", name, value);
                    env.insert(
                        EnvEntry::map_key(name.clone().into()),
                        EnvEntry {
                            is_from_base_env: true,
                            preferred_key: name.into(),
                            value,
                        },
                    );
                }
            }
        }

        if let Ok(sys_env) = RegKey::predef(HKEY_CURRENT_USER).open_subkey("Environment") {
            for (name, value) in sys_env.enum_values().flatten() {
                if let Ok(value) = reg_value_to_string(&value) {
                    // Merge the system and user paths together
                    let value = if name.eq_ignore_ascii_case("path") {
                        match env.get(&EnvEntry::map_key(name.clone().into())) {
                            Some(entry) => {
                                let mut result = OsString::new();
                                result.push(&entry.value);
                                result.push(";");
                                result.push(&value);
                                result
                            }
                            None => value,
                        }
                    } else {
                        value
                    };

                    log::trace!("adding USER env: {:?} {:?}", name, value);
                    env.insert(
                        EnvEntry::map_key(name.clone().into()),
                        EnvEntry {
                            is_from_base_env: true,
                            preferred_key: name.into(),
                            value,
                        },
                    );
                }
            }
        }
    }

    env
}
