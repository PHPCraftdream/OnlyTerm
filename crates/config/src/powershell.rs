//! Starting PowerShell sessions on UTF-8.
//!
//! Windows PowerShell 5.1 inherits the console's code page, which on a
//! default install is the system OEM one -- 437 on the machine this was
//! diagnosed on, where the ANSI code page is 1252. Neither can represent
//! Cyrillic, so any such character that has to cross a code page is
//! substituted with a literal `?` before it gets anywhere near the terminal.
//!
//! Terminal-side *output* happens to escape this, which is what makes the
//! symptom so confusing: a console app writing to a real console goes
//! through `WriteConsoleW` in UTF-16, with no code page involved. What does
//! not escape it is everything byte-oriented -- console input, and therefore
//! pasted text, plus pipes to and from native commands.

use portable_pty::CommandBuilder;
use std::ffi::OsStr;
use std::path::Path;

/// Run at PowerShell startup to put the session on UTF-8.
///
/// Assigning `[Console]::OutputEncoding`/`InputEncoding` is what does the
/// work, rather than a plain `chcp 65001`: those setters call
/// `SetConsoleOutputCP`/`SetConsoleCP` *and* rebuild the cached reader and
/// writer, whereas `chcp` alone would leave PowerShell still encoding its
/// own output with the code page it captured at startup -- the console and
/// the shell would then disagree, which is worse than either alone.
/// `$OutputEncoding` is a third, separate thing again, governing what
/// PowerShell writes into a native command's stdin.
///
/// Deliberately free of quote characters so it survives being quoted into a
/// single argument by every spawn path, including `elevate.rs`'s
/// `construct_start_command_line`.
///
/// Verified in a real ConPTY (`onlyterm record`): all three report `utf-8`
/// afterwards and `chcp` reports 65001, with no exception from the
/// `InputEncoding` setter.
pub const POWERSHELL_UTF8_PREAMBLE: &str =
    "[Console]::OutputEncoding = [Console]::InputEncoding = \
     [System.Text.UTF8Encoding]::new($false); $OutputEncoding = [Console]::OutputEncoding";

/// The arguments that run [`POWERSHELL_UTF8_PREAMBLE`] and then leave an
/// interactive shell behind. `-NoExit` is not optional: `-Command` on its own
/// would run the preamble and immediately exit, closing the tab the user just
/// asked for.
pub fn powershell_utf8_args() -> Vec<String> {
    vec![
        "-NoExit".to_string(),
        "-Command".to_string(),
        POWERSHELL_UTF8_PREAMBLE.to_string(),
    ]
}

/// Whether `argv0` names Windows PowerShell or PowerShell 7.
///
/// Matches on the file stem so it recognises a fully qualified path just as
/// well as a bare `powershell.exe` resolved via PATH. `pwsh` already defaults
/// to UTF-8, but is included anyway: re-asserting it costs nothing and keeps
/// the two from drifting apart in behaviour.
fn is_powershell(argv0: &OsStr) -> bool {
    Path::new(argv0)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(|stem| {
            let stem = stem.to_ascii_lowercase();
            stem == "powershell" || stem == "pwsh"
        })
        .unwrap_or(false)
}

/// Whether the arguments already tell PowerShell what to run.
///
/// PowerShell accepts only one of `-Command`/`-File`/`-EncodedCommand`, and
/// `-Command` has to come last because it swallows everything after it, so
/// appending our own would corrupt an invocation that already has one. Its
/// switch matching is case-insensitive and prefix-based (`-c` means
/// `-Command`), which is why this compares by prefix rather than by equality.
///
/// A bare `-e` is treated as `-EncodedCommand` even though it could equally
/// have been meant as `-ExecutionPolicy`; that ambiguity is PowerShell's own,
/// and the resulting false positive merely skips the UTF-8 preamble, which
/// leaves the session exactly as it would have been before. Erring toward
/// skipping is the whole point: never corrupt a command the caller asked for.
fn already_specifies_what_to_run<T: AsRef<OsStr>>(args: &[T]) -> bool {
    args.iter().any(|arg| {
        let Some(arg) = arg.as_ref().to_str() else {
            return false;
        };
        let switch = arg.trim_start_matches(['-', '/']).to_ascii_lowercase();
        !switch.is_empty()
            && ["command", "file", "encodedcommand"]
                .iter()
                .any(|full| full.starts_with(&switch))
    })
}

