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

    /// Argv for launching this shell: the executable this shell resolves to
    /// on *this* machine (see `resolve`), followed by whatever extra
    /// arguments that shell needs. When resolution finds nothing, argv[0]
    /// falls back to the bare image name and ordinary process-launch
    /// resolution applies -- so a shell installed somewhere unexpected still
    /// gets a chance to start rather than a hard error.
    pub fn argv(&self) -> Vec<String> {
        let program = self
            .resolve()
            .and_then(|path| path.to_str().map(|s| s.to_string()))
            .unwrap_or_else(|| self.image_name().to_string());
        let mut argv = vec![program];
        argv.extend(self.extra_args());
        argv
    }

    /// The bare executable name, used only as the last-resort argv[0].
    fn image_name(&self) -> &'static str {
        match self {
            Shell::Cmd => "cmd.exe",
            Shell::Bash => "bash.exe",
            Shell::Powershell => "powershell.exe",
            Shell::Wsl => "wsl.exe",
        }
    }

    /// Arguments that always follow the executable.
    fn extra_args(&self) -> Vec<String> {
        match self {
            // Spelled out here as well as applied centrally in
            // `Config::apply_cmd_defaults`, because the elevated variant of
            // this argv is assembled into a command line for a *separate*
            // process (see `elevate.rs`) that never passes through that
            // config hook. `ensure_powershell_utf8` is idempotent, so a path
            // that picks it up twice is harmless.
            Shell::Powershell => crate::powershell::powershell_utf8_args(),
            Shell::Cmd | Shell::Bash | Shell::Wsl => vec![],
        }
    }

    /// The executable this shell launches on this machine, or `None` when
    /// the shell is not installed.
    ///
    /// Every candidate below is an explicit well-known location; none of
    /// them is a bare image name left for ordinary process-launch
    /// resolution to find. That is deliberate, and it is what keeps two
    /// entries in the New Tab Options dialog from being the same program
    /// wearing two different names: Windows resolves an unqualified image
    /// name against `%SystemRoot%\System32` *before* consulting PATH, and
    /// System32 holds a `bash.exe` that is WSL's own launcher shim. See
    /// `find_git_bash` for the live failure that established this rule.
    #[cfg(windows)]
    pub fn resolve(&self) -> Option<PathBuf> {
        match self {
            Shell::Cmd => system32("cmd.exe"),
            Shell::Bash => find_git_bash_path(),
            Shell::Powershell => system32("WindowsPowerShell\\v1.0\\powershell.exe"),
            // `wsl.exe` is present in System32 on machines where no
            // distribution has ever been installed, and running it there
            // only prints an installation notice into a tab that is then
            // useless -- so its presence alone does not make WSL an
            // offerable shell.
            Shell::Wsl => system32("wsl.exe").filter(|_| wsl_has_a_distribution()),
        }
    }

    /// The non-Windows answer. This fork targets Windows, so only `Bash` can
    /// resolve to anything here; the rest are honestly reported absent
    /// rather than offered and then failing to spawn.
    #[cfg(not(windows))]
    pub fn resolve(&self) -> Option<PathBuf> {
        match self {
            Shell::Bash => Some(PathBuf::from("/bin/bash")).filter(|p| p.is_file()),
            Shell::Cmd | Shell::Powershell | Shell::Wsl => None,
        }
    }
}

/// A shell that was found on this machine, paired with the executable it
/// resolved to. The path is carried alongside so callers that want to
/// compare, log or display it don't have to resolve a second time and risk
/// getting a different answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvailableShell {
    pub shell: Shell,
    pub path: PathBuf,
}

