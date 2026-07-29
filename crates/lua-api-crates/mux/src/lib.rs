#![warn(clippy::undocumented_unsafe_blocks)]
use config::keyassignment::SpawnTabDomain;
use config::impl_rhai_conversion_dynamic;
use mux::domain::{DomainId, SplitSource};
use mux::pane::{Pane, PaneId};
use mux::tab::{SplitDirection, SplitRequest, SplitSize, Tab, TabId};
use mux::window::{Window, WindowId};
use mux::Mux;
use portable_pty::CommandBuilder;
use std::collections::HashMap;
use std::sync::Arc;
use wezterm_dynamic::{FromDynamic, ToDynamic};
use wezterm_term::TerminalSize;

mod domain;
mod pane;
mod tab;
mod window;

pub use domain::MuxDomain;
pub use pane::MuxPane;
pub use tab::MuxTab;
pub use window::{set_gui_window_resolver_rhai, MuxWindow};

fn get_mux() -> anyhow::Result<Arc<Mux>> {
    Mux::try_get().ok_or_else(|| anyhow::anyhow!("cannot get Mux!?"))
}

/// rhai-flavored equivalent of `get_mux()` above, shared by every `_rhai`
/// module in this crate (also re-exported as `domain::get_mux_rhai` for
/// `domain.rs`'s own use, since it's defined first there in the mlua-mirrored
/// file layout -- both names resolve to the same function).
pub(crate) use domain::get_mux_rhai;


// ---------------------------------------------------------------------------
// L4d: rhai port of `register` above (see
// docs/plans/2026-07-23-lua-rhai-migration.md), plus the rhai custom-type
// registration for every `MuxDomain`/`MuxPane`/`MuxTab`/`MuxWindow` method
// defined in `domain.rs`/`pane.rs`/`tab.rs`/`window.rs`. Runs in parallel with
// the mlua path; does not replace or touch `register(lua: &Lua)`.
//
// Registered under a `mux` static module, the same call-site shape
// (`mux.get_window(id)`, `mux.all_windows()`, etc) as the mlua path's
// `wezterm.mux.*` sub-module.
//
// `get_domain` takes `LuaValue` on the mlua side to accept nil/string/integer
// interchangeably; rhai's `register_fn` only supports fixed-type overloads
// resolved by arity+type at the call site (not a single dynamically-typed
// parameter), so this registers three separate overloads instead: a
// zero-arg `get_domain()` (nil-equivalent, the default domain), a
// `get_domain(name: String)`, and a `get_domain(id: INT)`.
pub fn register_rhai(engine: &mut rhai::Engine) -> anyhow::Result<()> {
    domain::register_rhai(engine)?;
    pane::register_rhai(engine)?;
    tab::register_rhai(engine)?;
    window::register_rhai(engine)?;

    let mut mux_module = rhai::Module::new();

    mux_module.set_native_fn(
        "get_active_workspace",
        || -> Result<String, Box<rhai::EvalAltResult>> {
            let mux = get_mux_rhai()?;
            Ok(mux.active_workspace())
        },
    );

    mux_module.set_native_fn(
        "get_workspace_names",
        || -> Result<rhai::Array, Box<rhai::EvalAltResult>> {
            let mux = get_mux_rhai()?;
            Ok(mux
                .iter_workspaces()
                .into_iter()
                .map(rhai::Dynamic::from)
                .collect())
        },
    );

    mux_module.set_native_fn(
        "set_active_workspace",
        |workspace: String| -> Result<(), Box<rhai::EvalAltResult>> {
            let mux = get_mux_rhai()?;
            let workspaces = mux.iter_workspaces();
            if workspaces.contains(&workspace) {
                mux.set_active_workspace(&workspace);
                Ok(())
            } else {
                Err(format!("{:?} is not an existing workspace", workspace).into())
            }
        },
    );

    mux_module.set_native_fn(
        "rename_workspace",
        |old_workspace: String, new_workspace: String| -> Result<(), Box<rhai::EvalAltResult>> {
            let mux = get_mux_rhai()?;
            mux.rename_workspace(&old_workspace, &new_workspace);
            Ok(())
        },
    );

    mux_module.set_native_fn(
        "get_window",
        |window_id: rhai::INT| -> Result<MuxWindow, Box<rhai::EvalAltResult>> {
            let mux = get_mux_rhai()?;
            let window_id = window_id_from_rhai_int(window_id)?;
            let window = MuxWindow(window_id);
            let _ = window::resolve_rhai(&window, &mux)?;
            Ok(window)
        },
    );

    mux_module.set_native_fn(
        "get_pane",
        |pane_id: rhai::INT| -> Result<MuxPane, Box<rhai::EvalAltResult>> {
            let mux = get_mux_rhai()?;
            let pane_id = pane_id_from_rhai_int(pane_id)?;
            let pane = MuxPane(pane_id);
            pane::resolve_rhai(&pane, &mux)?;
            Ok(pane)
        },
    );

    mux_module.set_native_fn(
        "get_tab",
        |tab_id: rhai::INT| -> Result<MuxTab, Box<rhai::EvalAltResult>> {
            let mux = get_mux_rhai()?;
            let tab_id = tab_id_from_rhai_int(tab_id)?;
            let tab = MuxTab(tab_id);
            tab::resolve_rhai(&tab, &mux)?;
            Ok(tab)
        },
    );

    mux_module.set_native_fn(
        "spawn_window",
        |spawn: rhai::Map| -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
            let spawn = SpawnWindow::try_from(rhai::Dynamic::from_map(spawn)).map_err(
                |err| -> Box<rhai::EvalAltResult> { format!("spawn_window: {err}").into() },
            )?;
            spawn_window_rhai(spawn)
        },
    );
    // Zero-arg overload: `mux.spawn_window()` with defaults.
    mux_module.set_native_fn(
        "spawn_window",
        || -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
            spawn_window_rhai(SpawnWindow::default())
        },
    );

    mux_module.set_native_fn(
        "all_windows",
        || -> Result<rhai::Array, Box<rhai::EvalAltResult>> {
            let mux = get_mux_rhai()?;
            Ok(mux
                .iter_windows()
                .into_iter()
                .map(|id| rhai::Dynamic::from(MuxWindow(id)))
                .collect())
        },
    );

    // Zero-arg overload of `get_domain`: the mlua path's nil case, returning
    // the default domain.
    mux_module.set_native_fn(
        "get_domain",
        || -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
            let mux = get_mux_rhai()?;
            Ok(rhai::Dynamic::from(MuxDomain(
                mux.default_domain().domain_id(),
            )))
        },
    );
    mux_module.set_native_fn(
        "get_domain",
        |name: String| -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
            let mux = get_mux_rhai()?;
            Ok(match mux.get_domain_by_name(&name) {
                Some(dom) => rhai::Dynamic::from(MuxDomain(dom.domain_id())),
                None => rhai::Dynamic::UNIT,
            })
        },
    );
    mux_module.set_native_fn(
        "get_domain",
        |id: rhai::INT| -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
            let mux = get_mux_rhai()?;
            let id: DomainId = id.try_into().map_err(|err| -> Box<rhai::EvalAltResult> {
                format!("invalid domain identifier passed to mux::get_domain: {err:#}").into()
            })?;
            Ok(match mux.get_domain(id) {
                Some(dom) => rhai::Dynamic::from(MuxDomain(dom.domain_id())),
                None => rhai::Dynamic::UNIT,
            })
        },
    );

    mux_module.set_native_fn(
        "all_domains",
        || -> Result<rhai::Array, Box<rhai::EvalAltResult>> {
            let mux = get_mux_rhai()?;
            Ok(mux
                .iter_domains()
                .into_iter()
                .map(|dom| rhai::Dynamic::from(MuxDomain(dom.domain_id())))
                .collect())
        },
    );

    mux_module.set_native_fn(
        "set_default_domain",
        |domain: MuxDomain| -> Result<(), Box<rhai::EvalAltResult>> {
            let mux = get_mux_rhai()?;
            let domain = domain::resolve_rhai(&domain, &mux)?;
            mux.set_default_domain(&domain);
            Ok(())
        },
    );

    engine.register_static_module("mux", mux_module.into());

    Ok(())
}

