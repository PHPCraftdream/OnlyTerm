use crate::schemes::base16::Base16Scheme;
use crate::schemes::sexy::Sexy;
use config::{ColorSchemeFile, ColorSchemeMetaData, Gradient, Palette, RgbaColor, SrgbaTuple};
use std::convert::TryFrom;

mod image_colors;
pub mod schemes;

#[derive(Clone)]
pub struct ColorWrap(RgbaColor);

impl ColorWrap {
    pub fn complement(&self) -> Self {
        Self(self.0.complement().into())
    }
    pub fn complement_ryb(&self) -> Self {
        Self(self.0.complement_ryb().into())
    }
    pub fn triad(&self) -> (Self, Self) {
        let (a, b) = self.0.triad();
        (Self(a.into()), Self(b.into()))
    }
    pub fn square(&self) -> (Self, Self, Self) {
        let (a, b, c) = self.0.square();
        (Self(a.into()), Self(b.into()), Self(c.into()))
    }
    pub fn saturate(&self, factor: f64) -> Self {
        Self(self.0.saturate(factor).into())
    }
    pub fn saturate_fixed(&self, amount: f64) -> Self {
        Self(self.0.saturate_fixed(amount).into())
    }
    pub fn lighten(&self, factor: f64) -> Self {
        Self(self.0.lighten(factor).into())
    }
    pub fn lighten_fixed(&self, amount: f64) -> Self {
        Self(self.0.lighten_fixed(amount).into())
    }
    pub fn adjust_hue_fixed(&self, amount: f64) -> Self {
        Self(self.0.adjust_hue_fixed(amount).into())
    }
    pub fn adjust_hue_fixed_ryb(&self, amount: f64) -> Self {
        Self(self.0.adjust_hue_fixed_ryb(amount).into())
    }
}

impl PartialEq for ColorWrap {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl std::fmt::Display for ColorWrap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s: String = self.0.into();
        write!(f, "{s}")
    }
}

/// Shared, engine-agnostic implementation of `gradient`/`gradient_colors`, used
/// by both the mlua binding above and the rhai binding below so the two engines
/// can't drift in behavior.
fn gradient_colors_impl(gradient: &Gradient, num_colors: usize) -> anyhow::Result<Vec<ColorWrap>> {
    let g = gradient.build()?;
    Ok(g.colors(num_colors)
        .into_iter()
        .map(|c| {
            let tuple = SrgbaTuple::from(c);
            ColorWrap(tuple.into())
        })
        .collect())
}

// ---------------------------------------------------------------------------
// L4a: rhai port of `register` above (see
// docs/plans/2026-07-23-lua-rhai-migration.md). Runs in parallel with the mlua
// path; does not replace or touch `register(lua: &Lua)`.
//
// `ColorWrap` becomes a rhai custom type (`register_type_with_name` +
// `register_fn`/`register_get`), the rhai analogue of its mlua `UserData` impl
// above: same method names, same semantics, registered as instance methods on
// the type rather than as `UserDataMethods` entries. Two categories of
// mlua-side return shape have no 1:1 rhai equivalent and are adapted, each
// documented at its call site below:
//   * multi-value tuple returns (`triad`, `square`, `linear_rgba`, `hsla`,
//     `laba`) -> rhai `Array`, since rhai has no native multi-return;
//   * the mlua `(Palette, ColorSchemeMetaData)` tuple returned by
//     `load_scheme`/`load_terminal_sexy_scheme`/`load_base16_scheme` -> a
//     single `ColorSchemeFile` value (which already bundles exactly those two
//     fields, see `config/src/color.rs`), since that is a real existing type
//     rather than an ad hoc synthesized map, and round-trips cleanly with
//     `save_scheme` on the way back;
//   * `get_builtin_color_schemes`/`get_builtin_schemes`'s
//     `HashMap<String, Palette>` -> rhai `Map` (rhai's own associative-array
//     type), keyed the same way.
// ---------------------------------------------------------------------------

/// Registers a rhai instance method on `ColorWrap` under `name`, calling
/// `method` and converting its `Self`-typed result back into `ColorWrap`.
/// Small helper to keep the many unary "returns Self" methods below terse.
fn register_self_method(
    engine: &mut rhai::Engine,
    name: &'static str,
    method: fn(&ColorWrap, f64) -> ColorWrap,
) {
    engine.register_fn(name, move |this: &mut ColorWrap, arg: f64| method(this, arg));
}

