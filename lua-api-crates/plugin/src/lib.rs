use anyhow::{anyhow, Context};
use config::lua::get_or_create_sub_module;
use config::lua::mlua::{self, Lua, Value};
use config::impl_rhai_conversion_dynamic;
use git2::build::CheckoutBuilder;
use git2::{Remote, Repository};
use luahelper::to_lua;
use std::path::PathBuf;
use tempfile::TempDir;
use wezterm_dynamic::{FromDynamic, ToDynamic};

#[derive(FromDynamic, ToDynamic, Debug, Clone)]
struct RepoSpec {
    url: String,
    component: String,
    plugin_dir: PathBuf,
}

// Gives `RepoSpec` a `TryFrom<rhai::Dynamic>`/`From<RepoSpec> for rhai::Dynamic`
// pair for free from its existing `FromDynamic`/`ToDynamic` derive above, the
// rhai analogue of how `to_lua` (used by the mlua `list` binding below) already
// converts it via `wezterm_dynamic`. Used by `list_rhai` to turn a
// `Vec<RepoSpec>` into a rhai `Array` the same way `list` turns it into a Lua
// table.
impl_rhai_conversion_dynamic!(RepoSpec);

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

fn get_remote(repo: &Repository) -> anyhow::Result<Option<Remote<'_>>> {
    let remotes = repo.remotes()?;
    for remote in remotes.iter() {
        if let Some(name) = remote {
            let remote = repo.find_remote(name)?;
            return Ok(Some(remote));
        }
    }
    Ok(None)
}

impl RepoSpec {
    fn parse(url: String) -> anyhow::Result<Self> {
        let component = compute_repo_dir(&url);
        if component.starts_with('.') {
            anyhow::bail!("invalid repo spec {url}");
        }

        let plugin_dir = RepoSpec::plugins_dir().join(&component);

        Ok(Self {
            url,
            component,
            plugin_dir,
        })
    }

    fn load_from_dir(path: PathBuf) -> anyhow::Result<Self> {
        let component = path
            .file_name()
            .ok_or_else(|| anyhow!("missing file name!?"))?
            .to_str()
            .ok_or_else(|| anyhow!("{path:?} isn't unicode"))?
            .to_string();

        let plugin_dir = RepoSpec::plugins_dir().join(&component);

        let repo = Repository::open(&path)?;
        let remote = get_remote(&repo)?.ok_or_else(|| anyhow!("no remotes!?"))?;
        let url = remote.url();
        if let Some(url) = url {
            let url = url.to_string();
            return Ok(Self {
                component,
                url,
                plugin_dir,
            });
        }
        anyhow::bail!("Unable to create a complete RepoSpec for repo at {path:?}");
    }

    fn plugins_dir() -> PathBuf {
        config::DATA_DIR.join("plugins")
    }

    fn checkout_path(&self) -> PathBuf {
        Self::plugins_dir().join(&self.component)
    }

    fn is_checked_out(&self) -> bool {
        self.checkout_path().exists()
    }

    fn update(&self) -> anyhow::Result<()> {
        let path = self.checkout_path();
        let repo = Repository::open(&path)?;
        let mut remote = get_remote(&repo)?.ok_or_else(|| anyhow!("no remotes!?"))?;
        remote.connect(git2::Direction::Fetch).context("connect")?;
        let branch = remote
            .default_branch()
            .context("get default branch")?
            .as_str()
            .ok_or_else(|| anyhow!("default branch is not utf8"))?
            .to_string();

        remote.fetch(&[branch], None, None).context("fetch")?;
        let mut merge_info = None;
        repo.fetchhead_foreach(|refname, _remote_url, target_oid, was_merge| {
            if was_merge {
                merge_info.replace((refname.to_string(), *target_oid));
                return true;
            }
            false
        })
        .context("fetchhead_foreach")?;

        let (refname, target_oid) = merge_info.ok_or_else(|| anyhow!("No merge info!?"))?;
        let commit = repo
            .find_annotated_commit(target_oid)
            .context("find_annotated_commit")?;

        let (analysis, _preference) = repo.merge_analysis(&[&commit]).context("merge_analysis")?;
        if analysis.is_up_to_date() {
            log::debug!("{} is up to date!", self.component);
            return Ok(());
        }
        if analysis.is_fast_forward() {
            log::debug!("{} can fast forward!", self.component);
            let mut reference = repo.find_reference(&refname).context("find_reference")?;
            reference
                .set_target(target_oid, "fast forward")
                .context("set_target")?;
            repo.checkout_head(Some(CheckoutBuilder::new().force()))
                .context("checkout_head")?;
            return Ok(());
        }

        log::debug!("{} will merge", self.component);
        repo.merge(&[&commit], None, Some(CheckoutBuilder::new().safe()))
            .context("merge")?;
        Ok(())
    }