fn spawn_window_rhai(spawn: SpawnWindow) -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
    let (tab, pane, window) = smol::block_on(spawn.spawn())
        .map_err(|err| -> Box<rhai::EvalAltResult> { format!("{err:#}").into() })?;
    Ok(rhai::Dynamic::from(vec![
        rhai::Dynamic::from(tab),
        rhai::Dynamic::from(pane),
        rhai::Dynamic::from(window),
    ]))
}

/// rhai has no unsigned integer type (`INT` is `i64`, or `i32` under
/// `only_i32`), unlike the mlua path's plain id newtypes, so each rhai
/// binding above takes `rhai::INT` and narrows it here, rejecting
/// out-of-range/negative ids with a proper rhai error instead of silently
/// truncating.
fn window_id_from_rhai_int(id: rhai::INT) -> Result<WindowId, Box<rhai::EvalAltResult>> {
    WindowId::try_from(id).map_err(|_| -> Box<rhai::EvalAltResult> {
        format!("window id {id} is out of range").into()
    })
}

fn pane_id_from_rhai_int(id: rhai::INT) -> Result<PaneId, Box<rhai::EvalAltResult>> {
    PaneId::try_from(id)
        .map_err(|_| -> Box<rhai::EvalAltResult> { format!("pane id {id} is out of range").into() })
}

fn tab_id_from_rhai_int(id: rhai::INT) -> Result<TabId, Box<rhai::EvalAltResult>> {
    TabId::try_from(id)
        .map_err(|_| -> Box<rhai::EvalAltResult> { format!("tab id {id} is out of range").into() })
}

#[derive(Debug, Default, FromDynamic, ToDynamic)]
struct CommandBuilderFrag {
    args: Option<Vec<String>>,
    cwd: Option<String>,
    #[dynamic(default)]
    set_environment_variables: HashMap<String, String>,
}

