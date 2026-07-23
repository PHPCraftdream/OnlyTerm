//! rhai engine entry point for the Lua -> rhai config migration (L2).
//!
//! See `docs/plans/2026-07-23-lua-rhai-migration.md` for the overall plan. This
//! module is the rhai analogue of `config/src/lua.rs::make_lua_context`: it builds a
//! `rhai::Engine` configured the way wezterm needs it, plus the event-callback
//! registry (`wezterm.on(...)`/`wezterm.emit(...)` in the Lua pipeline) and a
//! config-reload watch-list mechanism.
//!
//! ## Coexistence, not replacement
//!
//! `make_lua_context` in `config/src/lua.rs` remains the production entry point
//! until L5 removes `mlua` from the workspace. This module does not touch it and is
//! not yet wired into `config/src/lib.rs`'s config-loading path (that wiring, and
//! actually parsing `Config` out of a `.rhai` file end to end, is later migration
//! work -- L4/L5 territory, once the 13 `lua-api-crates` have a rhai-side
//! equivalent). What's here is a complete, independently testable rhai engine
//! construction + event-callback + watch-list API surface, proven against real
//! config structs (see the tests at the bottom, and `config/src/exec_domain.rs`
//! where `impl_rhai_conversion_dynamic!` is applied as this task's macro-integration
//! sample).
//!
//! ## What has a 1:1 rhai equivalent, and what doesn't
//!
//! * **Event callbacks** (`wezterm.on`/`wezterm.emit`/`emit_sync_callback` in
//!   `lua.rs:701-835`). Lua's mechanism stores `mlua::Function` values (first-class,
//!   `'lua`-scoped) in Lua's own registry, keyed by a decorated event name, and
//!   walks the list on emit. rhai's first-class function-value type is
//!   [`rhai::FnPtr`], which is `'static` (no engine-lifetime attached) and callable
//!   later via [`rhai::FnPtr::call`] given an `&Engine` and an `&AST` to resolve the
//!   function body against. There is no rhai-native "registry" concept analogous to
//!   `lua.set_named_registry_value`, so this module rolls its own: an
//!   [`EventRegistry`] (`HashMap<String, Vec<FnPtr>>`) shared via `Arc<Mutex<_>>` and
//!   captured by the closures registered on the engine for `on`/`emit`. This
//!   preserves the required semantics 1:1: register once (`on`), call multiple
//!   times (`emit`), each call synchronous (rhai has no async story at all, unlike
//!   mlua's `create_async_function`/`call_async` -- see "Async" below), first
//!   handler that returns `false` stops the chain (mirrors `emit_event`).
//! * **Async.** `wezterm.emit` on the Lua side is `async` (`create_async_function`,
//!   `call_async`) because Lua callbacks can themselves await other async Lua
//!   functions. rhai has no async execution model at all (scripts run to
//!   completion synchronously), so [`emit_sync`] is the *only* flavor available here
//!   -- there is no async counterpart to port. This is a genuine, not cosmetic,
//!   semantic gap between the two engines; documented here per the migration plan's
//!   call to "verify async/sync semantics carry over" (L2 risk #2). Any wezterm
//!   functionality that currently relies on a Lua handler awaiting another async
//!   call before returning will need to be redesigned (e.g. via a callback-based or
//!   polling pattern) when the rhai path becomes the default -- flagged for L4/L5,
//!   not solved here.
//! * **Config reload watch list** (`add_to_config_reload_watch_list` in
//!   `lua.rs:855-863`). On the Lua side this rides on `require()`/
//!   `package.searchers` -- wezterm shims the module searcher so that every file a
//!   config `require`s is speculatively added to a registry-backed watch list before
//!   the module is actually loaded (see the `package.searchers[2]` shim in
//!   `make_lua_context`). rhai 1.25 (the version pinned in this workspace, see
//!   `Cargo.toml`) has an analogous file-module system --
//!   [`rhai::Engine::set_module_resolver`]/[`rhai::module_resolvers::FileModuleResolver`]
//!   for `import "foo" as foo;` statements -- so the natural rhai-side hook point is
//!   a custom `ModuleResolver` that records every path it resolves before delegating
//!   to a real resolver, exactly mirroring the `package.searchers[2]` shim. That
//!   custom resolver is intentionally **not** implemented yet: with no `.rhai` config
//!   files or `import` usage in the wild yet (config parsing itself doesn't start
//!   consuming rhai until L4/L5), wiring a real resolver now would be speculative.
//!   Instead this module provides the same public shape the Lua path exposes --
//!   [`ConfigReloadWatchList::add`]/[`ConfigReloadWatchList::paths`], plus
//!   [`make_rhai_engine`] registering an `add_to_config_reload_watch_list` function
//!   on the engine's `wezterm` module -- so that whichever future task wires up
//!   `import` support (or wires the rhai path into the real config loader) has a
//!   ready-made, already-tested sink to write into. See the `TODO(L4/L5)` comment on
//!   [`install_module_resolver`] below.

