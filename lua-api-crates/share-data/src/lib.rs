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

lazy_static::lazy_static! {
    static ref GLOBALS: Value = Value::Object(Object{inner:Arc::new(Mutex::new(BTreeMap::new()))});
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
