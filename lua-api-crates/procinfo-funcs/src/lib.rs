use config::lua::get_or_create_sub_module;
use config::lua::mlua::Lua;
use procinfo::LocalProcessInfo;
use wezterm_dynamic::ToDynamic;

pub fn register(lua: &Lua) -> anyhow::Result<()> {
    let proc_mod = get_or_create_sub_module(lua, "procinfo")?;
    proc_mod.set(
        "pid",
        lua.create_function(|_, _: ()| Ok(unsafe { libc::getpid() }))?,
    )?;
    proc_mod.set(
        "get_info_for_pid",
        lua.create_function(|_, pid: u32| Ok(LocalProcessInfo::with_root_pid(pid)))?,
    )?;
    proc_mod.set(
        "current_working_dir_for_pid",
        lua.create_function(|_, pid: u32| {
            Ok(LocalProcessInfo::current_working_dir(pid)
                .and_then(|p| p.to_str().map(|s| s.to_string())))
        })?,
    )?;
    proc_mod.set(
        "executable_path_for_pid",
        lua.create_function(|_, pid: u32| {
            Ok(LocalProcessInfo::executable_path(pid)
                .and_then(|p| p.to_str().map(|s| s.to_string())))
        })?,
    )?;
    Ok(())
}

// `LocalProcessInfo` already derives `FromDynamic`/`ToDynamic` (gated behind
// procinfo's `lua` feature, which is also its `default` feature -- see
// `procinfo/src/lib.rs` -- so it's active here without any extra feature wiring
// on this crate's own `procinfo.workspace = true` dependency declaration).
// `impl_lua_conversion_dynamic!` already exists on the mlua path (also in
// `procinfo/src/lib.rs`).
//
// Unlike `battery`'s `BatteryInfo` (a struct defined *in* the battery crate,
// so `impl_rhai_conversion_dynamic!(BatteryInfo)` there is a normal local
// trait impl), `LocalProcessInfo` is a foreign type defined in the `procinfo`
// crate, and the macro's generated impls (`TryFrom<rhai::Dynamic> for
// LocalProcessInfo` / `From<LocalProcessInfo> for rhai::Dynamic`) would both
// be foreign-type-for-foreign-trait impls here -- a orphan-rule violation
// (E0117) neither this crate nor `procinfo` itself owns `rhai::Dynamic` or
// (from here) `LocalProcessInfo`. So instead of the macro, this calls the same
// underlying free function the macro expands to
// (`config::rhai_value::dynamic_to_rhai_dynamic`) directly on the `ToDynamic`
// output, which needs no new trait impl and is exactly as total/lossless.
fn local_process_info_to_rhai(info: &LocalProcessInfo) -> rhai::Dynamic {
    config::rhai_value::dynamic_to_rhai_dynamic(&info.to_dynamic())
}

// ---------------------------------------------------------------------------
// L4b: rhai port of `register` above (see
// docs/plans/2026-07-23-lua-rhai-migration.md). Runs in parallel with the mlua
// path; does not replace or touch `register(lua: &Lua)`.
//
// All four functions are registered under a `procinfo` static module, the
// same call-site shape (`procinfo::pid()`, `procinfo::get_info_for_pid(pid)`,
// etc) as the mlua path's `wezterm.procinfo.*` sub-module.
// ---------------------------------------------------------------------------
pub fn register_rhai(engine: &mut rhai::Engine) -> anyhow::Result<()> {
    let mut proc_module = rhai::Module::new();

    proc_module.set_native_fn("pid", || Ok(unsafe { libc::getpid() } as rhai::INT));

    proc_module.set_native_fn(
        "get_info_for_pid",
        |pid: rhai::INT| -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
            let pid = pid_from_rhai_int(pid)?;
            match LocalProcessInfo::with_root_pid(pid) {
                Some(info) => Ok(local_process_info_to_rhai(&info)),
                // mlua's `Option<LocalProcessInfo>` becomes Lua `nil` when
                // `None`; the rhai equivalent of "no meaningful value" is `()`
                // (unit), which is what `Dynamic::UNIT` represents.
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

/// rhai has no unsigned integer type (`INT` is `i64`, or `i32` under
/// `only_i32`), unlike the mlua path's plain `u32` parameter, so each rhai
/// binding above takes `rhai::INT` and narrows it here, rejecting
/// out-of-range/negative pids with a proper rhai error instead of silently
/// truncating (which `as u32` would do for a negative `INT`).
fn pid_from_rhai_int(pid: rhai::INT) -> Result<u32, Box<rhai::EvalAltResult>> {
    u32::try_from(pid).map_err(|_| -> Box<rhai::EvalAltResult> {
        format!("pid {pid} is out of range for a process id (expected a non-negative u32)").into()
    })
}
