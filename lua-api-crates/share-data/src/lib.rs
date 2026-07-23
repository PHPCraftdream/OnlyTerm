use config::lua::get_or_create_module;
use config::lua::mlua::{
    self, IntoLua, Lua, UserData, UserDataMethods, UserDataRef, Value as LuaValue,
};
use ordered_float::OrderedFloat;
use std::cmp::Ordering;
use std::collections::{BTreeMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
struct Object {
    inner: Arc<Mutex<BTreeMap<String, Value>>>,
}

impl Ord for Object {
    fn cmp(&self, other: &Self) -> Ordering {
        let self_ptr = self as *const Self;
        let other_ptr = other as *const Self;
        self_ptr.cmp(&other_ptr)
    }
}

impl PartialOrd for Object {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for Object {
    fn eq(&self, other: &Self) -> bool {
        let a = self.inner.lock().unwrap();
        let b = other.inner.lock().unwrap();
        *a == *b
    }
}

impl Eq for Object {}

impl Hash for Object {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.inner.lock().unwrap().hash(state)
    }
}

#[derive(Debug, Clone)]
struct Array {
    inner: Arc<Mutex<Vec<Value>>>,
}

impl Ord for Array {
    fn cmp(&self, other: &Self) -> Ordering {
        let self_ptr = self as *const Self;
        let other_ptr = other as *const Self;
        self_ptr.cmp(&other_ptr)
    }
}

impl PartialOrd for Array {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for Array {
    fn eq(&self, other: &Self) -> bool {
        let a = self.inner.lock().unwrap();
        let b = other.inner.lock().unwrap();
        *a == *b
    }
}

impl Eq for Array {}

impl Hash for Array {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.inner.lock().unwrap().hash(state)
    }
}

#[derive(Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Clone)]
enum Value {
    Null,
    Bool(bool),
    String(String),
    Array(Array),
    Object(Object),
    I64(i64),
    F64(OrderedFloat<f64>),
}

fn lua_value_to_gvalue(value: LuaValue) -> mlua::Result<Value> {
    let mut visited = HashSet::new();
    lua_value_to_gvalue_impl(value, &mut visited)
}

fn lua_value_to_gvalue_impl(value: LuaValue, visited: &mut HashSet<usize>) -> mlua::Result<Value> {
    if let LuaValue::Table(_) = &value {
        let ptr = value.to_pointer() as usize;
        if visited.contains(&ptr) {
            // Skip this one, as we've seen it before.
            // Treat it as a Null value.
            return Ok(Value::Null);
        }
        visited.insert(ptr);
    }
    Ok(match value {
        LuaValue::Nil => Value::Null,
        LuaValue::String(s) => Value::String(s.to_str()?.to_string()),
        LuaValue::Boolean(b) => Value::Bool(b),
        LuaValue::Integer(i) => Value::I64(i),
        LuaValue::Number(i) => Value::F64(i.into()),
        // Handle our special Null userdata case and map it to Null
        LuaValue::LightUserData(ud) if ud.0.is_null() => Value::Null,
        LuaValue::LightUserData(_) => {
            return Err(mlua::Error::FromLuaConversionError {
                from: "userdata",
                to: "Value",
                message: None,
            })
        }
        LuaValue::UserData(ud) => match ud.get_metatable() {
            Ok(mt) => {
                if let Ok(to_dynamic) = mt.get::<mlua::Function>("__wezterm_to_dynamic") {
                    match to_dynamic.call(LuaValue::UserData(ud.clone())) {
                        Ok(value) => {
                            return lua_value_to_gvalue_impl(value, visited);
                        }
                        Err(err) => {
                            return Err(mlua::Error::FromLuaConversionError {
                                from: "userdata",
                                to: "Value",
                                message: Some(format!(
                                    "error calling __wezterm_to_dynamic: {err:#}"
                                )),
                            })
                        }
                    }
                }

                match mt.get::<mlua::Function>(mlua::MetaMethod::ToString) {
                    Ok(to_string) => match to_string.call(LuaValue::UserData(ud.clone())) {
                        Ok(value) => {
                            return lua_value_to_gvalue_impl(value, visited);
                        }
                        Err(err) => {
                            return Err(mlua::Error::FromLuaConversionError {
                                from: "userdata",
                                to: "Value",
                                message: Some(format!("error calling tostring: {err:#}")),
                            })
                        }
                    },
                    Err(err) => {
                        return Err(mlua::Error::FromLuaConversionError {
                            from: "userdata",
                            to: "Value",
                            message: Some(format!("error getting tostring: {err:#}")),
                        })
                    }
                }
            }
            Err(err) => {
                return Err(mlua::Error::FromLuaConversionError {
                    from: "userdata",
                    to: "Value",
                    message: Some(format!("error getting metatable: {err:#}")),
                })
            }
        },
        LuaValue::Function(_) => {
            return Err(mlua::Error::FromLuaConversionError {
                from: "function",
                to: "Value",
                message: None,
            })
        }
        LuaValue::Thread(_) => {
            return Err(mlua::Error::FromLuaConversionError {
                from: "thread",
                to: "Value",
                message: None,
            })
        }
        LuaValue::Error(e) => return Err(e),
        LuaValue::Table(table) => {
            if let Ok(true) = table.contains_key(1) {
                let mut array = vec![];
                let pairs = table.clone();
                for value in table.sequence_values() {
                    array.push(lua_value_to_gvalue(value?)?);
                }

                for pair in pairs.pairs::<LuaValue, LuaValue>() {
                    let (key, _value) = pair?;
                    match &key {
                        LuaValue::Integer(n) if *n >= 1 && *n as usize <= array.len() => {
                            // Ok!
                        }
                        _ => {
                            let type_name = key.type_name();
                            return Err(mlua::Error::FromLuaConversionError {
                                from: type_name,
                                to: "numeric array index",
                                message: Some(format!(
                                    "Unexpected key {key:?} for array style table"
                                )),
                            });
                        }
                    }
                }

                Value::Array(Array {
                    inner: Arc::new(Mutex::new(array.into())),
                })
            } else {
                let mut obj = BTreeMap::default();
                for pair in table.pairs::<String, LuaValue>() {
                    let (key, value) = pair?;
                    let lua_type = value.type_name();
                    let value = lua_value_to_gvalue(value).map_err(|e| {
                        mlua::Error::FromLuaConversionError {
                            from: lua_type,
                            to: "value",
                            message: Some(format!("while processing {key:?}: {e}")),
                        }
                    })?;
                    obj.insert(key, value);
                }
                Value::Object(Object {
                    inner: Arc::new(Mutex::new(obj.into())),
                })
            }
        }
    })
}

lazy_static::lazy_static! {
    static ref GLOBALS: Value = Value::Object(Object{inner:Arc::new(Mutex::new(BTreeMap::new()))});
}

fn gvalue_to_lua<'lua>(lua: &'lua Lua, value: &Value) -> mlua::Result<LuaValue<'lua>> {
    match value {
        Value::Array(arr) => {
            let result = lua.create_table()?;
            let arr = arr.inner.lock().unwrap();
            for (idx, value) in arr.iter().enumerate() {
                result.set(idx + 1, gvalue_to_lua(lua, value)?)?;
            }
            Ok(LuaValue::Table(result))
        }
        Value::Object(obj) => {
            let result = lua.create_table()?;
            let obj = obj.inner.lock().unwrap();
            for (key, value) in obj.iter() {
                result.set(key.clone(), gvalue_to_lua(lua, value)?)?;
            }
            Ok(LuaValue::Table(result))
        }
        Value::Bool(b) => Ok(LuaValue::Boolean(*b)),
        Value::Null => Ok(LuaValue::Nil),
        Value::String(s) => s.to_string().into_lua(lua),
        Value::I64(i) => Ok(LuaValue::Integer(*i)),
        Value::F64(n) => n.into_lua(lua),
    }
}