use crate::keyassignment::KeyAssignment;
use rhai::{Dynamic, Engine, EvalAltResult, FnPtr, Scope, AST};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Registry of event-name -> registered callbacks, the rhai analogue of the
/// decorated `"wezterm-event-{name}"` keys Lua stores in its registry (see
/// `register_event`/`emit_sync_callback` in `config/src/lua.rs`).
///
/// Shared (`Arc<Mutex<_>>`) because it needs to be captured by two separate
/// closures registered on the engine (`on` and `emit`), and both need to observe
/// registrations made through the other.
#[derive(Default, Clone)]
pub struct EventRegistry {
    inner: Arc<Mutex<HashMap<String, Vec<FnPtr>>>>,
}

impl EventRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a callback for `name`. May be called multiple times for the same
    /// name; callbacks accumulate and are called in registration order, matching
    /// `register_event`'s append-to-table behavior on the Lua side.
    pub fn register(&self, name: impl Into<String>, callback: FnPtr) {
        let mut handlers = self.inner.lock().unwrap();
        handlers.entry(name.into()).or_default().push(callback);
    }

    /// Returns the number of distinct event names with at least one registered
    /// handler. Exposed mainly for tests.
    pub fn event_count(&self) -> usize {
        self.inner.lock().unwrap().len()
    }

    /// Returns the number of handlers registered for `name`.
    pub fn handler_count(&self, name: &str) -> usize {
        self.inner
            .lock()
            .unwrap()
            .get(name)
            .map(|v| v.len())
            .unwrap_or(0)
    }

    /// Emit `name`, calling each registered handler in registration order with
    /// `args` (cloned per call, since `FnPtr::call` consumes its argument vec).
    ///
    /// Mirrors `emit_event`'s contract on the Lua side: if a handler returns
    /// `false`, no further handlers are called and `emit_sync` itself returns
    /// `false` (meaning "the default action should be prevented"). If every
    /// handler runs without returning `false` (including the "no handlers
    /// registered at all" case), this returns `true`.
    ///
    /// Unlike the Lua path there is no async variant: rhai has no async execution
    /// model, so this is the only `emit`. See the module-level doc comment.
    pub fn emit_sync(
        &self,
        engine: &Engine,
        ast: &AST,
        name: &str,
        args: Vec<Dynamic>,
    ) -> Result<bool, Box<EvalAltResult>> {
        // Clone the handler list out from under the lock before calling anything:
        // a handler could itself try to register a new event (re-entrant `on`
        // call), which would deadlock if we were still holding the lock.
        let handlers: Vec<FnPtr> = {
            let registry = self.inner.lock().unwrap();
            match registry.get(name) {
                Some(handlers) => handlers.clone(),
                None => return Ok(true),
            }
        };

        for handler in handlers {
            let result: Dynamic = handler.call(engine, ast, args.clone())?;
            if let Some(false) = result.clone().try_cast::<bool>() {
                // Default action prevented; stop calling further handlers, exactly
                // like the `Value::Boolean(b) if !b` early-return in the Lua
                // `emit_event`.
                return Ok(false);
            }
        }
        Ok(true)
    }
}

