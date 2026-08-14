//! The set of shells a tab can be launched with, shared by the New Tab
//! Options dialog and by `--start-conf` layouts so that both agree on what
//! argv each name means.

use std::path::PathBuf;
use wezterm_dynamic::{FromDynamic, FromDynamicOptions, ToDynamic, Value};

/// Folds a config token to its comparison form: case-insensitive, and
/// ignoring the separators people reasonably reach for. `above_normal`,
/// `AboveNormal`, `above-normal` and `Above Normal` all collapse to the same
/// thing, so a config is never rejected over a spelling that was obviously
/// meant.
pub(crate) fn normalize_token(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, '_' | '-' | ' '))
        .flat_map(|c| c.to_lowercase())
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ToDynamic)]
pub enum Shell {
    Cmd,
    Bash,
    Powershell,
    Wsl,
}

impl Shell {
    pub const ALL: [Shell; 4] = [Shell::Cmd, Shell::Bash, Shell::Powershell, Shell::Wsl];

    /// The name shown in the New Tab Options dialog, which is also exactly
    /// the token accepted in a `--start-conf` layout.
    pub fn name(&self) -> &'static str {
        match self {
            Shell::Cmd => "cmd",
            Shell::Bash => "bash",
            Shell::Powershell => "powershell",
            Shell::Wsl => "wsl",
        }
    }

    /// Argv for launching this shell. `Cmd`/`Powershell`/`Wsl` are resolved
    /// via PATH -- the same convention `windows_default_prog()` and the old
    /// (now removed) WSL launcher entries used, since this codebase has no
    /// separate shell-path resolution helper to defer to instead. `Bash`
    /// instead resolves to an explicit path via `find_git_bash()`: see that
    /// function's doc comment for why a bare `"bash.exe"` is unsafe on a
    /// machine with WSL installed.
    pub fn argv(&self) -> Vec<String> {
        match self {
            Shell::Cmd => vec!["cmd.exe".to_string()],
            Shell::Bash => vec![find_git_bash()],
            Shell::Powershell => {
                // Spelled out here as well as applied centrally in
                // `Config::apply_cmd_defaults`, because the elevated variant
                // of this argv is assembled into a command line for a
                // *separate* process (see `elevate.rs`) that never passes
                // through that config hook. `ensure_powershell_utf8` is
                // idempotent, so a path that picks it up twice is harmless.
                let mut argv = vec!["powershell.exe".to_string()];
                argv.extend(crate::powershell::powershell_utf8_args());
                argv
            }
            Shell::Wsl => vec!["wsl.exe".to_string()],
        }
    }
}

impl FromDynamic for Shell {
    fn from_dynamic(
        value: &Value,
        options: FromDynamicOptions,
    ) -> Result<Self, wezterm_dynamic::Error> {
        let s = String::from_dynamic(value, options)?;
        match normalize_token(&s).as_str() {
            "cmd" => Ok(Self::Cmd),
            "bash" => Ok(Self::Bash),
            "powershell" => Ok(Self::Powershell),
            "wsl" => Ok(Self::Wsl),
            _ => Err(wezterm_dynamic::Error::Message(format!(
                "`{s}` is not a valid shell; use one of {}",
                Shell::ALL
                    .iter()
                    .map(|s| s.name())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))),
        }
    }
}

/// Locates Git for Windows' `bash.exe` by checking well-known install
/// locations directly, rather than resolving a bare `"bash.exe"` via PATH:
/// Windows process-launch handle resolution consults the system directory
/// (`%SystemRoot%\System32`) *before* PATH-listed directories, and on a
/// machine with WSL installed, `System32\bash.exe` is WSL's own legacy
/// launcher shim -- so a plain `"bash.exe"` argv silently launched WSL
/// instead of Git Bash, even though `where.exe` (which does not replicate
/// that same-directory-first search order) reported Git Bash's `bash.exe`
/// first. Confirmed live: the dialog's "bash" and "wsl" options were
/// producing identical `/mnt/c/...` WSL prompts.
///
/// Falls back to the bare `"bash.exe"` (letting ordinary process-launch
/// resolution, System32-shim gotcha included, apply) only if none of the
/// well-known install locations exist, so a Git-for-Windows install in a
/// nonstandard location still gets *something* rather than a hard error.
pub fn find_git_bash() -> String {
    let candidates = [
        std::env::var_os("ProgramFiles").map(|p| PathBuf::from(p).join("Git\\bin\\bash.exe")),
        std::env::var_os("ProgramFiles(x86)").map(|p| PathBuf::from(p).join("Git\\bin\\bash.exe")),
        std::env::var_os("LOCALAPPDATA")
            .map(|p| PathBuf::from(p).join("Programs\\Git\\bin\\bash.exe")),
    ];
    for candidate in candidates.iter().flatten() {
        if candidate.is_file() {
            if let Some(s) = candidate.to_str() {
                return s.to_string();
            }
        }
    }
    "bash.exe".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Result<Shell, wezterm_dynamic::Error> {
        Shell::from_dynamic(&Value::String(s.into()), FromDynamicOptions::default())
    }

    #[test]
    fn every_shells_own_name_parses_back_to_it() {
        // The dialog's labels and the config tokens are the same strings, so
        // this also pins them together: renaming one without the other would
        // fail here.
        for shell in Shell::ALL {
            assert_eq!(parse(shell.name()).unwrap(), shell);
        }
    }

    #[test]
    fn spelling_is_forgiving() {
        for spelling in ["powershell", "PowerShell", "POWERSHELL", "power_shell"] {
            assert_eq!(parse(spelling).unwrap(), Shell::Powershell, "{}", spelling);
        }
    }

    #[test]
    fn an_unknown_shell_is_an_error_naming_the_valid_ones() {
        let err = parse("fish").unwrap_err().to_string();
        assert!(err.contains("fish"), "{}", err);
        for shell in Shell::ALL {
            assert!(
                err.contains(shell.name()),
                "{} should list {:?}",
                err,
                shell
            );
        }
    }

    #[test]
    fn powershell_argv_carries_the_utf8_preamble() {
        let argv = Shell::Powershell.argv();
        assert_eq!(argv[0], "powershell.exe");
        assert!(argv.contains(&"-NoExit".to_string()));
        assert!(argv.contains(&crate::powershell::POWERSHELL_UTF8_PREAMBLE.to_string()));
    }

    #[test]
    fn the_other_shells_take_no_extra_arguments() {
        assert_eq!(Shell::Cmd.argv(), vec!["cmd.exe".to_string()]);
        assert_eq!(Shell::Wsl.argv(), vec!["wsl.exe".to_string()]);
        // Bash resolves to a path that varies by machine, so only its shape
        // is assertable here.
        assert_eq!(Shell::Bash.argv().len(), 1);
    }
}