/// The shells actually installed on this machine, in `Shell::ALL` order.
///
/// Two things are filtered out. Shells that resolve to nothing are absent
/// entirely, so the dialog never offers a tab that cannot start. And any
/// two names that resolve to the *same* executable collapse to whichever
/// comes first in `Shell::ALL` -- because a user picking between two
/// identically-behaving entries is being asked a question with no answer.
/// That second rule is mostly insurance: `resolve` is written so the
/// collision cannot arise, and this makes it impossible rather than merely
/// unlikely.
///
/// Never empty: if detection finds nothing at all, `Shell::Cmd` is offered
/// on the strength of the bare image name. A Windows install without a
/// command processor is broken in ways this dialog cannot help with, and an
/// empty result would leave the dialog with a Run button and nothing to run.
pub fn available_shells() -> Vec<AvailableShell> {
    let mut found: Vec<AvailableShell> = Vec::with_capacity(Shell::ALL.len());
    for shell in Shell::ALL {
        let Some(path) = shell.resolve() else {
            continue;
        };
        // Windows paths compare case-insensitively, and every candidate is
        // built from an environment variable plus a literal suffix, so the
        // strings are already normalized in shape -- folding case is enough
        // to spot a genuine collision without a filesystem round trip.
        let is_duplicate = found.iter().any(|other| {
            other
                .path
                .as_os_str()
                .eq_ignore_ascii_case(path.as_os_str())
        });
        if is_duplicate {
            log::debug!(
                "shell {} resolves to {}, already offered under another name; hiding it",
                shell.name(),
                path.display()
            );
            continue;
        }
        found.push(AvailableShell { shell, path });
    }

    if found.is_empty() {
        log::warn!("no shell could be detected; offering cmd on the bare image name");
        found.push(AvailableShell {
            shell: Shell::Cmd,
            path: PathBuf::from(Shell::Cmd.image_name()),
        });
    }
    found
}

/// `%SystemRoot%\System32\<relative>`, if it exists.
#[cfg(windows)]
fn system32(relative: &str) -> Option<PathBuf> {
    let root = std::env::var_os("SystemRoot")?;
    let path = PathBuf::from(root).join("System32").join(relative);
    if path.is_file() {
        Some(path)
    } else {
        None
    }
}