impl CommandBuilderFrag {
    fn to_command_builder(&self) -> (Option<CommandBuilder>, Option<String>) {
        if let Some(args) = &self.args {
            let mut builder = CommandBuilder::from_argv(args.iter().map(Into::into).collect());
            for (k, v) in self.set_environment_variables.iter() {
                builder.env(k, v);
            }
            if let Some(cwd) = self.cwd.clone() {
                builder.cwd(cwd);
            }
            (Some(builder), None)
        } else {
            (None, self.cwd.clone())
        }
    }
}

#[derive(Debug, FromDynamic, ToDynamic)]
enum HandySplitDirection {
    Left,
    Right,
    Top,
    Bottom,
}
impl_rhai_conversion_dynamic!(HandySplitDirection);

impl Default for HandySplitDirection {
    fn default() -> Self {
        Self::Right
    }
}

#[derive(Debug, Default, FromDynamic, ToDynamic)]
struct SpawnWindow {
    #[dynamic(default = "spawn_tab_default_domain")]
    domain: SpawnTabDomain,
    width: Option<usize>,
    height: Option<usize>,
    workspace: Option<String>,
    position: Option<config::GuiPosition>,
    #[dynamic(flatten)]
    cmd_builder: CommandBuilderFrag,
}
impl_rhai_conversion_dynamic!(SpawnWindow);

fn spawn_tab_default_domain() -> SpawnTabDomain {
    SpawnTabDomain::DefaultDomain
}

impl SpawnWindow {
    async fn spawn(self) -> anyhow::Result<(MuxTab, MuxPane, MuxWindow)> {
        let mux = get_mux()?;

        let size = match (self.width, self.height) {
            (Some(cols), Some(rows)) => TerminalSize {
                rows,
                cols,
                ..Default::default()
            },
            _ => config::configuration().initial_size(0, None),
        };

        let (cmd_builder, cwd) = self.cmd_builder.to_command_builder();
        let (tab, pane, window_id) = mux
            .spawn_tab_or_window(
                None,
                self.domain,
                cmd_builder,
                cwd,
                size,
                None,
                self.workspace.unwrap_or_else(|| mux.active_workspace()),
                self.position,
            )
            .await
            .map_err(|e| anyhow::anyhow!(format!("{:#?}", e)))?;

        Ok((
            MuxTab(tab.tab_id()),
            MuxPane(pane.pane_id()),
            MuxWindow(window_id),
        ))
    }
}

#[derive(Debug, Default, FromDynamic, ToDynamic)]
struct SpawnTab {
    #[dynamic(default)]
    domain: SpawnTabDomain,
    #[dynamic(flatten)]
    cmd_builder: CommandBuilderFrag,
}
impl_rhai_conversion_dynamic!(SpawnTab);

impl SpawnTab {
    async fn spawn(self, window: &MuxWindow) -> anyhow::Result<(MuxTab, MuxPane, MuxWindow)> {
        let mux = get_mux()?;
        let size;
        let pane;

        {
            let window = window.resolve(&mux)?;
            size = window
                .get_by_idx(0)
                .map(|tab| tab.get_size())
                .unwrap_or_else(|| config::configuration().initial_size(0, None));

            pane = window
                .get_active()
                .and_then(|tab| tab.get_active_pane().map(|pane| pane.pane_id()));
        };

        let (cmd_builder, cwd) = self.cmd_builder.to_command_builder();

        let (tab, pane, window_id) = mux
            .spawn_tab_or_window(
                Some(window.0),
                self.domain,
                cmd_builder,
                cwd,
                size,
                pane,
                String::new(),
                None, // optional gui window position
            )
            .await
            .map_err(|e| anyhow::anyhow!(format!("{:#?}", e)))?;

        Ok((
            MuxTab(tab.tab_id()),
            MuxPane(pane.pane_id()),
            MuxWindow(window_id),
        ))
    }
}

#[derive(Clone, FromDynamic, ToDynamic)]
struct MuxTabInfo {
    pub index: usize,
    pub is_active: bool,
}
impl_rhai_conversion_dynamic!(MuxTabInfo);

#[derive(Clone, FromDynamic, ToDynamic)]
struct MuxPaneInfo {
    /// The topological pane index that can be used to reference this pane
    pub index: usize,
    /// true if this is the active pane at the time the position was computed
    pub is_active: bool,
    /// true if this pane is zoomed
    pub is_zoomed: bool,
    /// The offset from the top left corner of the containing tab to the top
    /// left corner of this pane, in cells.
    pub left: usize,
    /// The offset from the top left corner of the containing tab to the top
    /// left corner of this pane, in cells.
    pub top: usize,
    /// The width of this pane in cells
    pub width: usize,
    pub pixel_width: usize,
    /// The height of this pane in cells
    pub height: usize,
    pub pixel_height: usize,
}
impl_rhai_conversion_dynamic!(MuxPaneInfo);
