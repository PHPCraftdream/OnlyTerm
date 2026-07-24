use config::lua::get_or_create_sub_module;
use config::lua::mlua::{self, Lua, Value};
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Z2: git-based plugin installation has been removed (see the "убрать весь
// C/C++ из wezterm" initiative). `wezterm.plugin.require(url)` used to clone
// (via git2/libgit2, a C library) a plugin repo into
// `DATA_DIR/plugins/<component>` on first use, and `list()`/`update_all()`
// enumerated/fetched those clones. None of that git machinery survives: a
// plugin is now installed by the user placing its files locally and
// `require`ing that local path/directory directly -- no clone-on-demand
// registry, so there is nothing left for `list()`/`update_all()` to enumerate
// or update either. Both are kept as callable functions (so existing configs
// that reference `wezterm.plugin.list`/`update_all` don't hard-crash with "no
// such global") but they now return a clear error explaining the removal
// instead of silently doing nothing.
const GIT_PLUGIN_REMOVED_MESSAGE: &str =
    "git-based plugin installation has been removed from wezterm; install \
     plugins by placing their files locally and requiring a local \
     path/directory instead. See the migration guide.";

/// Given a URL, generate a string that can be used as a directory name.
/// The returned name must be a single valid filesystem component
fn compute_repo_dir(url: &str) -> String {
    let mut dir = String::new();
    for c in url.chars() {
        match c {
            '/' | '\\' => {
                dir.push_str("sZs");
            }
            ':' => {
                dir.push_str("sCs");
            }
            '.' => {
                dir.push_str("sDs");
            }
            '-' | '_' => dir.push(c),
            c if c.is_alphanumeric() => dir.push(c),
            c => dir.push_str(&format!("u{}", c as u32)),
        }
    }
    if dir.ends_with("sZs") {
        dir.truncate(dir.len() - 3);
    }
    dir
}

/// `wezterm.plugin.require(path)`: opens a local plugin directory and
/// `require`s it via Lua's own module resolver. Git-URL installation
/// (`https://...`, `file://...` treated as a git remote, etc.) is no longer
/// supported; see `GIT_PLUGIN_REMOVED_MESSAGE`.
fn require_plugin(lua: &Lua, path: String) -> anyhow::Result<Value<'_>> {
    if looks_like_git_url(&path) {
        anyhow::bail!(GIT_PLUGIN_REMOVED_MESSAGE);
    }

    let dir = Path::new(&path);
    if !dir.is_dir() {
        anyhow::bail!(
            "{path:?} is not a local directory. {}",
            GIT_PLUGIN_REMOVED_MESSAGE
        );
    }

    let component = compute_repo_dir(&path);

    let require: mlua::Function = lua.globals().get("require")?;
    match require.call::<_, Value>(component.clone()) {
        Ok(value) => Ok(value),
        Err(err) => {
            log::error!("Failed to require local plugin at {path:?} (component {component}): {err:#}");
            Err(err.into())
        }
    }
}

/// Heuristic used only to give a better error message: does `path` look like
/// something that used to be accepted as a git remote (a URL scheme, or a
/// scp-like `host:path` git spec) rather than a plain local filesystem path?
fn looks_like_git_url(path: &str) -> bool {
    let schemes = ["https://", "http://", "ssh://", "git://", "file://"];
    if schemes.iter().any(|s| path.starts_with(s)) {
        return true;
    }
    // scp-like syntax, e.g. `git@github.com:owner/repo.git`, but don't
    // misfire on Windows drive-letter paths like `C:\...`.
    if let Some((host, _rest)) = path.split_once(':') {
        if host.contains('@') && !host.contains('/') && !host.contains('\\') {
            return true;
        }
    }
    false
}

fn list_plugins() -> anyhow::Result<()> {
    anyhow::bail!(GIT_PLUGIN_REMOVED_MESSAGE)
}