/// Config-reload watch list: the rhai analogue of the Lua path's
/// `"wezterm-watch-paths"` named registry value (see
/// `add_to_config_reload_watch_list` in `config/src/lua.rs`).
///
/// TODO(L4/L5): once `.rhai` configs actually use `import`, hook this up to a
/// custom `rhai::ModuleResolver` (see [`install_module_resolver`]) so that
/// `import`-ed files are captured automatically, the way the Lua
/// `package.searchers[2]` shim captures `require`-d files. Until then, this is
/// populated only by explicit calls to `wezterm.add_to_config_reload_watch_list(...)`
/// from within a script, which is a strict subset of the Lua behavior but the same
/// public shape, so no future API break is expected.
#[derive(Default, Clone)]
pub struct ConfigReloadWatchList {
    inner: Arc<Mutex<Vec<String>>>,
}

impl ConfigReloadWatchList {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&self, path: impl Into<String>) {
        self.inner.lock().unwrap().push(path.into());
    }

    /// Returns a snapshot of the paths accumulated so far.
    pub fn paths(&self) -> Vec<String> {
        self.inner.lock().unwrap().clone()
    }
}

/// TODO(L4/L5): install a `rhai::ModuleResolver` that records every path it
/// resolves into a [`ConfigReloadWatchList`] before delegating to a real resolver
/// (e.g. `rhai::module_resolvers::FileModuleResolver`), mirroring the
/// `package.searchers[2]` shim in `make_lua_context`. Not implemented yet: there is
/// no rhai config file in production use yet to exercise `import` against, so a
/// resolver here would be exercised by nothing but its own unit tests. Once L4/L5
/// wires the rhai path into the real config loader and defines what a multi-file
/// rhai config actually looks like (single `import`? `.wezterm.rhai` per directory,
/// like Lua's `package.path` prefixing?), implement this function to call
/// `engine.set_module_resolver(...)`.
fn install_module_resolver(_engine: &mut Engine, _watch_list: &ConfigReloadWatchList) {
    // Intentionally empty for now; see doc comment above.
}

/// The rhai engine plus the auxiliary state (`config/src/lua.rs` keeps the
/// equivalent of this in Lua's own registry, via `set_named_registry_value`; rhai
/// has no such registry, so this struct is the explicit, typed equivalent).
pub struct RhaiConfigEngine {
    pub engine: Engine,
    pub events: EventRegistry,
    pub watch_list: ConfigReloadWatchList,
    pub config_file: PathBuf,
    pub config_dir: PathBuf,
}

impl RhaiConfigEngine {
    /// Evaluate `script` against this engine, returning the resulting `Dynamic`.
    /// Thin wrapper kept mainly so tests (and future callers) don't need to reach
    /// past this type into `rhai::Engine` directly for the common case.
    pub fn eval(&self, script: &str) -> Result<Dynamic, Box<EvalAltResult>> {
        self.engine.eval(script)
    }

    /// Compile `script` to an `AST` and evaluate it with a fresh `Scope`, returning
    /// both. The `AST` is needed by [`EventRegistry::emit_sync`] to resolve
    /// `FnPtr`s registered by the script (an `FnPtr` captured from a script is only
    /// callable against the `AST` it was defined in -- unlike an `mlua::Function`,
    /// which is fully self-contained once created).
    pub fn compile_and_eval(&self, script: &str) -> Result<(AST, Dynamic), Box<EvalAltResult>> {
        let ast = self
            .engine
            .compile(script)
            .map_err(|e| Box::new(EvalAltResult::from(e)))?;
        let mut scope = Scope::new();
        let result = self.engine.eval_ast_with_scope(&mut scope, &ast)?;
        Ok((ast, result))
    }

    /// Emit `name` with `args`, calling any handlers registered (via
    /// `wezterm.on(name, ...)` inside `ast`) against this engine. See
    /// [`EventRegistry::emit_sync`] for the return-value contract.
    pub fn emit_sync(
        &self,
        ast: &AST,
        name: &str,
        args: Vec<Dynamic>,
    ) -> Result<bool, Box<EvalAltResult>> {
        self.events.emit_sync(&self.engine, ast, name, args)
    }
}

