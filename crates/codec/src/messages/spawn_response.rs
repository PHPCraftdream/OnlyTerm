use mux::pane::PaneId;
use mux::tab::TabId;
use mux::window::WindowId;
use onlyterm_term::TerminalSize;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct SpawnResponse {
    pub tab_id: TabId,
    pub pane_id: PaneId,
    pub window_id: WindowId,
    pub size: TerminalSize,
}
