use config::impl_rhai_conversion_dynamic;
use finl_unicode::grapheme_clusters::Graphemes;
use std::str::FromStr;
use termwiz::caps::{Capabilities, ColorLevel, ProbeHints};
use termwiz::cell::{grapheme_column_width, unicode_column_width, AttributeChange, CellAttributes};
use termwiz::color::{AnsiColor, ColorAttribute, ColorSpec, SrgbaTuple};
use termwiz::render::terminfo::TerminfoRenderer;
use termwiz::surface::change::Change;
use termwiz::surface::Line;
use wezterm_dynamic::{FromDynamic, ToDynamic};

#[derive(Clone)]
struct NerdFonts {}

#[derive(Debug, FromDynamic, ToDynamic, Clone, PartialEq, Eq)]
pub enum FormatColor {
    AnsiColor(AnsiColor),
    Color(String),
    Default,
}

impl FormatColor {
    fn to_attr(self) -> ColorAttribute {
        let spec: ColorSpec = self.into();
        let attr: ColorAttribute = spec.into();
        attr
    }
}

impl From<FormatColor> for ColorSpec {
    fn from(val: FormatColor) -> Self {
        match val {
            FormatColor::AnsiColor(c) => c.into(),
            FormatColor::Color(s) => {
                let rgba = SrgbaTuple::from_str(&s).unwrap_or_else(|()| (0xff, 0xff, 0xff).into());
                rgba.into()
            }
            FormatColor::Default => ColorSpec::Default,
        }
    }
}

#[derive(Debug, FromDynamic, ToDynamic, Clone, PartialEq, Eq)]
pub enum FormatItem {
    Foreground(FormatColor),
    Background(FormatColor),
    Attribute(AttributeChange),
    ResetAttributes,
    Text(String),
}
// Rhai-side analogue of the mlua conversion above (L4c, see
// docs/plans/2026-07-23-lua-rhai-migration.md): `FormatItem` already derives
// `FromDynamic`/`ToDynamic` (needed for the mlua path), so this is a
// mechanical application of the same macro pattern used elsewhere (e.g.
// `config/src/exec_domain.rs::ExecDomain`) -- it works transitively through
// `AttributeChange` (a `wezterm-cell` type that also derives
// `FromDynamic`/`ToDynamic`) with no further wiring needed, since both
// directions of the rhai bridge go through `wezterm_dynamic::Value` as the
// common intermediate, exactly like the mlua bridge does.
impl_rhai_conversion_dynamic!(FormatItem);

impl From<FormatItem> for Change {
    fn from(val: FormatItem) -> Self {
        match val {
            FormatItem::Attribute(change) => change.into(),
            FormatItem::Text(t) => t.into(),
            FormatItem::Foreground(c) => AttributeChange::Foreground(c.to_attr()).into(),
            FormatItem::Background(c) => AttributeChange::Background(c.to_attr()).into(),
            FormatItem::ResetAttributes => Change::AllAttributes(CellAttributes::default()),
        }
    }
}

struct FormatTarget {
    target: Vec<u8>,
}