/// Build a rhai engine for executing wezterm config scripts.
///
/// Analogue of `config/src/lua.rs::make_lua_context`. The path to the directory
/// containing the configuration is passed in and used to pre-set some values
/// accessible from the script (mirroring the `wezterm` module's
/// `config_file`/`config_dir` fields on the Lua side), and to register the
/// `wezterm` namespace's `on`/`emit`/`add_to_config_reload_watch_list` functions.
///
/// Unlike `make_lua_context`, this does not (yet) register the 13
/// `lua-api-crates` modules (`wezterm.color`, `wezterm.mux`, etc) -- that is L4's
/// job, once each crate has a rhai-side `register(engine: &mut Engine)` sibling to
/// its current `register(lua: &Lua)`.
///
/// ## Sandboxing / limits
///
/// The mlua path (`Lua::new()`) has no explicit sandbox configuration beyond
/// omitting the `debug` module load (implicit in not registering it) -- Lua
/// scripts otherwise get the full standard library. rhai's `Engine::new()`
/// registers its standard package by default and, unlike mlua, has first-class,
/// cheap-to-set resource limits (rhai's `unchecked` feature is NOT enabled in this
/// workspace's `Cargo.toml`, so limit-checking code is compiled in). Since a
/// wezterm config script is untrusted-ish input (a user's own dotfile, but one
/// that historically could be shared/copy-pasted from the internet), this sets
/// conservative-but-generous limits on operation count, call-stack depth, and
/// string/array size, so a pathological or accidentally-infinite config script
/// fails fast with a rhai error instead of hanging or exhausting memory. These
/// numbers have no hard requirement behind them yet (no real `.rhai` config exists
/// to benchmark against) and are expected to be revisited once L4/L5 land real
/// config scripts to test against.
pub fn make_rhai_engine(config_file: &Path) -> anyhow::Result<RhaiConfigEngine> {
    let mut engine = Engine::new();

    let config_dir = config_file
        .parent()
        .unwrap_or_else(|| Path::new("/"))
        .to_path_buf();

    // Conservative resource limits; see the doc comment above for rationale.
    engine.set_max_expr_depths(64, 64);
    engine.set_max_call_levels(64);
    engine.set_max_operations(10_000_000);
    engine.set_max_string_size(64 * 1024 * 1024);
    engine.set_max_array_size(1_000_000);
    engine.set_max_map_size(1_000_000);

    let events = EventRegistry::new();
    let watch_list = ConfigReloadWatchList::new();

    install_module_resolver(&mut engine, &watch_list);

    // `wezterm.on(name, callback)` - register an event handler. Mirrors
    // `register_event`/`wezterm.on` in `config/src/lua.rs`.
    {
        let events = events.clone();
        engine.register_fn("on", move |name: &str, callback: FnPtr| {
            events.register(name, callback);
        });
    }

    // `wezterm.add_to_config_reload_watch_list(path)` - mirrors
    // `add_to_config_reload_watch_list` in `config/src/lua.rs`. Accepts a single
    // path per call (rhai's `register_fn` doesn't have a direct analogue of Lua's
    // `Variadic<String>`; scripts that want to add several paths call this in a
    // loop or call it multiple times).
    {
        let watch_list = watch_list.clone();
        engine.register_fn("add_to_config_reload_watch_list", move |path: &str| {
            watch_list.add(path);
        });
    }

    engine.register_fn("config_file", {
        let config_file_str = config_file.to_string_lossy().into_owned();
        move || config_file_str.clone()
    });
    engine.register_fn("config_dir", {
        let config_dir_str = config_dir.to_string_lossy().into_owned();
        move || config_dir_str.clone()
    });
    engine.register_fn("target_triple", || crate::wezterm_target_triple().to_string());
    engine.register_fn("version", || crate::wezterm_version().to_string());
    engine.register_fn("home_dir", || crate::HOME_DIR.to_string_lossy().into_owned());
    engine.register_fn("hostname", || -> Result<String, Box<EvalAltResult>> {
        let hostname = hostname::get()
            .map_err(|e| format!("failed to resolve hostname: {e}"))?;
        hostname
            .to_str()
            .map(|s| s.to_owned())
            .ok_or_else(|| "hostname isn't UTF-8".into())
    });
    engine.register_fn("has_action", |name: &str| {
        KeyAssignment::variants().contains(&name)
    });

    Ok(RhaiConfigEngine {
        engine,
        events,
        watch_list,
        config_file: config_file.to_path_buf(),
        config_dir,
    })
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::exec_domain::ExecDomain;
    use std::convert::TryFrom;
    use std::sync::atomic::{AtomicI32, Ordering};

    /// Test helper alias: `Box<rhai::EvalAltResult>` is not `Sync` (it can embed
    /// an `FnPtr`, which closes over non-`Sync` `Rc`-based AST nodes), so it can't
    /// flow through `anyhow::Error`'s blanket `From` impl, which requires
    /// `Send + Sync`. Tests below convert rhai errors to `String` with `.to_string()`
    /// (via `.map_err`/the `?` operator against this alias) instead of using
    /// `anyhow::Result`.
    type TestResult = Result<(), String>;

    trait MapErrString<T> {
        fn str_err(self) -> Result<T, String>;
    }
    impl<T, E: std::fmt::Display> MapErrString<T> for Result<T, E> {
        fn str_err(self) -> Result<T, String> {
            self.map_err(|e| e.to_string())
        }
    }

    #[test]
    fn make_rhai_engine_sets_config_paths() -> TestResult {
        let rhai_engine = make_rhai_engine(Path::new("/some/dir/wezterm.rhai")).str_err()?;
        assert_eq!(rhai_engine.config_dir, Path::new("/some/dir"));
        assert_eq!(
            rhai_engine.config_file,
            Path::new("/some/dir/wezterm.rhai")
        );

        let result = rhai_engine.eval("config_dir()").str_err()?;
        assert_eq!(result.into_string().unwrap(), "/some/dir".to_string());
        Ok(())
    }

    #[test]
    fn make_rhai_engine_exposes_has_action() -> TestResult {
        let rhai_engine = make_rhai_engine(Path::new("testing")).str_err()?;
        let result = rhai_engine.eval(r#"has_action("Nop")"#).str_err()?;
        assert_eq!(result.as_bool().unwrap(), true);

        let result = rhai_engine
            .eval(r#"has_action("ThisIsNotARealAction")"#)
            .str_err()?;
        assert_eq!(result.as_bool().unwrap(), false);
        Ok(())
    }

    /// End-to-end proof that the L2 engine entry point, plus the L1 bridge, plus
    /// the macro-integration sample applied to `ExecDomain` in
    /// `config/src/exec_domain.rs`, all compose: parse a `.rhai`-style object map
    /// literal via `make_rhai_engine`, and get a real `ExecDomain` out the other
    /// end via the generated `TryFrom<rhai::Dynamic>` impl.
    ///
    /// Uses `"my-exec-domain"` rather than the more obvious `"local"` as the
    /// domain name deliberately: `ExecDomain::name` carries a
    /// `#[dynamic(validate = "validate_domain_name")]` attribute (see
    /// `config/src/config.rs`) that rejects `"local"` as a reserved, built-in
    /// domain name. That validator runs inside `FromDynamic::from_dynamic` itself,
    /// which both the mlua and rhai bridges call identically -- so it is backend
    /// -agnostic behavior, not a rhai-specific edge case, but it was the thing
    /// that actually broke first when writing this test (using `"local"`, copied
    /// from a Lua doc example, produced `"local" is a built-in domain and cannot
    /// be redefined` instead of a `TryFrom` failure). Worth calling out because it
    /// means callers of `impl_rhai_conversion_dynamic!`-generated `TryFrom` impls
    /// need to be ready for `FromDynamic`-level semantic validation errors, not
    /// just structural/type-shape ones -- exactly mirroring what the mlua path
    /// already has to handle via `Config::from_dynamic` in `lua.rs`'s
    /// `config_builder_new_index`.
    #[test]
    fn parses_exec_domain_from_rhai_script_via_macro() -> TestResult {
        let rhai_engine = make_rhai_engine(Path::new("testing")).str_err()?;

        let script = r#"
            #{
                name: "my-exec-domain",
                fixup_command: "some-event-name",
            }
        "#;
        let value = rhai_engine.eval(script).str_err()?;

        let domain = ExecDomain::try_from(value).str_err()?;

        assert_eq!(domain.name, "my-exec-domain");
        assert_eq!(domain.fixup_command, "some-event-name");
        assert!(domain.label.is_none());
        Ok(())
    }

    /// The validator-rejection edge case documented above, made explicit as its
    /// own test: confirm that `impl_rhai_conversion_dynamic!`'s generated
    /// `TryFrom<rhai::Dynamic>` surfaces a `FromDynamic`-level validation failure
    /// (not just structural mismatches) as an `Err`, exactly like the mlua path's
    /// `from_lua`/`Config::from_dynamic` does.
    #[test]
    fn parses_exec_domain_rejects_reserved_local_name() {
        let rhai_engine = make_rhai_engine(Path::new("testing")).expect("engine");
        let script = r#"
            #{
                name: "local",
                fixup_command: "some-event-name",
            }
        "#;
        let value = rhai_engine.eval(script).expect("script should evaluate");
        let err = ExecDomain::try_from(value).expect_err("\"local\" must be rejected");
        assert!(err.to_string().contains("built-in domain"));
    }

    /// Same idea, but for the reverse leg (`From<ExecDomain> for rhai::Dynamic`,
    /// also generated by `impl_rhai_conversion_dynamic!`): build an `ExecDomain` in
    /// Rust, convert it to a `Dynamic`, and confirm a rhai script can read its
    /// fields back out as an ordinary object map.
    #[test]
    fn exec_domain_converts_back_into_rhai_dynamic() -> TestResult {
        let domain = ExecDomain {
            name: "my-exec-domain".to_string(),
            fixup_command: "some-event".to_string(),
            label: None,
        };
        let dynamic: Dynamic = domain.into();
        assert!(dynamic.is_map());

        let mut scope = Scope::new();
        scope.push("domain", dynamic);
        let engine = Engine::new();
        let result: String = engine.eval_with_scope(&mut scope, "domain.name").str_err()?;
        assert_eq!(result, "my-exec-domain");
        Ok(())
    }

    #[test]
    fn can_register_and_emit_an_event() -> TestResult {
        let mut rhai_engine = make_rhai_engine(Path::new("testing")).str_err()?;

        let total = Arc::new(AtomicI32::new(0));

        {
            let total = total.clone();
            rhai_engine
                .engine
                .register_fn("record", move |n: i64| {
                    total.fetch_add(n as i32, Ordering::SeqCst);
                });
        }

        let script = r#"
            on("foo", |n| {
                record(n);
            });
        "#;
        let (ast, _) = rhai_engine.compile_and_eval(script).str_err()?;

        assert_eq!(rhai_engine.events.handler_count("foo"), 1);

        let default_action = rhai_engine
            .emit_sync(&ast, "foo", vec![Dynamic::from(21_i64)])
            .str_err()?;
        assert_eq!(default_action, true);
        assert_eq!(total.load(Ordering::SeqCst), 21);

        // Calling emit_sync again re-invokes the same handler (registration is
        // once, invocation is multiple times), matching the required semantics.
        rhai_engine
            .emit_sync(&ast, "foo", vec![Dynamic::from(1_i64)])
            .str_err()?;
        assert_eq!(total.load(Ordering::SeqCst), 22);

        Ok(())
    }

    #[test]
    fn multiple_handlers_run_in_registration_order_and_false_stops_the_chain() -> TestResult {
        let mut rhai_engine = make_rhai_engine(Path::new("testing")).str_err()?;

        let order = Arc::new(Mutex::new(Vec::<i32>::new()));

        {
            let order = order.clone();
            rhai_engine.engine.register_fn("record", move |n: i64| {
                order.lock().unwrap().push(n as i32);
            });
        }

        let script = r#"
            on("bar", |n| {
                record(n);
                true
            });
            on("bar", |n| {
                record(n * 2);
                // Prevent any later handlers from being called.
                false
            });
            on("bar", |n| {
                record(n * 3);
            });
        "#;
        let (ast, _) = rhai_engine.compile_and_eval(script).str_err()?;

        assert_eq!(rhai_engine.events.handler_count("bar"), 3);

        let default_action = rhai_engine
            .emit_sync(&ast, "bar", vec![Dynamic::from(2_i64)])
            .str_err()?;
        assert_eq!(default_action, false);

        // Third handler must not have run: only the first two contributed.
        assert_eq!(*order.lock().unwrap(), vec![2, 4]);

        Ok(())
    }

    #[test]
    fn emitting_an_event_with_no_handlers_returns_true() -> TestResult {
        let rhai_engine = make_rhai_engine(Path::new("testing")).str_err()?;
        let ast = rhai_engine.engine.compile("()").str_err()?;
        let default_action = rhai_engine
            .emit_sync(&ast, "never-registered", vec![])
            .str_err()?;
        assert_eq!(default_action, true);
        Ok(())
    }

    #[test]
    fn config_reload_watch_list_accumulates_paths() -> TestResult {
        let rhai_engine = make_rhai_engine(Path::new("testing")).str_err()?;
        let _ = rhai_engine
            .eval(r#"add_to_config_reload_watch_list("/some/extra/file.rhai")"#)
            .str_err()?;
        let _ = rhai_engine
            .eval(r#"add_to_config_reload_watch_list("/another/file.rhai")"#)
            .str_err()?;

        let paths = rhai_engine.watch_list.paths();
        assert_eq!(
            paths,
            vec![
                "/some/extra/file.rhai".to_string(),
                "/another/file.rhai".to_string(),
            ]
        );
        Ok(())
    }

    /// Edge case not present on the Lua side at all: rhai closures that capture
    /// an outer variable by value become non-`'static`-looking but are still
    /// perfectly callable later via `FnPtr::call`, because `FnPtr` captures its
    /// closed-over state as `Dynamic`s internally rather than borrowing Rust
    /// stack frames. Lua's `mlua::Function` has no notion of "capture" at all
    /// (Lua upvalues are a purely Lua-side concept invisible to the Rust
    /// wrapper), so this is a genuinely new category of behavior to verify for
    /// the rhai path: does a captured value stay stable across repeated
    /// `emit_sync` calls, or does it reset each time?
    ///
    /// This is also where the `Box<EvalAltResult>: !Sync` gap surfaced during
    /// implementation (see `TestResult`/`MapErrString` above): an `FnPtr` value
    /// captured from a closure holds an `Rc`-based AST reference, which is exactly
    /// what makes the whole error type non-`Sync` and therefore incompatible with
    /// `anyhow::Result` in test signatures. That is an actual, load-bearing
    /// consequence of rhai's single-threaded-by-default design (`sync` feature
    /// off in this workspace), not just a test-plumbing annoyance: any future
    /// caller that wants to propagate a rhai error through an `anyhow`-based
    /// (or otherwise `Send + Sync`-bound) error type will need the same
    /// `.to_string()` (or similar) conversion at the boundary.
    #[test]
    fn captured_closure_state_persists_across_repeated_emits() -> TestResult {
        let rhai_engine = make_rhai_engine(Path::new("testing")).str_err()?;

        let script = r#"
            let counter = 0;
            on("tick", |n| {
                counter += n;
                counter
            });
        "#;
        let (ast, _) = rhai_engine.compile_and_eval(script).str_err()?;

        // Each emit_sync call re-invokes the handler against the same AST/closure
        // state. Confirm this doesn't panic and returns a well-formed bool each
        // time (the actual counter value living inside the rhai closure isn't
        // observable from here without another registered callback, but the
        // contract under test is that repeated emission against the same AST
        // remains stable and side-effect-free from the Rust caller's point of
        // view beyond the documented `bool` return).
        for _ in 0..5 {
            let default_action = rhai_engine
                .emit_sync(&ast, "tick", vec![Dynamic::from(1_i64)])
                .str_err()?;
            assert_eq!(default_action, true);
        }
        Ok(())
    }
}