impl UserData for Value {
    fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
        methods.add_meta_method(
            "__wezterm_to_dynamic",
            |lua: &Lua, this, _: ()| -> mlua::Result<mlua::Value> { gvalue_to_lua(lua, this) },
        );
        methods.add_meta_method(
            mlua::MetaMethod::Len,
            |lua: &Lua, this, _: ()| -> mlua::Result<mlua::Value> {
                match this {
                    Value::Array(arr) => arr.inner.lock().unwrap().len().into_lua(lua),
                    Value::Object(obj) => obj.inner.lock().unwrap().len().into_lua(lua),
                    Value::String(s) => s.to_string().into_lua(lua),
                    _ => Err(mlua::Error::external(
                        "invalid type for len operator".to_string(),
                    )),
                }
            },
        );

        methods.add_meta_method(mlua::MetaMethod::Pairs, |lua, this, ()| match this {
            Value::Array(_) => {
                let stateless_iter = lua.create_function(
                    |lua, (this, i): (UserDataRef<Value>, usize)| match &*this {
                        Value::Array(arr) => {
                            let arr = arr.inner.lock().unwrap();
                            let i = i + 1;

                            if i <= arr.len() {
                                return Ok(mlua::Variadic::from_iter(vec![
                                    i.into_lua(lua)?,
                                    arr[i - 1].clone().into_lua(lua)?,
                                ]));
                            }
                            return Ok(mlua::Variadic::new());
                        }
                        _ => unreachable!(),
                    },
                )?;
                Ok((stateless_iter, this.clone(), 0.into_lua(lua)?))
            }
            Value::Object(_) => {
                let stateless_iter = lua.create_function(
                    |lua, (this, key): (UserDataRef<Value>, Option<String>)| match &*this {
                        Value::Object(obj) => {
                            let obj = obj.inner.lock().unwrap();
                            let mut iter = obj.iter();

                            let mut this_is_key = false;

                            if key.is_none() {
                                this_is_key = true;
                            }

                            while let Some((this_key, value)) = iter.next() {
                                if this_is_key {
                                    return Ok(mlua::MultiValue::from_vec(vec![
                                        this_key.clone().into_lua(lua)?,
                                        value.clone().into_lua(lua)?,
                                    ]));
                                }
                                if Some(this_key.as_str()) == key.as_deref() {
                                    this_is_key = true;
                                }
                            }
                            return Ok(mlua::MultiValue::new());
                        }
                        _ => unreachable!(),
                    },
                )?;
                Ok((stateless_iter, this.clone(), LuaValue::Nil))
            }
            _ => Err(mlua::Error::external(
                "invalid type for __ipairs metamethod".to_string(),
            )),
        });

