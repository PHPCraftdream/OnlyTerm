use mux::pane::PaneId;
use mux::tab::TabId;
use mux::window::WindowId;
use serde::{Deserialize, Serialize};
use wezterm_term::TerminalSize;

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct SpawnResponse {
    pub tab_id: TabId,
    pub pane_id: PaneId,
    pub window_id: WindowId,
    pub size: TerminalSize,
}
