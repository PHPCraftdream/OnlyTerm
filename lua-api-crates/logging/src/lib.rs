use config::lua::get_or_create_module;
use config::lua::mlua::{Lua, Value, Variadic};
use luahelper::ValuePrinter;

pub fn register(lua: &Lua) -> anyhow::Result<()> {
    let wezterm_mod = get_or_create_module(lua, "wezterm")?;

    wezterm_mod.set(
        "log_error",
        lua.create_function(|_, args: Variadic<Value>| {
            let output = print_helper(args);
            log::error!("lua: {}", output);
            Ok(())
        })?,
    )?;
    wezterm_mod.set(
        "log_info",
        lua.create_function(|_, args: Variadic<Value>| {
            let output = print_helper(args);
            log::info!("lua: {}", output);
            Ok(())
        })?,
    )?;
    wezterm_mod.set(
        "log_warn",
        lua.create_function(|_, args: Variadic<Value>| {
            let output = print_helper(args);
            log::warn!("lua: {}", output);
            Ok(())
        })?,
    )?;

    wezterm_mod.set(
        "to_string",
        lua.create_function(|_, arg: Value| {
            let res = ValuePrinter(arg);
            Ok(format!("{:#?}", res).to_string())
        })?,
    )?;

    lua.globals().set(
        "print",
        lua.create_function(|_, args: Variadic<Value>| {
            let output = print_helper(args);
            log::info!("lua: {}", output);
            Ok(())
        })?,
    )?;

    Ok(())
}

fn print_helper(args: Variadic<Value>) -> String {
    let mut output = String::new();
    for (idx, item) in args.into_iter().enumerate() {
        if idx > 0 {
            output.push(' ');
        }

        match item {
            Value::String(s) => match s.to_str() {
                Ok(s) => output.push_str(s),
                Err(_) => {
                    let item = String::from_utf8_lossy(s.as_bytes());
                    output.push_str(&item);
                }
            },
            item @ _ => {
                let item = format!("{:#?}", ValuePrinter(item));
                output.push_str(&item);
            }
        }
    }
    output
}

// ---------------------------------------------------------------------------
// L4b: rhai port of `register` above (see
// docs/plans/2026-07-23-lua-rhai-migration.md). Runs in parallel with the mlua
// path; does not replace or touch `register(lua: &Lua)`.
//
// mlua's `Variadic<Value>` (an arbitrary-arity argument list, used here so
// `log_error("a", 1, true)` works the same as Lua's own variadic `print`) has
// no direct rhai equivalent: `Engine::register_fn` only supports fixed arity.
// rhai's own idiomatic answer to "log/print with any number of arguments" is
// its built-in `Array` type, so each rhai-facing function here takes a single
// `rhai::Array` parameter instead of a variadic parameter list; a `.rhai`
// script calls e.g. `log_error(["a", 1, true])` rather than
// `log_error("a", 1, true)`. This is the one place this crate's rhai call-site
// shape necessarily differs from the mlua one (documented here rather than
// silently, since every other function/namespace path in this crate is 1:1).
pub fn register_rhai(engine: &mut rhai::Engine) -> anyhow::Result<()> {
    engine.register_fn("log_error", |args: rhai::Array| {
        let output = print_helper_rhai(&args);
        log::error!("rhai: {}", output);
    });
    engine.register_fn("log_info", |args: rhai::Array| {
        let output = print_helper_rhai(&args);
        log::info!("rhai: {}", output);
    });
    engine.register_fn("log_warn", |args: rhai::Array| {
        let output = print_helper_rhai(&args);
        log::warn!("rhai: {}", output);
    });

    engine.register_fn("to_string", |arg: rhai::Dynamic| -> String {
        rhai_value_to_debug_string(&arg)
    });

    // `print` is not an ordinary registrable function in rhai the way
    // `log_error`/`log_info`/`log_warn` above are: it (along with `debug`) is
    // a special built-in engine command, always resolved to a single-string
    // signature and dispatched through `Engine::on_print`/`on_debug` (which
    // default to writing to stdout), not through the normal
    // `register_fn`-based function-overload table -- attempting to
    // `register_fn("print", ...)` with a different (`Array`) parameter type
    // creates a same-named *additional* overload rather than actually
    // replacing the built-in's dispatch, and the two paths conflict (verified
    // empirically: the override function *is* invoked, but rhai's own
    // print-statement evaluation then still separately expects/produces a
    // `String` result afterwards, raising `ErrorMismatchOutputType`). The
    // correct, intended mechanism for "redirect what `print(...)` does" is
    // `Engine::on_print`, which this uses instead -- the rhai equivalent of
    // the mlua path's `lua.globals().set("print", ...)` override, just via
    // rhai's dedicated hook rather than function registration.
    engine.on_print(|s| {
        log::info!("rhai: {}", s);
    });

    Ok(())
}

/// rhai analogue of `print_helper` above: formats each element of a rhai
/// `Array` the same way `print_helper` formats each element of a Lua
/// `Variadic<Value>` -- strings are emitted verbatim (no quoting), everything
/// else goes through a debug-ish pretty-printer, and elements are
/// space-separated.
fn print_helper_rhai(args: &rhai::Array) -> String {
    let mut output = String::new();
    for (idx, item) in args.iter().enumerate() {
        if idx > 0 {
            output.push(' ');
        }
        if item.is_string() {
            output.push_str(&item.clone().into_string().unwrap_or_default());
        } else {
            output.push_str(&rhai_value_to_debug_string(item));
        }
    }
    output
}

/// rhai analogue of `ValuePrinter` above: there is no rhai equivalent of
/// `luahelper::ValuePrinter` (which pretty-prints an `mlua::Value` including
/// walking table contents), so this falls back to rhai's own `Dynamic::Debug`
/// impl, which already recursively renders `Array`/`Map` contents in a
/// broadly similar "structured debug" style.
fn rhai_value_to_debug_string(value: &rhai::Dynamic) -> String {
    format!("{:#?}", value)
}