        methods.add_meta_method(
            mlua::MetaMethod::Index,
            |lua: &Lua, this, key: LuaValue| -> mlua::Result<mlua::Value> {
                match this {
                    Value::Array(arr) => match key {
                        LuaValue::Integer(i) => {
                            if i <= 0 {
                                return Err(mlua::Error::external(format!(
                                    "invalid array index {i}"
                                )));
                            }
                            // Convert lua 1-based indices to 0-based
                            let i = (i as usize) - 1;

                            let arr = arr.inner.lock().unwrap();
                            let value = match arr.get(i) {
                                None => return Ok(LuaValue::Nil),
                                Some(v) => v,
                            };

                            match value {
                                Value::Null => Ok(LuaValue::Nil),
                                Value::Bool(b) => Ok(LuaValue::Boolean(*b)),
                                Value::String(s) => s.clone().into_lua(lua),
                                Value::F64(u) => u.into_lua(lua),
                                Value::I64(u) => u.into_lua(lua),
                                Value::Array(_) => value.clone().into_lua(lua),
                                Value::Object(_) => value.clone().into_lua(lua),
                            }
                        }
                        _ => Err(mlua::Error::external(
                            "can only index arrays using integer values",
                        )),
                    },
                    Value::Object(obj) => match key {
                        LuaValue::String(s) => match s.to_str() {
                            Err(e) => Err(mlua::Error::external(format!(
                                "can only index objects using unicode strings: {e:#}"
                            ))),
                            Ok(s) => {
                                let obj = obj.inner.lock().unwrap();
                                let value = match obj.get(s) {
                                    None => return Ok(LuaValue::Nil),
                                    Some(v) => v,
                                };
                                match value {
                                    Value::Null => Ok(LuaValue::Nil),
                                    Value::Bool(b) => Ok(LuaValue::Boolean(*b)),
                                    Value::String(s) => s.clone().into_lua(lua),
                                    Value::F64(u) => u.into_lua(lua),
                                    Value::I64(u) => u.into_lua(lua),
                                    Value::Array(_) => value.clone().into_lua(lua),
                                    Value::Object(_) => value.clone().into_lua(lua),
                                }
                            }
                        },
                        _ => Err(mlua::Error::external(
                            "can only index objects using string values",
                        )),
                    },
                    _ => Err(mlua::Error::external(
                        "can only index array or object values".to_string(),
                    )),
                }
            },
        );
        methods.add_meta_method(
            mlua::MetaMethod::NewIndex,
            |_, this, (key, value): (LuaValue, LuaValue)| -> mlua::Result<()> {
                match this {
                    Value::Array(arr) => match key {
                        LuaValue::Integer(i) => {
                            if i <= 0 {
                                return Err(mlua::Error::external(format!(
                                    "invalid array index {i}"
                                )));
                            }
                            // Convert lua 1-based indices to 0-based
                            let i = (i as usize) - 1;

                            let mut arr = arr.inner.lock().unwrap();
                            if i >= arr.len() {
                                return Err(mlua::Error::external(format!(
                                    "cannot make sparse array by inserting at {i} when len is {}",
                                    arr.len()
                                )));
                            }

                            let value = lua_value_to_gvalue(value)?;

                            if i == arr.len() - 1 {
                                arr.push(value);
                            } else {
                                arr[i] = value;
                            }

                            Ok(())
                        }
                        _ => Err(mlua::Error::external(
                            "can only index arrays using integer values",
                        )),
                    },
                    Value::Object(obj) => match key {
                        LuaValue::String(s) => match s.to_str() {
                            Err(e) => Err(mlua::Error::external(format!(
                                "can only index objects using unicode strings: {e:#}"
                            ))),
                            Ok(s) => {
                                let mut obj = obj.inner.lock().unwrap();
                                let value = lua_value_to_gvalue(value)?;
                                obj.insert(s.to_string(), value);
                                Ok(())
                            }
                        },
                        _ => Err(mlua::Error::external(
                            "can only index objects using string values",
                        )),
                    },
                    _ => Err(mlua::Error::external(
                        "can only index array or object values".to_string(),
                    )),
                }
            },
        );
    }
}