pub fn register_rhai(engine: &mut rhai::Engine) -> anyhow::Result<()> {
    engine.register_type_with_name::<ColorWrap>("Color");

    engine.register_fn("to_string", |this: &mut ColorWrap| this.to_string());
    engine.register_fn("==", |a: &mut ColorWrap, b: ColorWrap| *a == b);

    engine.register_fn("complement", |this: &mut ColorWrap| this.complement());
    engine.register_fn("complement_ryb", |this: &mut ColorWrap| {
        this.complement_ryb()
    });
    // `triad`/`square` return multiple `Self` values on the mlua path (Lua
    // supports returning several values from one call, unpacked at the call
    // site). rhai has no native multi-return, so these come back as a plain
    // rhai `Array` of `Color` values instead; a `.rhai` script destructures it
    // positionally (`let (a, b) = arr[0], arr[1];`-style, or just indexes it).
    engine.register_fn("triad", |this: &mut ColorWrap| -> rhai::Array {
        let (a, b) = this.triad();
        vec![rhai::Dynamic::from(a), rhai::Dynamic::from(b)]
    });
    engine.register_fn("square", |this: &mut ColorWrap| -> rhai::Array {
        let (a, b, c) = this.square();
        vec![
            rhai::Dynamic::from(a),
            rhai::Dynamic::from(b),
            rhai::Dynamic::from(c),
        ]
    });

    register_self_method(engine, "saturate", ColorWrap::saturate);
    register_self_method(engine, "desaturate", |this, factor| {
        this.saturate(-factor)
    });
    register_self_method(engine, "saturate_fixed", ColorWrap::saturate_fixed);
    register_self_method(engine, "desaturate_fixed", |this, amount| {
        this.saturate_fixed(-amount)
    });
    register_self_method(engine, "lighten", ColorWrap::lighten);
    register_self_method(engine, "darken", |this, factor| this.lighten(-factor));
    register_self_method(engine, "lighten_fixed", ColorWrap::lighten_fixed);
    register_self_method(engine, "darken_fixed", |this, amount| {
        this.lighten_fixed(-amount)
    });
    register_self_method(engine, "adjust_hue_fixed", ColorWrap::adjust_hue_fixed);
    register_self_method(
        engine,
        "adjust_hue_fixed_ryb",
        ColorWrap::adjust_hue_fixed_ryb,
    );

    engine.register_fn("srgba_u8", |this: &mut ColorWrap| -> rhai::Array {
        let (r, g, b, a) = this.0.to_srgb_u8();
        vec![
            rhai::Dynamic::from_int(r as i64),
            rhai::Dynamic::from_int(g as i64),
            rhai::Dynamic::from_int(b as i64),
            rhai::Dynamic::from_int(a as i64),
        ]
    });
    engine.register_fn("linear_rgba", |this: &mut ColorWrap| -> rhai::Array {
        let rgba = this.0.to_linear();
        vec![
            rhai::Dynamic::from_float(rgba.0 as f64),
            rhai::Dynamic::from_float(rgba.1 as f64),
            rhai::Dynamic::from_float(rgba.2 as f64),
            rhai::Dynamic::from_float(rgba.3 as f64),
        ]
    });
    engine.register_fn("hsla", |this: &mut ColorWrap| -> rhai::Array {
        let (h, s, l, a) = this.0.to_hsla();
        vec![
            rhai::Dynamic::from_float(h),
            rhai::Dynamic::from_float(s),
            rhai::Dynamic::from_float(l),
            rhai::Dynamic::from_float(a),
        ]
    });
    engine.register_fn("laba", |this: &mut ColorWrap| -> rhai::Array {
        let (l, a, b, alpha) = this.0.to_laba();
        vec![
            rhai::Dynamic::from_float(l),
            rhai::Dynamic::from_float(a),
            rhai::Dynamic::from_float(b),
            rhai::Dynamic::from_float(alpha),
        ]
    });
    // `contrast_ratio`/`delta_e` return `f32` on the Rust side; widen to `f64`
    // (rhai's `FLOAT`) explicitly since rhai does not implicitly widen numeric
    // return types the way Rust's own numeric-conversion `as` does.
    engine.register_fn(
        "contrast_ratio",
        |this: &mut ColorWrap, other: ColorWrap| this.0.contrast_ratio(&other.0) as f64,
    );
    engine.register_fn("delta_e", |this: &mut ColorWrap, other: ColorWrap| {
        this.0.delta_e(&other.0) as f64
    });

    let mut color_module = rhai::Module::new();
    color_module.set_native_fn("parse", parse_color_rhai);
    color_module.set_native_fn("from_hsla", |h: f64, s: f64, l: f64, a: f64| {
        Ok(ColorWrap(SrgbaTuple::from_hsla(h, s, l, a).into()))
    });
    color_module.set_native_fn(
        "extract_colors_from_image",
        |file_name: String| -> Result<rhai::Array, Box<rhai::EvalAltResult>> {
            extract_colors_from_image_rhai(file_name, None)
        },
    );
    color_module.set_native_fn(
        "extract_colors_from_image",
        extract_colors_from_image_with_params_rhai,
    );
    color_module.set_native_fn("get_default_colors", get_default_colors_rhai);
    color_module.set_native_fn("load_scheme", load_scheme_rhai);
    color_module.set_native_fn("save_scheme", save_scheme_rhai);
    color_module.set_native_fn("load_terminal_sexy_scheme", load_terminal_sexy_scheme_rhai);
    color_module.set_native_fn("load_base16_scheme", load_base16_scheme_rhai);
    color_module.set_native_fn("gradient", gradient_colors_rhai);
    color_module.set_native_fn("get_builtin_schemes", get_builtin_color_schemes_rhai);
    engine.register_static_module("color", color_module.into());

    // Also mirrored directly on the top-level `wezterm`-equivalent namespace
    // for the two functions the mlua path additionally exposes there (see
    // `wezterm_mod.set("gradient_colors", ...)`/`wezterm_mod.set(
    // "get_builtin_color_schemes", ...)` in `register` above). There is no
    // shared `wezterm` module instance available to this crate in isolation
    // (that's wired up where `config/src/rhai_engine.rs` composes all 13
    // crates together), so these are registered as bare top-level rhai
    // functions here, matching the call-site shape
    // `wezterm.gradient_colors(...)`/`wezterm.get_builtin_color_schemes(...)`
    // would have once namespaced under the real `wezterm` module by that
    // integration step.
    engine.register_fn("gradient_colors", gradient_colors_rhai);
    engine.register_fn("get_builtin_color_schemes", get_builtin_color_schemes_rhai);

    Ok(())
}

