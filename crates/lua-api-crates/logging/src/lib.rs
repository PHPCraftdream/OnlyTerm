/// Rhai-side registration of logging functions (see
/// `docs/plans/2026-07-23-lua-rhai-migration.md`).
///
/// rhai's `Array` type is used for the arbitrary-arity argument list, so a
/// `.rhai` script calls e.g. `log_error(["a", 1, true])`.
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

    // `print` is a special built-in engine command dispatched through
    // `Engine::on_print`; the correct mechanism for redirecting it is
    // `Engine::on_print` rather than function registration.
    engine.on_print(|s| {
        log::info!("rhai: {}", s);
    });

    Ok(())
}

/// Formats each element of a rhai `Array` -- strings verbatim, everything else
/// through a debug-ish pretty-printer, space-separated.
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

/// Falls back to rhai's own `Dynamic::Debug` impl, which recursively renders
/// `Array`/`Map` contents.
fn rhai_value_to_debug_string(value: &rhai::Dynamic) -> String {
    format!("{:#?}", value)
}