pub fn register(lua: &Lua) -> anyhow::Result<()> {
    let wezterm_mod = get_or_create_module(lua, "wezterm")?;
    wezterm_mod.set("GLOBAL", GLOBALS.clone())?;
    Ok(())
}

// ---------------------------------------------------------------------------
// L4c: rhai port of `register` above (see
// docs/plans/2026-07-23-lua-rhai-migration.md). Runs in parallel with the mlua
// path; does not replace or touch `register(lua: &Lua)`.
//
// ## Single shared storage, not two independent copies
//
// The whole point of `wezterm.GLOBAL` is sharing data *between* different
// parts of the config/event pipeline -- other windows, other event callbacks,
// etc -- regardless of which script engine touched it. Both the mlua and
// rhai paths therefore MUST observe the exact same underlying storage, or a
// value written from a rhai config/callback would be invisible from an mlua
// one and vice versa, defeating the feature's entire purpose (this is called
// out explicitly in the migration plan's "thin points" section).
//
// That sharing is achieved here the same way it already works *within* the
// mlua path across multiple `Lua` instances: `GLOBALS` (the `lazy_static`
// above) is a single process-wide `Value::Object` whose storage is an
// `Arc<Mutex<BTreeMap<String, Value>>>`. `register` above clones that `Value`
// (a cheap `Arc` bump, not a deep copy - see `Object`'s `#[derive(Clone)]`)
// into each `Lua` instance's `wezterm` module. `register_rhai` below does the
// exact same thing: it clones `GLOBALS` again and hands that clone to the
// rhai engine as the `GlobalData` custom type wrapping the *same* `Arc`. Since
// `Value`'s `Object`/`Array` variants both hold `Arc<Mutex<_>>` internally,
// every clone anywhere - whether handed to an `mlua::Lua` or a
// `rhai::Engine`, no matter how many of either are created - is a handle onto
// the one shared `BTreeMap`/`Vec` behind that `Arc`, so writes performed
// through one engine are immediately visible through any other. There is no
// second, independent rhai-side store to keep in sync: it is *the same*
// store, reused.
//
// ## Representation: not mlua-specific
//
// `Value` (this crate's own recursive JSON-like enum, not `rhai::Dynamic` or
// `mlua::Value`) was already engine-agnostic before this port -- it only
// touches mlua in the `UserData`/`IntoLua` impls above, which the rhai path
// below does not use at all. The rhai path instead converts through
// `config::rhai_value` (`rhai_dynamic_to_dynamic`/`dynamic_to_rhai_dynamic`,
// the same bridge every other L4 crate uses), going by way of
// `wezterm_dynamic::Value` as the common intermediate, exactly mirroring how
// the mlua path in this file converts through `LuaValue` on one side and this
// crate's own `Value` on the other. No mlua type ever appears on the rhai
// call path.
pub fn register_rhai(engine: &mut rhai::Engine) -> anyhow::Result<()> {
    engine.register_type_with_name::<GlobalData>("GlobalData");

    engine.register_indexer_get_set(
        |this: &mut GlobalData, key: rhai::Dynamic| -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
            global_data_index_get(this, key)
        },
        |this: &mut GlobalData, key: rhai::Dynamic, value: rhai::Dynamic| -> Result<(), Box<rhai::EvalAltResult>> {
            global_data_index_set(this, key, value)
        },
    );
    engine.register_get("len", |this: &mut GlobalData| -> Result<rhai::INT, Box<rhai::EvalAltResult>> {
        global_data_len(this)
    });
    engine.register_fn("to_string", |this: &mut GlobalData| -> String {
        format!("{:?}", this.0)
    });

    // `wezterm.GLOBAL` on the mlua path is a bare value (a table-like
    // userdata), not a function call -- there is no direct rhai analogue of
    // "a bare global constant visible without calling anything", since
    // `Engine::register_fn`/`register_static_module` only register callables,
    // and injecting a `Scope`-level constant would require plumbing a
    // `Scope` through this crate's `register_rhai(engine: &mut Engine)`
    // signature (which every other L4 crate's `register_rhai` also only
    // takes an `Engine`, so changing this crate's signature alone would be
    // inconsistent). Instead this exposes a zero-arg function,
    // `global_data()`, that returns a `GlobalData` handle onto the exact same
    // shared `GLOBALS` singleton `register` uses above -- the rhai call site
    // is `global_data()["key"]` rather than a bare `GLOBAL["key"]`, the same
    // kind of accessor-function adaptation already used for `nerdfonts` in
    // `termwiz-funcs`.
    engine.register_fn("global_data", global_data_rhai);

    Ok(())
}