/// Whether WSL has at least one registered distribution.
///
/// Read from the registry rather than by running `wsl.exe --list`: this is
/// consulted while the dialog is being built, and a process launch there
/// would be paid on the render path. The key holds one GUID subkey per
/// registered distribution.
///
/// Docker Desktop registers its own distributions here too, so a `true`
/// answer means "wsl can start something", not "there is a distribution you
/// would want to live in". Telling those apart would mean guessing from
/// names, which is worse than offering an entry the user can simply not
/// pick.
#[cfg(windows)]
fn wsl_has_a_distribution() -> bool {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let lxss = match RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Lxss")
    {
        Ok(key) => key,
        Err(_) => return false,
    };
    lxss.enum_keys().flatten().any(|guid| {
        lxss.open_subkey(&guid)
            .and_then(|distro| distro.get_value::<String, _>("DistributionName"))
            .is_ok()
    })
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
/// Returns `None` when Git for Windows is not installed in any of them.
/// Callers that must produce *something* regardless use `find_git_bash`,
/// which substitutes the bare image name; callers deciding whether to offer
/// bash at all use this one, because that bare name is precisely the
/// System32 WSL shim described above -- offering it would put a second WSL
/// entry in the dialog under the name "bash".
///
/// `Git\bin\bash.exe` rather than `Git\usr\bin\bash.exe`: the former is a
/// small wrapper that establishes `MSYSTEM` and the MSYS PATH before handing
/// off to the latter, which is the actual bash binary. Launching the binary
/// directly yields a shell with a half-configured environment.
pub fn find_git_bash_path() -> Option<PathBuf> {
    let candidates = [
        std::env::var_os("ProgramFiles").map(|p| PathBuf::from(p).join("Git\\bin\\bash.exe")),
        std::env::var_os("ProgramFiles(x86)").map(|p| PathBuf::from(p).join("Git\\bin\\bash.exe")),
        std::env::var_os("LOCALAPPDATA")
            .map(|p| PathBuf::from(p).join("Programs\\Git\\bin\\bash.exe")),
    ];
    // A `for` loop rather than `candidates.into_iter()`: this crate is
    // edition 2018, where that method call still resolves to the slice
    // iterator and yields references.
    for candidate in candidates {
        match candidate {
            Some(path) if path.is_file() => return Some(path),
            _ => {}
        }
    }
    None
}

/// Falls back to the bare `"bash.exe"` (letting ordinary process-launch
/// resolution, System32-shim gotcha included, apply) only if none of the
/// well-known install locations exist, so a Git-for-Windows install in a
/// nonstandard location still gets *something* rather than a hard error.
pub fn find_git_bash() -> String {
    find_git_bash_path()
        .and_then(|path| path.to_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "bash.exe".to_string())
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

    /// argv[0] is an absolute path wherever the shell was actually found,
    /// and the bare image name otherwise, so only its tail is assertable
    /// without pinning the test to the machine it runs on.
    fn assert_program_is(argv: &[String], image_name: &str) {
        let program = argv[0].to_ascii_lowercase();
        assert!(
            program.ends_with(image_name),
            "{:?} should launch {}",
            argv,
            image_name
        );
    }

    #[test]
    fn powershell_argv_carries_the_utf8_preamble() {
        let argv = Shell::Powershell.argv();
        assert_program_is(&argv, "powershell.exe");
        assert!(argv.contains(&"-NoExit".to_string()));
        assert!(argv.contains(&crate::powershell::POWERSHELL_UTF8_PREAMBLE.to_string()));
    }

    #[test]
    fn the_other_shells_take_no_extra_arguments() {
        for (shell, image_name) in [
            (Shell::Cmd, "cmd.exe"),
            (Shell::Wsl, "wsl.exe"),
            (Shell::Bash, "bash.exe"),
        ] {
            let argv = shell.argv();
            assert_eq!(argv.len(), 1, "{:?} should take no extra args", shell);
            assert_program_is(&argv, image_name);
        }
    }

    /// The point of the whole detection pass: whatever is offered can start.
    #[test]
    fn every_offered_shell_resolves_to_a_file_that_exists() {
        for available in available_shells() {
            // The empty-detection fallback is the one entry allowed to be a
            // bare name, since by then there is nothing left to verify.
            if available.path.as_os_str() == available.shell.image_name() {
                continue;
            }
            assert!(
                available.path.is_file(),
                "{:?} was offered as {}, which is not a file",
                available.shell,
                available.path.display()
            );
        }
    }

    /// Two names for one executable is the "portal" case: `bash` falling
    /// back to the bare image name would resolve to System32's WSL shim and
    /// sit in the dialog next to `wsl`, both starting the same thing.
    #[test]
    fn no_two_offered_shells_share_an_executable() {
        let offered = available_shells();
        for (i, a) in offered.iter().enumerate() {
            for b in &offered[i + 1..] {
                assert!(
                    !a.path.as_os_str().eq_ignore_ascii_case(b.path.as_os_str()),
                    "{:?} and {:?} both launch {}",
                    a.shell,
                    b.shell,
                    a.path.display()
                );
            }
        }
    }

    /// The dialog selects the first entry by default and documents that
    /// default as "cmd", which only holds while `Shell::ALL` leads with it
    /// and the detection pass preserves that order.
    #[test]
    fn detection_preserves_shell_all_order_starting_at_cmd() {
        assert_eq!(Shell::ALL[0], Shell::Cmd);

        let offered = available_shells();
        assert!(!offered.is_empty(), "detection must never offer nothing");

        let positions: Vec<usize> = offered
            .iter()
            .map(|available| {
                Shell::ALL
                    .iter()
                    .position(|s| *s == available.shell)
                    .expect("an offered shell must be one of Shell::ALL")
            })
            .collect();
        assert!(
            positions.windows(2).all(|w| w[0] < w[1]),
            "offered shells are out of Shell::ALL order: {:?}",
            offered
        );
    }
}
