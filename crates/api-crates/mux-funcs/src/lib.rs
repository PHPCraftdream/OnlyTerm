#![warn(clippy::undocumented_unsafe_blocks)]
use mux::pane::{Pane, PaneId};
use mux::tab::{Tab, TabId};
use mux::window::{Window, WindowId};
use mux::Mux;
use std::sync::Arc;

mod domain;
mod pane;
mod tab;
mod window;

pub use domain::MuxDomain;
pub use pane::MuxPane;
pub use tab::MuxTab;
pub use window::MuxWindow;

// `register_rhai` (the "L4d" rhai port of `register` above, plus the rhai
// custom-type registration for every `MuxDomain`/`MuxPane`/`MuxTab`/
// `MuxWindow` method, and the private `spawn_window_rhai`/
// `window_id_from_rhai_int`/`pane_id_from_rhai_int`/`tab_id_from_rhai_int`/
// `CommandBuilderFrag`/`HandySplitDirection`/`SpawnWindow`/`SpawnTab`/
// `MuxTabInfo`/`MuxPaneInfo` helpers it alone used, along with the
// re-exported `domain::get_mux_rhai`/`window::set_gui_window_resolver_rhai`)
// used to live below this point. With the scripting layer removed it had no
// remaining caller anywhere in the workspace (only this crate's own
// `tests/rhai_smoke.rs`, which exercised nothing else) and has been deleted,
// along with the same-named `register_rhai` in `domain.rs`/`pane.rs`/
// `tab.rs`/`window.rs` that it composed. This crate's own `get_mux()` helper
// (used only by that deleted code) went dead in turn and has also been
// removed.
