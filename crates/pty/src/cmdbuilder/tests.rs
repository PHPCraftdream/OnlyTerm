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
    cmd.env("Cargo_Pkg_Authors", "Not Wez");
    assert!(cmd.get_env("cargo_pkg_authors") == Some(OsStr::new("Not Wez")));

    cmd.env_remove("cARGO_pKG_aUTHORS");
    assert!(cmd.get_env("CARGO_PKG_AUTHORS").is_none());
}

#[cfg(windows)]
#[test]
fn test_environment_block_skips_empty_name() {
    // Regression test for wezterm/wezterm#4364: a stray environment
    // variable with an empty name (which can end up in the user
    // environment if e.g. the `(Default)` value under
    // `HKEY_CURRENT_USER\Environment` is explicitly set to an empty
    // string) used to be encoded verbatim as a bare `=value\0` entry
    // in the block passed to `CreateProcessW`. That malformed block
    // caused `CreateProcessW` to fail wholesale with
    // ERROR_INVALID_PARAMETER (os error 87), so no program at all
    // could be spawned in the pty. Such entries must be dropped when
    // building the environment block.
    let mut cmd = CommandBuilder::new("dummy");
    cmd.env("", "should be dropped");
    cmd.env("REGULAR_VAR", "regular value");

    let block = cmd.environment_block();
    let block_str = String::from_utf16_lossy(&block[..block.len().saturating_sub(1)]);

    for entry in block_str.split('\0').filter(|s| !s.is_empty()) {
        assert!(
            !entry.starts_with('='),
            "environment block must not contain an entry with an empty \
             name (found {:?}); this produces ERROR_INVALID_PARAMETER \
             from CreateProcessW",
            entry
        );
    }

    assert!(block_str.contains("REGULAR_VAR=regular value"));
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