    fn check_out(&self) -> anyhow::Result<()> {
        let plugins_dir = Self::plugins_dir();
        std::fs::create_dir_all(&plugins_dir)?;
        let target_dir = TempDir::new_in(&plugins_dir)?;
        log::debug!("Cloning {} into temporary dir {target_dir:?}", self.url);
        Repository::clone_recurse(&self.url, target_dir.path())?;
        let target_dir = target_dir.keep();
        let checkout_path = self.checkout_path();
        match std::fs::rename(&target_dir, &checkout_path) {
            Ok(_) => {
                log::info!("Cloned {} into {checkout_path:?}", self.url);
                Ok(())
            }
            Err(err) => {
                log::error!(
                    "Failed to rename {target_dir:?} -> {:?}, removing temporary dir",
                    self.checkout_path()
                );
                if let Err(err) = std::fs::remove_dir_all(&target_dir) {
                    log::error!(
                        "Failed to remove {target_dir:?}: {err:#}, \
                         you will need to remove it manually"
                    );
                }
                Err(err.into())
            }
        }
    }
}

fn require_plugin(lua: &Lua, url: String) -> anyhow::Result<Value<'_>> {
    let spec = RepoSpec::parse(url)?;

    if !spec.is_checked_out() {
        spec.check_out()?;
    }

    let require: mlua::Function = lua.globals().get("require")?;
    match require.call::<_, Value>(spec.component.to_string()) {
        Ok(value) => Ok(value),
        Err(err) => {
            log::error!(
                "Failed to require {} which is stored in {:?}: {err:#}",
                spec.component,
                spec.checkout_path()
            );
            Err(err.into())
        }
    }
}