fn global_data_rhai() -> GlobalData {
    GlobalData(GLOBALS.clone())
}

/// Thin rhai-registrable wrapper around this crate's `Value`, so that the
/// underlying recursive enum (also used directly by the mlua path above) can
/// be registered as a rhai custom type without leaking rhai-specific method
/// names onto the type the mlua path already depends on.
#[derive(Clone)]
pub struct GlobalData(Value);

fn global_data_index_get(
    this: &mut GlobalData,
    key: rhai::Dynamic,
) -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
    match &this.0 {
        Value::Array(arr) => {
            let i = key.as_int().map_err(|ty| -> Box<rhai::EvalAltResult> {
                format!("GlobalData: can only index arrays using integer values, got `{ty}`").into()
            })?;
            if i <= 0 {
                return Err(format!("GlobalData: invalid array index {i}").into());
            }
            // Convert rhai's 1-based-by-convention index (mirroring the mlua
            // path's Lua 1-based indices) to a 0-based Vec index.
            let idx = (i as usize) - 1;
            let arr = arr.inner.lock().unwrap();
            match arr.get(idx) {
                None => Ok(rhai::Dynamic::UNIT),
                Some(v) => Ok(config::rhai_value::dynamic_to_rhai_dynamic(&gvalue_to_dynamic(v))),
            }
        }
        Value::Object(obj) => {
            let key = key.into_immutable_string().map_err(|ty| -> Box<rhai::EvalAltResult> {
                format!("GlobalData: can only index objects using string keys, got `{ty}`").into()
            })?;
            let obj = obj.inner.lock().unwrap();
            match obj.get(key.as_str()) {
                None => Ok(rhai::Dynamic::UNIT),
                Some(v) => Ok(config::rhai_value::dynamic_to_rhai_dynamic(&gvalue_to_dynamic(v))),
            }
        }
        _ => Err("GlobalData: can only index array or object values".into()),
    }
}

