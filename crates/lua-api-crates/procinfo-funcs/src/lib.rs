use procinfo::LocalProcessInfo;
use wezterm_dynamic::ToDynamic;

// `LocalProcessInfo` already derives `FromDynamic`/`ToDynamic` (gated behind
// procinfo's `lua` feature, which is also its `default` feature -- see
// `procinfo/src/lib.rs` -- so it's active here without any extra feature wiring
// on this crate's own `procinfo.workspace = true` dependency declaration).

/// Unlike a struct defined in this crate, `LocalProcessInfo` is a foreign type
/// defined in the `procinfo` crate, so the `impl_rhai_conversion_dynamic!`
/// macro's generated impls would be foreign-type-for-foreign-trait impls -- an
/// orphan-rule violation. Instead this calls the same underlying free function
/// the macro expands to (`config::rhai_value::dynamic_to_rhai_dynamic`)
/// directly on the `ToDynamic` output.
fn local_process_info_to_rhai(info: &LocalProcessInfo) -> rhai::Dynamic {
    config::rhai_value::dynamic_to_rhai_dynamic(&info.to_dynamic())
}

// ---------------------------------------------------------------------------
// Rhai port of the former mlua `register`. All four functions are registered
// under a `procinfo` static module.
// ---------------------------------------------------------------------------
pub fn register_rhai(engine: &mut rhai::Engine) -> anyhow::Result<()> {
    let mut proc_module = rhai::Module::new();

    proc_module.set_native_fn("pid", || Ok(std::process::id() as rhai::INT));

    proc_module.set_native_fn(
        "get_info_for_pid",
        |pid: rhai::INT| -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
            let pid = pid_from_rhai_int(pid)?;
            match LocalProcessInfo::with_root_pid(pid) {
                Some(info) => Ok(local_process_info_to_rhai(&info)),
                // the rhai equivalent of "no meaningful value" is `()`
                None => Ok(rhai::Dynamic::UNIT),
            }
        },
    );

    proc_module.set_native_fn(
        "current_working_dir_for_pid",
        |pid: rhai::INT| -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
            let pid = pid_from_rhai_int(pid)?;
            Ok(
                match LocalProcessInfo::current_working_dir(pid)
                    .and_then(|p| p.to_str().map(|s| s.to_string()))
                {
                    Some(s) => rhai::Dynamic::from(s),
                    None => rhai::Dynamic::UNIT,
                },
            )
        },
    );

    proc_module.set_native_fn(
        "executable_path_for_pid",
        |pid: rhai::INT| -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
            let pid = pid_from_rhai_int(pid)?;
            Ok(
                match LocalProcessInfo::executable_path(pid)
                    .and_then(|p| p.to_str().map(|s| s.to_string()))
                {
                    Some(s) => rhai::Dynamic::from(s),
                    None => rhai::Dynamic::UNIT,
                },
            )
        },
    );

    engine.register_static_module("procinfo", proc_module.into());

    Ok(())
}

/// rhai has no unsigned integer type (`INT` is `i64`), so each rhai binding
/// above takes `rhai::INT` and narrows it here, rejecting out-of-range/negative
/// pids with a proper rhai error instead of silently truncating.
fn pid_from_rhai_int(pid: rhai::INT) -> Result<u32, Box<rhai::EvalAltResult>> {
    u32::try_from(pid).map_err(|_| -> Box<rhai::EvalAltResult> {
        format!("pid {pid} is out of range for a process id (expected a non-negative u32)").into()
    })
}