/// Appends the UTF-8 preamble to `cmd` when it launches PowerShell without
/// already saying what to run.
///
/// Called from `Config::apply_cmd_defaults`, the point every spawn path
/// funnels through, so a PowerShell reached via `default_prog`, an explicit
/// `onlyterm start -- powershell.exe`, or a mux domain spawn all get it.
pub fn ensure_powershell_utf8(cmd: &mut CommandBuilder) {
    let argv = cmd.get_argv();
    let Some((argv0, args)) = argv.split_first() else {
        return;
    };
    if !is_powershell(argv0) || already_specifies_what_to_run(args) {
        return;
    }
    let argv = cmd.get_argv_mut();
    argv.extend(powershell_utf8_args().into_iter().map(Into::into));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv_of(cmd: &CommandBuilder) -> Vec<String> {
        cmd.get_argv()
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    fn build(args: &[&str]) -> CommandBuilder {
        let mut cmd = CommandBuilder::new(args[0]);
        cmd.args(&args[1..]);
        cmd
    }

    #[test]
    fn preamble_needs_no_quote_escaping() {
        // Every spawn path re-quotes this into a single argument, and the
        // elevated one wraps anything containing a space in double quotes; an
        // embedded quote would end that wrapping early and hand PowerShell a
        // truncated command.
        assert!(
            !POWERSHELL_UTF8_PREAMBLE.contains('"') && !POWERSHELL_UTF8_PREAMBLE.contains('\''),
            "preamble must stay quote-free, got: {}",
            POWERSHELL_UTF8_PREAMBLE
        );
    }

    #[test]
    fn preamble_sets_all_three_encodings() {
        // The two `[Console]` assignments are what actually call
        // SetConsoleOutputCP/SetConsoleCP; `$OutputEncoding` separately covers
        // what gets written into a native command's stdin.
        assert!(POWERSHELL_UTF8_PREAMBLE.contains("[Console]::OutputEncoding"));
        assert!(POWERSHELL_UTF8_PREAMBLE.contains("[Console]::InputEncoding"));
        assert!(POWERSHELL_UTF8_PREAMBLE.contains("$OutputEncoding"));
        assert!(POWERSHELL_UTF8_PREAMBLE.contains("UTF8Encoding"));
    }

    #[test]
    fn bare_powershell_gets_the_preamble() {
        let mut cmd = build(&["powershell.exe"]);
        ensure_powershell_utf8(&mut cmd);
        assert_eq!(
            argv_of(&cmd),
            vec![
                "powershell.exe",
                "-NoExit",
                "-Command",
                POWERSHELL_UTF8_PREAMBLE
            ]
        );
    }

    #[test]
    fn fully_qualified_path_and_pwsh_are_recognised() {
        for prog in [
            r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe",
            "PowerShell.EXE",
            "pwsh",
        ] {
            let mut cmd = build(&[prog]);
            ensure_powershell_utf8(&mut cmd);
            assert_eq!(
                argv_of(&cmd).len(),
                4,
                "{prog} should have been given the preamble"
            );
        }
    }

    #[test]
    fn harmless_switches_still_get_the_preamble() {
        let mut cmd = build(&["powershell.exe", "-NoLogo", "-NoProfile"]);
        ensure_powershell_utf8(&mut cmd);
        assert_eq!(
            argv_of(&cmd),
            vec![
                "powershell.exe",
                "-NoLogo",
                "-NoProfile",
                "-NoExit",
                "-Command",
                POWERSHELL_UTF8_PREAMBLE
            ]
        );
    }

    /// PowerShell accepts only one of these and `-Command` must come last, so
    /// appending ours would corrupt what the caller asked to run.
    #[test]
    fn an_existing_command_is_never_clobbered() {
        for args in [
            vec!["powershell.exe", "-Command", "Get-Date"],
            vec!["powershell.exe", "-c", "Get-Date"],
            vec!["powershell.exe", "-File", "script.ps1"],
            vec!["powershell.exe", "-f", "script.ps1"],
            vec!["powershell.exe", "-EncodedCommand", "ZQBjAGgAbwA="],
            vec!["powershell.exe", "-NoProfile", "-Command", "Get-Date"],
        ] {
            let mut cmd = build(&args);
            ensure_powershell_utf8(&mut cmd);
            assert_eq!(
                argv_of(&cmd),
                args,
                "{args:?} already says what to run and must be left alone"
            );
        }
    }

    /// Applying it twice must not stack two `-Command` arguments: the New Tab
    /// Options dialog injects the preamble itself, and its result then flows
    /// through `apply_cmd_defaults` like any other command.
    #[test]
    fn injection_is_idempotent() {
        let mut cmd = build(&["powershell.exe"]);
        ensure_powershell_utf8(&mut cmd);
        let once = argv_of(&cmd);
        ensure_powershell_utf8(&mut cmd);
        assert_eq!(argv_of(&cmd), once);
    }

    #[test]
    fn other_shells_are_left_alone() {
        for prog in ["cmd.exe", "bash.exe", "wsl.exe", "powershell-ish.exe"] {
            let mut cmd = build(&[prog]);
            ensure_powershell_utf8(&mut cmd);
            assert_eq!(argv_of(&cmd), vec![prog], "{prog} must not be touched");
        }
    }
}