fn global_data_index_set(
    this: &mut GlobalData,
    key: rhai::Dynamic,
    value: rhai::Dynamic,
) -> Result<(), Box<rhai::EvalAltResult>> {
    let value = config::rhai_value::rhai_dynamic_to_dynamic(&value).map_err(
        |err| -> Box<rhai::EvalAltResult> { format!("GlobalData: {err}").into() },
    )?;
    let value = dynamic_to_gvalue(&value);

    match &this.0 {
        Value::Array(arr) => {
            let i = key.as_int().map_err(|ty| -> Box<rhai::EvalAltResult> {
                format!("GlobalData: can only index arrays using integer values, got `{ty}`").into()
            })?;
            if i <= 0 {
                return Err(format!("GlobalData: invalid array index {i}").into());
            }
            let idx = (i as usize) - 1;
            let mut arr = arr.inner.lock().unwrap();
            if idx >= arr.len() {
                return Err(format!(
                    "GlobalData: cannot make sparse array by inserting at {idx} when len is {}",
                    arr.len()
                )
                .into());
            }
            if idx == arr.len() - 1 {
                arr.push(value);
            } else {
                arr[idx] = value;
            }
            Ok(())
        }
        Value::Object(obj) => {
            let key = key.into_immutable_string().map_err(|ty| -> Box<rhai::EvalAltResult> {
                format!("GlobalData: can only index objects using string keys, got `{ty}`").into()
            })?;
            let mut obj = obj.inner.lock().unwrap();
            obj.insert(key.to_string(), value);
            Ok(())
        }
        _ => Err("GlobalData: can only index array or object values".into()),
    }
}

fn global_data_len(this: &mut GlobalData) -> Result<rhai::INT, Box<rhai::EvalAltResult>> {
    match &this.0 {
        Value::Array(arr) => Ok(arr.inner.lock().unwrap().len() as rhai::INT),
        Value::Object(obj) => Ok(obj.inner.lock().unwrap().len() as rhai::INT),
        _ => Err("GlobalData: len is only defined for array or object values".into()),
    }
}

/// Converts this crate's own `Value` into a `wezterm_dynamic::Value`, the
/// engine-agnostic intermediate the `config::rhai_value` bridge expects (the
/// same role `gvalue_to_lua` plays for the mlua path above, just targeting
/// `wezterm_dynamic::Value` instead of `LuaValue`).
fn gvalue_to_dynamic(value: &Value) -> wezterm_dynamic::Value {
    use wezterm_dynamic::Value as DynValue;
    match value {
        Value::Null => DynValue::Null,
        Value::Bool(b) => DynValue::Bool(*b),
        Value::String(s) => DynValue::String(s.clone()),
        Value::I64(i) => DynValue::I64(*i),
        Value::F64(f) => DynValue::F64(*f),
        Value::Array(arr) => {
            let arr = arr.inner.lock().unwrap();
            DynValue::Array(arr.iter().map(gvalue_to_dynamic).collect())
        }
        Value::Object(obj) => {
            let obj = obj.inner.lock().unwrap();
            let mut result = std::collections::BTreeMap::new();
            for (k, v) in obj.iter() {
                result.insert(DynValue::String(k.clone()), gvalue_to_dynamic(v));
            }
            DynValue::Object(result.into())
        }
    }
}

/// The reverse of `gvalue_to_dynamic`: converts a `wezterm_dynamic::Value`
/// (the result of running an incoming rhai value through
/// `config::rhai_value::rhai_dynamic_to_dynamic`) into this crate's own
/// `Value`, ready to be stored into the shared `GLOBALS` tree. New
/// `Array`/`Object` containers created here get their own fresh `Arc<Mutex<_>>`
/// (matching `lua_value_to_gvalue_impl`'s behavior above for nested
/// tables/maps assigned into `GLOBAL`), which is correct: only the *root*
/// `GLOBALS` value is the long-lived, identity-shared singleton -- a nested
/// object/array value being written into it starts as a fresh, independently
/// addressable subtree, exactly as it would coming from the mlua path.
fn dynamic_to_gvalue(value: &wezterm_dynamic::Value) -> Value {
    use wezterm_dynamic::Value as DynValue;
    match value {
        DynValue::Null => Value::Null,
        DynValue::Bool(b) => Value::Bool(*b),
        DynValue::String(s) => Value::String(s.clone()),
        DynValue::I64(i) => Value::I64(*i),
        DynValue::U64(u) => Value::I64(*u as i64),
        DynValue::F64(f) => Value::F64(*f),
        DynValue::Array(arr) => Value::Array(Array {
            inner: Arc::new(Mutex::new(arr.iter().map(dynamic_to_gvalue).collect())),
        }),
        DynValue::Object(obj) => {
            let mut result = BTreeMap::new();
            for (k, v) in obj.iter() {
                let key = match k {
                    DynValue::String(s) => s.clone(),
                    other => format!("{other:?}"),
                };
                result.insert(key, dynamic_to_gvalue(v));
            }
            Value::Object(Object {
                inner: Arc::new(Mutex::new(result)),
            })
        }
    }
}