fn parse_color_rhai(spec: String) -> Result<ColorWrap, Box<rhai::EvalAltResult>> {
    let color = RgbaColor::try_from(spec.clone()).map_err(|err| -> Box<rhai::EvalAltResult> {
        format!("failed to parse `{spec}` as RgbaColor: {err:#}").into()
    })?;
    Ok(ColorWrap(color))
}

/// Converts a `rhai::Dynamic` argument (e.g. a `#{...}` object-map literal in
/// a `.rhai` script) into a config struct that has an
/// `impl_rhai_conversion_dynamic!`-generated `TryFrom<rhai::Dynamic>` impl.
/// Needed because these config structs (`Gradient`, `Palette`,
/// `ColorSchemeMetaData`, `ExtractColorParams`) are not themselves registered
/// as rhai custom types (`register_type`) -- unlike `ColorWrap`/`Color`, they
/// are meant to be constructed from plain rhai object-map literals, the same
/// way a `.lua` config passes a plain Lua table where `FromDynamic` expects a
/// struct. Accepting `Dynamic` (rather than the struct type directly) as the
/// rhai-registered function's parameter type is what makes that possible:
/// `rhai::Module::set_native_fn`/`Engine::register_fn` only auto-convert
/// script-side values into parameter types that are themselves registered
/// rhai types or one of rhai's own built-ins (`Dynamic`, `Map`, `Array`,
/// `INT`, `FLOAT`, `String`, ...), so a bare `Gradient`/`Palette` parameter
/// type would only ever match a script-side `Gradient`/`Palette` custom-type
/// instance, never a `#{...}` map literal.
fn from_rhai_dynamic<T: TryFrom<rhai::Dynamic, Error = config::rhai_value::RhaiConversionError>>(
    value: rhai::Dynamic,
    what: &str,
) -> Result<T, Box<rhai::EvalAltResult>> {
    T::try_from(value).map_err(|err| -> Box<rhai::EvalAltResult> {
        format!("{what}: {err}").into()
    })
}