pub fn register(lua: &Lua) -> anyhow::Result<()> {
    let plugin_mod = get_or_create_sub_module(lua, "plugin")?;
    plugin_mod.set(
        "require",
        lua.create_function(|lua: &Lua, repo_spec: String| {
            require_plugin(lua, repo_spec).map_err(|e| mlua::Error::external(format!("{e:#}")))
        })?,
    )?;

    plugin_mod.set(
        "list",
        lua.create_function(|_, _: ()| {
            list_plugins().map_err(|e| mlua::Error::external(format!("{e:#}")))
        })?,
    )?;

    plugin_mod.set(
        "update_all",
        lua.create_function(|_, _: ()| {
            list_plugins().map_err(|e| mlua::Error::external(format!("{e:#}")))?;
            Ok(())
        })?,
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// L4e: rhai port of `register` above (see
// docs/plans/2026-07-23-lua-rhai-migration.md, thin spot #4). Runs in parallel
// with the mlua path; does not replace or touch `register(lua: &Lua)`.
//
// As of Z2, both the mlua and rhai paths share the same reduced surface: no
// more git cloning, only a local-directory `require`. `require_plugin_rhai`
// looks for `<dir>/plugin/init.rhai` (the rhai analogue of Lua's
// `plugin/init.lua`) and evaluates it with the caller's own `&rhai::Engine`
// via `Engine::eval_file`, so the plugin script runs against the same
// registered functions/modules as the config that `require`d it.

/// Locates the rhai entry point inside a local plugin directory, mirroring
/// the `plugins/?/plugin/init.lua` `package.path` entry the mlua path relies
/// on (see `config/src/lua.rs::make_lua_context`), but for `.rhai` plugins:
/// `<dir>/plugin/init.rhai`.
fn plugin_entry_point(checkout_path: &std::path::Path) -> anyhow::Result<PathBuf> {
    let entry = checkout_path.join("plugin").join("init.rhai");
    if !entry.is_file() {
        anyhow::bail!(
            "no rhai plugin entry point found at {entry:?}. \
             Note: plugins written for wezterm's Lua config engine (a \
             `plugin/init.lua`) are not compatible with the rhai engine and \
             must be republished with a `plugin/init.rhai` entry point."
        );
    }
    Ok(entry)
}

/// rhai analogue of `require_plugin` above: local-directory only, no git.
fn require_plugin_rhai(
    engine: &rhai::Engine,
    path: String,
) -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
    require_plugin_rhai_impl(engine, path)
        .map_err(|e| -> Box<rhai::EvalAltResult> { format!("{e:#}").into() })
}

fn require_plugin_rhai_impl(engine: &rhai::Engine, path: String) -> anyhow::Result<rhai::Dynamic> {
    if looks_like_git_url(&path) {
        anyhow::bail!(GIT_PLUGIN_REMOVED_MESSAGE);
    }

    let dir = Path::new(&path);
    if !dir.is_dir() {
        anyhow::bail!(
            "{path:?} is not a local directory. {}",
            GIT_PLUGIN_REMOVED_MESSAGE
        );
    }

    let entry = plugin_entry_point(dir)?;
    match engine.eval_file::<rhai::Dynamic>(entry.clone()) {
        Ok(value) => Ok(value),
        Err(err) => {
            log::error!("Failed to load rhai plugin at {path:?} (entry {entry:?}): {err:#}");
            // Not `err.into()`: `rhai::EvalAltResult` embeds `Rc`-based AST
            // nodes (see the `FnPtr`/`Dynamic` variants it can carry), so
            // `Box<EvalAltResult>` is not `Sync` and cannot satisfy
            // `anyhow::Error`'s blanket `From<E: std::error::Error + Send +
            // Sync>` impl -- reduce it to a plain string message instead
            // (matching how `config/src/rhai_engine.rs` and every other
            // `_rhai` binding in this workspace bridges rhai errors back into
            // `anyhow`/`Box<EvalAltResult>`).
            anyhow::bail!("{err:#}")
        }
    }
}

/// rhai analogue of `list_plugins` above.
fn list_plugins_rhai() -> Result<rhai::Array, Box<rhai::EvalAltResult>> {
    list_plugins()
        .map(|_| rhai::Array::new())
        .map_err(|e| -> Box<rhai::EvalAltResult> { format!("{e:#}").into() })
}

fn update_all_plugins_rhai() -> Result<(), Box<rhai::EvalAltResult>> {
    list_plugins().map_err(|e| -> Box<rhai::EvalAltResult> { format!("{e:#}").into() })?;
    Ok(())
}

/// Registers the rhai equivalent of `wezterm.plugin.require(path)` /
/// `wezterm.plugin.list()` / `wezterm.plugin.update_all()` under a `plugin`
/// static module, mirroring the mlua path's `wezterm.plugin.*` sub-module
/// (`get_or_create_sub_module(lua, "plugin")` above) as closely as rhai's
/// module system allows -- same call-site shape (`plugin.require(path)`),
/// same three function names.
pub fn register_rhai(engine: &mut rhai::Engine) -> anyhow::Result<()> {
    let mut plugin_module = rhai::Module::new();

    plugin_module.set_native_fn(
        "require",
        |context: rhai::NativeCallContext, path: String| {
            require_plugin_rhai(context.engine(), path)
        },
    );
    plugin_module.set_native_fn("list", list_plugins_rhai);
    plugin_module.set_native_fn("update_all", update_all_plugins_rhai);

    engine.register_static_module("plugin", plugin_module.into());

    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_compute_repo_dir() {
        for (input, expect) in &[
            ("foo", "foo"),
            (
                "githubsDscom/wezterm/wezterm-plugins",
                "githubsDscomsZsweztermsZswezterm-plugins",
            ),
            ("localhost:8080/repo", "localhostsCs8080sZsrepo"),
        ] {
            let result = compute_repo_dir(input);
            assert_eq!(&result, expect, "for input {input}");
        }
    }

    #[test]
    fn test_looks_like_git_url() {
        assert!(looks_like_git_url("https://github.com/owner/repo"));
        assert!(looks_like_git_url("http://example.com/repo.git"));
        assert!(looks_like_git_url("ssh://git@example.com/repo.git"));
        assert!(looks_like_git_url("git://example.com/repo.git"));
        assert!(looks_like_git_url("file:///home/user/projects/myPlugin"));
        assert!(looks_like_git_url("git@github.com:owner/repo.git"));

        assert!(!looks_like_git_url("/home/user/projects/myPlugin"));
        assert!(!looks_like_git_url("relative/path/to/plugin"));
        assert!(!looks_like_git_url(r"C:\Users\user\projects\myPlugin"));
    }

    #[test]
    fn require_plugin_rhai_rejects_git_url() {
        let engine = rhai::Engine::new();
        let err = require_plugin_rhai_impl(&engine, "https://github.com/owner/repo".to_string())
            .expect_err("git URLs must be rejected");
        let message = format!("{err:#}");
        assert!(
            message.contains("git-based plugin installation has been removed"),
            "got: {message}"
        );
    }

    #[test]
    fn require_plugin_rhai_rejects_missing_local_dir() {
        let engine = rhai::Engine::new();
        let err = require_plugin_rhai_impl(&engine, "/no/such/directory/hopefully".to_string())
            .expect_err("missing directory should fail");
        let message = format!("{err:#}");
        assert!(
            message.contains("not a local directory"),
            "got: {message}"
        );
    }

    #[test]
    fn plugin_entry_point_finds_init_rhai() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(dir.path().join("plugin")).unwrap();
        std::fs::write(dir.path().join("plugin").join("init.rhai"), "42").unwrap();

        let found = plugin_entry_point(dir.path()).expect("should find init.rhai");
        assert_eq!(found, dir.path().join("plugin").join("init.rhai"));
    }

    #[test]
    fn plugin_entry_point_reports_missing_rhai_entry_even_if_lua_entry_exists() {
        // This is the concrete shape of "existing Lua plugins don't work
        // anymore": a directory with only `plugin/init.lua` (the old entry
        // point) and no `plugin/init.rhai` must fail to resolve on the rhai
        // path, with an error that says so explicitly.
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(dir.path().join("plugin")).unwrap();
        std::fs::write(
            dir.path().join("plugin").join("init.lua"),
            "return { hello = true }",
        )
        .unwrap();

        let err = plugin_entry_point(dir.path()).expect_err("no init.rhai present");
        let message = format!("{err:#}");
        assert!(
            message.contains("init.rhai"),
            "error should mention init.rhai, got: {message}"
        );
        assert!(
            message.contains("not compatible"),
            "error should explain the Lua/rhai incompatibility, got: {message}"
        );
    }

    #[test]
    fn require_plugin_rhai_evaluates_a_local_rhai_plugin() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        std::fs::create_dir_all(dir.path().join("plugin"))?;
        std::fs::write(
            dir.path().join("plugin").join("init.rhai"),
            r#"#{ answer: 42, name: "demo-plugin" }"#,
        )?;

        let path = dir
            .path()
            .to_str()
            .expect("temp dir path should be utf8")
            .to_string();

        let engine = rhai::Engine::new();
        let result = require_plugin_rhai_impl(&engine, path)?;

        let map = result.try_cast::<rhai::Map>().expect("script returns a map");
        assert_eq!(map.get("answer").and_then(|v| v.as_int().ok()), Some(42));
        assert_eq!(
            map.get("name").and_then(|v| v.clone().into_string().ok()),
            Some("demo-plugin".to_string())
        );

        Ok(())
    }

    #[test]
    fn require_plugin_rhai_reports_a_useful_error_for_a_lua_only_plugin() -> anyhow::Result<()> {
        // Regression test for the migration's central breaking-change claim:
        // a plugin published with only a Lua entry point (`plugin/init.lua`,
        // no `plugin/init.rhai`) must fail loudly on the rhai path rather than
        // silently doing nothing or picking up the Lua file by mistake.
        let dir = tempfile::tempdir()?;
        std::fs::create_dir_all(dir.path().join("plugin"))?;
        std::fs::write(
            dir.path().join("plugin").join("init.lua"),
            "return { legacy = true }",
        )?;

        let path = dir.path().to_str().unwrap().to_string();

        let engine = rhai::Engine::new();
        let err = require_plugin_rhai_impl(&engine, path)
            .expect_err("a Lua-only plugin has no rhai entry point to load");
        let message = format!("{err:#}");
        assert!(
            message.contains("init.rhai"),
            "expected error to mention the missing rhai entry point, got: {message}"
        );

        Ok(())
    }

    #[test]
    fn list_and_update_all_rhai_report_removal() {
        let err = list_plugins_rhai().expect_err("list() should be removed");
        assert!(format!("{err}").contains("git-based plugin installation has been removed"));

        let err = update_all_plugins_rhai().expect_err("update_all() should be removed");
        assert!(format!("{err}").contains("git-based plugin installation has been removed"));
    }

    #[test]
    fn register_rhai_exposes_plugin_require_list_update_all() -> anyhow::Result<()> {
        // Smoke test that `register_rhai` wires up the `plugin` module under
        // the exact names the mlua path exposes under `wezterm.plugin.*`
        // (`require`/`list`/`update_all`), callable from a `.rhai` script the
        // way a real config would call them.
        let dir = tempfile::tempdir()?;
        std::fs::create_dir_all(dir.path().join("plugin"))?;
        std::fs::write(dir.path().join("plugin").join("init.rhai"), r#"#{ ok: true }"#)?;
        let path = dir.path().to_str().unwrap().to_string();

        let mut engine = rhai::Engine::new();
        register_rhai(&mut engine)?;

        let script = format!(r#"plugin::require("{}")"#, path.replace('\\', "\\\\"));
        let result: rhai::Dynamic = engine
            .eval(&script)
            .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
        let map = result.try_cast::<rhai::Map>().expect("script returns a map");
        assert_eq!(map.get("ok").and_then(|v| v.as_bool().ok()), Some(true));

        // list()/update_all() are retained as callable names but now report
        // that git-based plugin management has been removed.
        let list_err = engine
            .eval::<rhai::Dynamic>("plugin::list()")
            .expect_err("list() should error");
        assert!(format!("{list_err}").contains("git-based plugin installation has been removed"));

        let update_err = engine
            .eval::<()>("plugin::update_all()")
            .expect_err("update_all() should error");
        assert!(
            format!("{update_err}").contains("git-based plugin installation has been removed")
        );

        Ok(())
    }
}