fn list_plugins() -> anyhow::Result<Vec<RepoSpec>> {
    let mut plugins = vec![];

    let plugins_dir = RepoSpec::plugins_dir();
    std::fs::create_dir_all(&plugins_dir)?;

    for entry in plugins_dir.read_dir()? {
        let entry = entry?;
        if entry.path().is_dir() {
            plugins.push(RepoSpec::load_from_dir(entry.path())?);
        }
    }

    Ok(plugins)
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
        lua.create_function(|lua, _: ()| {
            let plugins = list_plugins().map_err(|e| mlua::Error::external(format!("{e:#}")))?;
            to_lua(lua, plugins)
        })?,
    )?;

    plugin_mod.set(
        "update_all",
        lua.create_function(|_, _: ()| {
            let plugins = list_plugins().map_err(|e| mlua::Error::external(format!("{e:#}")))?;
            for p in plugins {
                match p.update() {
                    Ok(_) => log::info!("Updated {p:?}"),
                    Err(err) => log::error!("Failed to update {p:?}: {err:#}"),
                }
            }
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
// ## What stays identical
//
// The entire loader *mechanism* -- URL -> filesystem-safe directory name
// (`compute_repo_dir`), the on-disk cache under `DATA_DIR/plugins/<component>`
// (`RepoSpec::plugins_dir`/`checkout_path`/`is_checked_out`), the git clone
// (`RepoSpec::check_out`, still via `git2`) and fetch/fast-forward/merge update
// flow (`RepoSpec::update`) -- is exactly the same Rust code as the mlua path
// above; none of `RepoSpec`'s methods touch `mlua` at all, so nothing there
// needed to change or be duplicated.
//
// ## What's genuinely engine-specific: the entry point
//
// `require_plugin` (mlua path) hands the freshly-cloned directory's *component
// name* off to Lua's own `require(...)`, which walks `package.path` --
// including the `plugins/?/plugin/init.lua` entry inserted in
// `config/src/lua.rs::make_lua_context` -- to find and `dofile`-equivalent-load
// `<checkout>/plugin/init.lua`, executing it in the *same* `Lua` context that's
// running the user's config (so the plugin can see/extend `wezterm.*`) and
// returning whatever value that script returns.
//
// rhai has no `require`/module-resolver wired up yet for this purpose (see the
// "Config reload watch list" section of the module doc comment in
// `config/src/rhai_engine.rs`: a real `ModuleResolver` is explicitly deferred
// to L4/L5 as "speculative" until something actually needs it). Since this task
// *is* that something, `require_plugin_rhai` below implements the narrower,
// concrete piece it actually needs directly: it looks for
// `<checkout>/plugin/init.rhai` (the rhai analogue of Lua's
// `plugin/init.lua`, i.e. same subdirectory, `.rhai` extension instead of
// `.lua`) and evaluates it with the caller's own `&rhai::Engine` via
// `Engine::eval_file`, so the plugin script runs against the same registered
// functions/modules as the config that `require`d it -- matching the mlua
// path's "same context" property -- and returns whatever `Dynamic` the script's
// last expression evaluates to, mirroring the mlua path's `Value` return.
//
// This does NOT make existing third-party Lua plugins work: a plugin published
// as `plugin/init.lua` has no `plugin/init.rhai` counterpart to find, so
// `require_plugin_rhai` will simply fail to locate an entry point for it (see
// `plugin_entry_point`'s error message). That is the intended, accepted
// consequence of the language switch (migration plan thin spot #4): plugin
// *authors* need to publish a `.rhai` entry point (or a compatibility shim) for
// their plugin to keep working once wezterm's config engine defaults to rhai;
// nothing in this loader can paper over that, nor should it try to.

/// Locates the rhai entry point inside a checked-out plugin directory,
/// mirroring the `plugins/?/plugin/init.lua` `package.path` entry the mlua
/// path relies on (see `config/src/lua.rs::make_lua_context`), but for
/// `.rhai` plugins: `<checkout>/plugin/init.rhai`.
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

/// rhai analogue of `require_plugin` above. Same clone-if-missing-then-load
/// flow; see the module-level comment above for what differs (the entry point
/// lookup) and what's intentionally identical (everything in `RepoSpec`).
fn require_plugin_rhai(
    engine: &rhai::Engine,
    url: String,
) -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
    require_plugin_rhai_impl(engine, url)
        .map_err(|e| -> Box<rhai::EvalAltResult> { format!("{e:#}").into() })
}

fn require_plugin_rhai_impl(engine: &rhai::Engine, url: String) -> anyhow::Result<rhai::Dynamic> {
    let spec = RepoSpec::parse(url)?;

    if !spec.is_checked_out() {
        spec.check_out()?;
    }

    let entry = plugin_entry_point(&spec.checkout_path())?;
    match engine.eval_file::<rhai::Dynamic>(entry.clone()) {
        Ok(value) => Ok(value),
        Err(err) => {
            log::error!(
                "Failed to load rhai plugin {} which is stored in {entry:?}: {err:#}",
                spec.component,
            );
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

/// rhai analogue of `list_plugins` above -- identical implementation (it never
/// touched `mlua` in the first place), re-exposed for the rhai-facing `list`
/// binding below to keep both `register`/`register_rhai` symmetrical.
fn list_plugins_rhai() -> Result<rhai::Array, Box<rhai::EvalAltResult>> {
    let plugins = list_plugins()
        .map_err(|e| -> Box<rhai::EvalAltResult> { format!("{e:#}").into() })?;
    // Not `.map(rhai::Dynamic::from)`: `rhai::Dynamic` has an *inherent*
    // `Dynamic::from<T: Variant + Clone>` associated function (rhai's generic
    // "wrap any clonable Rust value as an opaque native object" constructor),
    // and inherent associated functions always shadow same-named trait methods
    // when called via the type path -- so `rhai::Dynamic::from(repo_spec)`
    // would silently pick that generic wrapper instead of the
    // `impl_rhai_conversion_dynamic!`-generated `impl From<RepoSpec> for
    // Dynamic` above (which the `.get_by_str("component")`-style field access
    // `require_plugin_rhai`'s callers need actually relies on: a rhai `Map`,
    // not an opaque `RepoSpec` object). `.into()` goes through the `Into`
    // trait unambiguously instead, correctly resolving to the macro-generated
    // impl -- the same pattern every other `_rhai` binding in this workspace
    // uses for a config-struct-to-`Dynamic` conversion (e.g.
    // `color-funcs`'s `load_scheme_rhai` returning `scheme.into()`).
    Ok(plugins.into_iter().map(|p| p.into()).collect())
}

fn update_all_plugins_rhai() -> Result<(), Box<rhai::EvalAltResult>> {
    let plugins = list_plugins()
        .map_err(|e| -> Box<rhai::EvalAltResult> { format!("{e:#}").into() })?;
    for p in plugins {
        match p.update() {
            Ok(_) => log::info!("Updated {p:?}"),
            Err(err) => log::error!("Failed to update {p:?}: {err:#}"),
        }
    }
    Ok(())
}

/// Registers the rhai equivalent of `wezterm.plugin.require(url)` /
/// `wezterm.plugin.list()` / `wezterm.plugin.update_all()` under a `plugin`
/// static module, mirroring the mlua path's `wezterm.plugin.*` sub-module
/// (`get_or_create_sub_module(lua, "plugin")` above) as closely as rhai's
/// module system allows -- same call-site shape (`plugin.require(url)`),
/// same three function names.
///
/// `require` needs access to the `&Engine` it's being called against (to
/// evaluate the plugin's `.rhai` entry point in that same engine, preserving
/// the mlua path's "plugin runs in the caller's own Lua context" property --
/// see the module-level comment above). rhai's mechanism for a registered
/// function to obtain the calling `&Engine`/`&AST` context is a leading
/// `rhai::NativeCallContext` parameter, recognized specially by rhai's
/// function-registration machinery -- this works the same way whether the
/// function is registered via `Engine::register_fn` or, as here,
/// `rhai::Module::set_native_fn`, so `require` can live in the same `plugin`
/// module as `list`/`update_all` rather than needing a separate top-level
/// registration.
pub fn register_rhai(engine: &mut rhai::Engine) -> anyhow::Result<()> {
    let mut plugin_module = rhai::Module::new();

    plugin_module.set_native_fn(
        "require",
        |context: rhai::NativeCallContext, url: String| {
            require_plugin_rhai(context.engine(), url)
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

    // -----------------------------------------------------------------------
    // L4e rhai-path tests (see docs/plans/2026-07-23-lua-rhai-migration.md).
    //
    // ## Avoiding real network git-clone in tests
    //
    // `RepoSpec::check_out` clones via `git2::Repository::clone_recurse(&self.url,
    // ...)`, and `git2`/libgit2 treat a plain filesystem path exactly like any
    // other clone source -- there is no special-casing of "remote" URLs, a local
    // path just resolves through libgit2's own local-transport instead of the
    // smart-http/ssh transports. So rather than mocking or abstracting away the
    // network layer, these tests build a throwaway *local* git repository (via
    // `git2::Repository::init` + a single commit containing a
    // `plugin/init.rhai` file) in a temp directory, then pass that directory's
    // path as the "url" to `RepoSpec::parse`/`check_out`. This exercises the
    // exact real production code path end to end (clone, cache-dir layout,
    // entry-point resolution, rhai evaluation) with zero code changed or
    // stubbed out for testability, and zero network access.
    //
    // ## Isolating the plugin cache directory
    //
    // `RepoSpec::plugins_dir()` resolves to `config::DATA_DIR.join("plugins")`,
    // and `config::DATA_DIR` is a process-wide `lazy_static` computed once from
    // the environment -- it can't be pointed at a per-test temp directory. Since
    // `compute_repo_dir` derives the cache subdirectory name deterministically
    // from the source URL, each test instead uses a unique throwaway *source*
    // path (a fresh `tempfile::TempDir`, whose path already contains a
    // process-unique temp name), which guarantees a unique, collision-free
    // `component`/cache subdirectory even though all tests share the same real
    // `DATA_DIR/plugins` parent -- and each test removes its own cache
    // subdirectory afterward so repeat runs don't see a stale checkout and skip
    // the clone.
    //
    // That per-test uniqueness handles cross-test collisions on any single
    // cache *subdirectory*, but `list_plugins`/`list_plugins_rhai`/
    // `update_all_plugins_rhai` don't look at one subdirectory -- they
    // `read_dir` the whole shared `DATA_DIR/plugins` *parent*, and
    // `#[test]` functions in the same binary run concurrently on separate
    // threads by default. So a test that calls `list_plugins_rhai` can observe
    // another concurrently-running test's checkout mid-clone (not yet a valid
    // repo) or mid-cleanup (partially removed), which is a real race, not a
    // production concern (`list`/`update_all` are only ever invoked from a
    // single-threaded config-reload path in the real product) -- so it's
    // handled here with a test-only global lock rather than by changing
    // `list_plugins`'s production behavior.
    static SHARED_PLUGINS_DIR_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    use std::path::Path;

    /// Creates a local git repository at `repo_dir` (which must already exist
    /// and be empty) containing a single commit that adds `plugin/init.rhai`
    /// with the given script contents. Returns nothing; the caller already has
    /// `repo_dir`.
    fn make_local_plugin_repo(repo_dir: &Path, entry_script: &str) {
        let repo = Repository::init(repo_dir).expect("git init");

        let plugin_subdir = repo_dir.join("plugin");
        std::fs::create_dir_all(&plugin_subdir).expect("create plugin/ subdir");
        std::fs::write(plugin_subdir.join("init.rhai"), entry_script).expect("write init.rhai");

        let mut index = repo.index().expect("get index");
        index
            .add_path(Path::new("plugin/init.rhai"))
            .expect("stage init.rhai");
        index.write().expect("write index");
        let tree_id = index.write_tree().expect("write tree");
        let tree = repo.find_tree(tree_id).expect("find tree");

        let sig = git2::Signature::now("Test", "test@example.com").expect("signature");
        repo.commit(Some("HEAD"), &sig, &sig, "initial commit", &tree, &[])
            .expect("commit");
    }

    /// Removes whatever `RepoSpec` cached for `spec`, so a test can assert
    /// `check_out()`/`require_plugin_rhai_impl` behavior from a clean slate
    /// even though `DATA_DIR/plugins` is shared process-wide state (see the
    /// module comment above).
    fn clear_cache(spec: &RepoSpec) {
        let _ = std::fs::remove_dir_all(spec.checkout_path());
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
        // anymore": a checkout with only `plugin/init.lua` (the old entry
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
    fn require_plugin_rhai_clones_caches_and_evaluates_a_local_rhai_plugin(
    ) -> anyhow::Result<()> {
        let _guard = SHARED_PLUGINS_DIR_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let source = tempfile::tempdir()?;
        make_local_plugin_repo(source.path(), r#"#{ answer: 42, name: "demo-plugin" }"#);

        let url = source
            .path()
            .to_str()
            .expect("temp dir path should be utf8")
            .to_string();
        let spec = RepoSpec::parse(url.clone())?;
        clear_cache(&spec);
        assert!(
            !spec.is_checked_out(),
            "precondition: cache should start empty"
        );

        let engine = rhai::Engine::new();
        let result = require_plugin_rhai_impl(&engine, url.clone())?;

        assert!(
            spec.is_checked_out(),
            "check_out() should have populated the cache directory"
        );
        assert!(
            spec.checkout_path().join("plugin").join("init.rhai").is_file(),
            "cloned checkout should contain plugin/init.rhai"
        );

        let map = result.try_cast::<rhai::Map>().expect("script returns a map");
        assert_eq!(
            map.get("answer").and_then(|v| v.as_int().ok()),
            Some(42)
        );
        assert_eq!(
            map.get("name").and_then(|v| v.clone().into_string().ok()),
            Some("demo-plugin".to_string())
        );

        // Second call should hit the cache (no re-clone) and still evaluate
        // correctly -- exercises the `is_checked_out()` short-circuit in
        // `require_plugin_rhai_impl` the same way the mlua path's
        // `require_plugin` does.
        let result_again = require_plugin_rhai_impl(&engine, url)?;
        let map_again = result_again
            .try_cast::<rhai::Map>()
            .expect("script returns a map");
        assert_eq!(map_again.get("answer").and_then(|v| v.as_int().ok()), Some(42));

        clear_cache(&spec);
        Ok(())
    }

    #[test]
    fn require_plugin_rhai_reports_a_useful_error_for_a_lua_only_plugin() -> anyhow::Result<()> {
        // Regression test for the migration's central breaking-change claim:
        // a plugin published with only a Lua entry point (`plugin/init.lua`,
        // no `plugin/init.rhai`) must fail loudly on the rhai path rather than
        // silently doing nothing or picking up the Lua file by mistake.
        let _guard = SHARED_PLUGINS_DIR_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let source = tempfile::tempdir()?;
        let repo = Repository::init(source.path())?;
        let plugin_subdir = source.path().join("plugin");
        std::fs::create_dir_all(&plugin_subdir)?;
        std::fs::write(plugin_subdir.join("init.lua"), "return { legacy = true }")?;
        let mut index = repo.index()?;
        index.add_path(Path::new("plugin/init.lua"))?;
        index.write()?;
        let tree_id = index.write_tree()?;
        let tree = repo.find_tree(tree_id)?;
        let sig = git2::Signature::now("Test", "test@example.com")?;
        repo.commit(Some("HEAD"), &sig, &sig, "lua-only plugin", &tree, &[])?;

        let url = source.path().to_str().unwrap().to_string();
        let spec = RepoSpec::parse(url.clone())?;
        clear_cache(&spec);

        let engine = rhai::Engine::new();
        let err = require_plugin_rhai_impl(&engine, url)
            .expect_err("a Lua-only plugin has no rhai entry point to load");
        let message = format!("{err:#}");
        assert!(
            message.contains("init.rhai"),
            "expected error to mention the missing rhai entry point, got: {message}"
        );

        clear_cache(&spec);
        Ok(())
    }

    #[test]
    fn list_plugins_rhai_reflects_a_freshly_cloned_plugin() -> anyhow::Result<()> {
        let _guard = SHARED_PLUGINS_DIR_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let source = tempfile::tempdir()?;
        make_local_plugin_repo(source.path(), "()");
        let url = source.path().to_str().unwrap().to_string();
        let spec = RepoSpec::parse(url.clone())?;
        clear_cache(&spec);

        let engine = rhai::Engine::new();
        let _ = require_plugin_rhai_impl(&engine, url)?;

        let listed = list_plugins_rhai().map_err(|e| anyhow::anyhow!("{e}"))?;
        let found = listed.iter().any(|v| {
            v.clone()
                .try_cast::<rhai::Map>()
                .and_then(|m| m.get("component").cloned())
                .and_then(|c| c.into_string().ok())
                .as_deref()
                == Some(spec.component.as_str())
        });
        assert!(
            found,
            "expected list_plugins_rhai() to include the just-cloned component {}",
            spec.component
        );

        clear_cache(&spec);
        Ok(())
    }

    #[test]
    fn register_rhai_exposes_plugin_require_list_update_all() -> anyhow::Result<()> {
        // Smoke test that `register_rhai` wires up the `plugin` module under
        // the exact names the mlua path exposes under `wezterm.plugin.*`
        // (`require`/`list`/`update_all`), callable from a `.rhai` script the
        // way a real config would call them.
        let _guard = SHARED_PLUGINS_DIR_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let source = tempfile::tempdir()?;
        make_local_plugin_repo(source.path(), r#"#{ ok: true }"#);
        let url = source.path().to_str().unwrap().to_string();
        let spec = RepoSpec::parse(url.clone())?;
        clear_cache(&spec);

        let mut engine = rhai::Engine::new();
        register_rhai(&mut engine)?;

        let script = format!(r#"plugin::require("{}")"#, url.replace('\\', "\\\\"));
        let result: rhai::Dynamic = engine
            .eval(&script)
            .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
        let map = result.try_cast::<rhai::Map>().expect("script returns a map");
        assert_eq!(map.get("ok").and_then(|v| v.as_bool().ok()), Some(true));

        let list_result: rhai::Dynamic = engine
            .eval("plugin::list()")
            .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
        assert!(list_result.is_array(), "plugin::list() should return an array");

        // update_all() should run without error even though our test repo has
        // no real remote branch to fast-forward against beyond itself.
        let _: () = engine
            .eval("plugin::update_all()")
            .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;

        clear_cache(&spec);
        Ok(())
    }
}