impl std::io::Write for FormatTarget {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        std::io::Write::write(&mut self.target, buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl termwiz::render::RenderTty for FormatTarget {
    fn get_size_in_cells(&mut self) -> termwiz::Result<(usize, usize)> {
        Ok((80, 24))
    }
}

pub fn format_as_escapes(items: Vec<FormatItem>) -> anyhow::Result<String> {
    let mut changes: Vec<Change> = items.into_iter().map(Into::into).collect();
    changes.push(Change::AllAttributes(CellAttributes::default()).into());
    let mut renderer = new_wezterm_terminfo_renderer();
    let mut target = FormatTarget { target: vec![] };
    renderer.render_to(&changes, &mut target)?;
    Ok(String::from_utf8(target.target)?)
}

pub fn pad_right(mut result: String, width: usize) -> String {
    let mut len = unicode_column_width(&result, None);
    while len < width {
        result.push(' ');
        len += 1;
    }

    result
}

pub fn pad_left(mut result: String, width: usize) -> String {
    let mut len = unicode_column_width(&result, None);
    while len < width {
        result.insert(0, ' ');
        len += 1;
    }

    result
}

pub fn truncate_left(s: &str, max_width: usize) -> String {
    let mut result = vec![];
    let mut len = 0;
    let graphemes: Vec<_> = Graphemes::new(s).collect();
    for &g in graphemes.iter().rev() {
        let g_len = grapheme_column_width(g, None);
        if g_len + len > max_width {
            break;
        }
        result.push(g);
        len += g_len;
    }

    result.reverse();
    result.join("")
}

pub fn truncate_right(s: &str, max_width: usize) -> String {
    let mut result = String::new();
    let mut len = 0;
    for g in Graphemes::new(s) {
        let g_len = grapheme_column_width(g, None);
        if g_len + len > max_width {
            break;
        }
        result.push_str(g);
        len += g_len;
    }
    result
}

lazy_static::lazy_static! {
    static ref CAPS: Capabilities = {
        let data = include_bytes!("../../../termwiz/data/xterm-256color");
        let db = terminfo::Database::from_buffer(&data[..]).unwrap();
        Capabilities::new_with_hints(
            ProbeHints::new_from_env()
                .term(Some("xterm-256color".into()))
                .terminfo_db(Some(db))
                .color_level(Some(ColorLevel::TrueColor))
                .colorterm(None)
                .colorterm_bce(None)
                .term_program(Some("WezTerm".into()))
                .term_program_version(Some(config::wezterm_version().into())),
        )
        .expect("cannot fail to make internal Capabilities")
    };
}

pub fn new_wezterm_terminfo_renderer() -> TerminfoRenderer {
    TerminfoRenderer::new(CAPS.clone())
}

pub fn lines_to_escapes(lines: Vec<Line>) -> anyhow::Result<String> {
    let mut changes = vec![];
    let mut attr = CellAttributes::blank();
    for line in lines {
        changes.append(&mut line.changes(&attr));
        changes.push(Change::Text("\r\n".to_string()));
        if let Some(a) = line.visible_cells().last().map(|cell| cell.attrs().clone()) {
            attr = a;
        }
    }
    changes.push(Change::AllAttributes(CellAttributes::blank()));
    let mut renderer = new_wezterm_terminfo_renderer();

    struct Target {
        target: Vec<u8>,
    }

    impl std::io::Write for Target {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            std::io::Write::write(&mut self.target, buf)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl termwiz::render::RenderTty for Target {
        fn get_size_in_cells(&mut self) -> termwiz::Result<(usize, usize)> {
            Ok((80, 24))
        }
    }

    let mut target = Target { target: vec![] };
    renderer.render_to(&changes, &mut target)?;
    Ok(String::from_utf8(target.target)?)
}

// ---------------------------------------------------------------------------
// L4c: rhai port of `register` above (see
// docs/plans/2026-07-23-lua-rhai-migration.md). Runs in parallel with the mlua
// path; does not replace or touch `register(lua: &Lua)`.
//
// `NerdFonts` is a mlua `UserData` with a custom `Index` metamethod (arbitrary
// string key -> optional glyph string), giving the `wezterm.nerdfonts.xxx`
// property-access call site. rhai's `register_indexer_get` is the direct
// analogue -- it lets a registered type support `nerdfonts["fa_git"]`-style
// indexing without needing a distinct type per key, so `NerdFonts` itself is
// reused as-is (it's a plain unit struct in this crate, not mlua-specific) and
// registered as a rhai custom type + global constant, mirroring
// `wezterm_mod.set("nerdfonts", NerdFonts {})` on the mlua path.
//
// `permute_any_mods`/`permute_any_or_no_mods` take/return a Lua table on the
// mlua path; the rhai equivalent uses rhai's own `Map` type, which plays
// exactly the same "arbitrary string-keyed table" role.
pub fn register_rhai(engine: &mut rhai::Engine) -> anyhow::Result<()> {
    engine.register_type_with_name::<NerdFonts>("NerdFonts");
    engine.register_indexer_get(nerdfonts_indexer_get);
    engine.register_fn("new_nerd_fonts", new_nerd_fonts);
    // No global-constant registration API on `Engine` for a value (as
    // opposed to a function or a type); the sample `.rhai` call site is
    // `nerdfonts["fa_git"]`, so expose a zero-arg constructor
    // (`new_nerd_fonts()`) plus a convenience top-level indexed-lookup
    // function `nerdfonts(key)`, giving scripts a way to reach the same
    // lookup table without needing a pre-existing global binding.
    engine.register_fn("nerdfonts", nerdfonts_lookup);

    engine.register_fn("format", format_rhai);
    engine.register_fn("column_width", |s: String| -> rhai::INT {
        unicode_column_width(&s, None) as rhai::INT
    });
    engine.register_fn("pad_right", |s: String, width: rhai::INT| -> String {
        pad_right(s, width.max(0) as usize)
    });
    engine.register_fn("pad_left", |s: String, width: rhai::INT| -> String {
        pad_left(s, width.max(0) as usize)
    });
    engine.register_fn(
        "truncate_right",
        |s: String, max_width: rhai::INT| -> String { truncate_right(&s, max_width.max(0) as usize) },
    );
    engine.register_fn(
        "truncate_left",
        |s: String, max_width: rhai::INT| -> String { truncate_left(&s, max_width.max(0) as usize) },
    );
    engine.register_fn("permute_any_mods", permute_any_mods_rhai);
    engine.register_fn("permute_any_or_no_mods", permute_any_or_no_mods_rhai);

    Ok(())
}

fn nerdfonts_indexer_get(_this: &mut NerdFonts, key: &str) -> rhai::Dynamic {
    match termwiz::nerdfonts::NERD_FONTS.get(key) {
        Some(c) => rhai::Dynamic::from(c.to_string()),
        None => rhai::Dynamic::UNIT,
    }
}

fn new_nerd_fonts() -> NerdFonts {
    NerdFonts {}
}

fn nerdfonts_lookup(key: String) -> rhai::Dynamic {
    match termwiz::nerdfonts::NERD_FONTS.get(key.as_str()) {
        Some(c) => rhai::Dynamic::from(c.to_string()),
        None => rhai::Dynamic::UNIT,
    }
}

fn format_rhai(items: rhai::Array) -> Result<String, Box<rhai::EvalAltResult>> {
    // `FormatItem::try_from(Dynamic)` comes from `impl_rhai_conversion_dynamic!`
    // above, the same conversion path `load_scheme`/`save_scheme` etc. use in
    // `color-funcs` for their own `Dynamic`-typed parameters.
    let items: Vec<FormatItem> = items
        .into_iter()
        .map(|item| {
            FormatItem::try_from(item).map_err(|err| -> Box<rhai::EvalAltResult> {
                format!("format: {err}").into()
            })
        })
        .collect::<Result<_, _>>()?;
    format_as_escapes(items).map_err(|err| -> Box<rhai::EvalAltResult> {
        format!("format: {err:#}").into()
    })
}

/// rhai analogue of `permute_mods` above: takes/returns rhai's own `Map`
/// type (the rhai equivalent of an arbitrary string-keyed Lua table), setting
/// the `"mods"` key to each permuted modifier combination's string rendering,
/// exactly like the mlua path does via `new_item.set("mods", ...)`.
fn permute_mods_rhai(item: rhai::Map, allow_none: bool) -> rhai::Array {
    use wezterm_input_types::Modifiers;

    let mut result = vec![];
    for ctrl in &[Modifiers::NONE, Modifiers::CTRL] {
        for shift in &[Modifiers::NONE, Modifiers::SHIFT] {
            for alt in &[Modifiers::NONE, Modifiers::ALT] {
                for sup in &[Modifiers::NONE, Modifiers::SUPER] {
                    let flags = *ctrl | *shift | *alt | *sup;
                    if flags == Modifiers::NONE && !allow_none {
                        continue;
                    }

                    let mut new_item = item.clone();
                    new_item.insert("mods".into(), rhai::Dynamic::from(flags.to_string()));
                    result.push(rhai::Dynamic::from_map(new_item));
                }
            }
        }
    }
    result
}

fn permute_any_mods_rhai(item: rhai::Map) -> rhai::Array {
    permute_mods_rhai(item, false)
}

fn permute_any_or_no_mods_rhai(item: rhai::Map) -> rhai::Array {
    permute_mods_rhai(item, true)
}