fn extract_colors_from_image_rhai(
    file_name: String,
    params: Option<image_colors::ExtractColorParams>,
) -> Result<rhai::Array, Box<rhai::EvalAltResult>> {
    let colors = image_colors::extract_colors_from_image_impl(file_name, params).map_err(
        |err| -> Box<rhai::EvalAltResult> { format!("extract_colors_from_image: {err:#}").into() },
    )?;
    Ok(colors.into_iter().map(rhai::Dynamic::from).collect())
}

fn extract_colors_from_image_with_params_rhai(
    file_name: String,
    params: rhai::Dynamic,
) -> Result<rhai::Array, Box<rhai::EvalAltResult>> {
    let params: image_colors::ExtractColorParams =
        from_rhai_dynamic(params, "extract_colors_from_image")?;
    extract_colors_from_image_rhai(file_name, Some(params))
}

// Returns `Result` (rather than a bare `Dynamic`) purely to satisfy
// `rhai::Module::set_native_fn`'s trait bound, which is only implemented for
// the "fallible" (`Result`-returning) function shape -- this call can never
// actually fail, unlike most other `_rhai` functions in this file, whose
// `Result` return reflects a genuinely fallible underlying operation.
fn get_default_colors_rhai() -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
    let palette: Palette = wezterm_term::color::ColorPalette::default().into();
    Ok(palette.into())
}

fn load_scheme_rhai(file_name: String) -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
    let data = std::fs::read_to_string(&file_name).map_err(|err| -> Box<rhai::EvalAltResult> {
        format!("load_scheme: error reading {file_name}: {err:#}").into()
    })?;
    let scheme = ColorSchemeFile::from_toml_str(&data)
        .map_err(|err| -> Box<rhai::EvalAltResult> { format!("load_scheme: {err:#}").into() })?;
    Ok(scheme.into())
}

fn save_scheme_rhai(
    colors: rhai::Dynamic,
    metadata: rhai::Dynamic,
    file_name: String,
) -> Result<(), Box<rhai::EvalAltResult>> {
    let colors: Palette = from_rhai_dynamic(colors, "save_scheme")?;
    let metadata: ColorSchemeMetaData = from_rhai_dynamic(metadata, "save_scheme")?;
    let scheme = ColorSchemeFile { colors, metadata };
    scheme
        .save_to_file(&file_name)
        .map_err(|err| format!("save_scheme: {err:#}").into())
}

fn load_terminal_sexy_scheme_rhai(
    file_name: String,
) -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
    let scheme = Sexy::load_file(file_name).map_err(|err| -> Box<rhai::EvalAltResult> {
        format!("load_terminal_sexy_scheme: {err:#}").into()
    })?;
    Ok(scheme.into())
}

fn load_base16_scheme_rhai(file_name: String) -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
    let scheme = Base16Scheme::load_file(file_name).map_err(|err| -> Box<rhai::EvalAltResult> {
        format!("load_base16_scheme: {err:#}").into()
    })?;
    Ok(scheme.into())
}

fn gradient_colors_rhai(
    gradient: rhai::Dynamic,
    num_colors: rhai::INT,
) -> Result<rhai::Array, Box<rhai::EvalAltResult>> {
    if num_colors < 0 {
        return Err(format!("gradient: num_colors must be non-negative, got {num_colors}").into());
    }
    let gradient: Gradient = from_rhai_dynamic(gradient, "gradient")?;
    let colors = gradient_colors_impl(&gradient, num_colors as usize)
        .map_err(|err| -> Box<rhai::EvalAltResult> { format!("gradient: {err:#}").into() })?;
    Ok(colors.into_iter().map(rhai::Dynamic::from).collect())
}

// See the comment on `get_default_colors_rhai` above for why this returns
// `Result` despite never actually failing.
fn get_builtin_color_schemes_rhai() -> Result<rhai::Map, Box<rhai::EvalAltResult>> {
    let mut map = rhai::Map::new();
    for (name, palette) in config::COLOR_SCHEMES.iter() {
        map.insert(name.as_str().into(), palette.clone().into());
    }
    Ok(map)
}
