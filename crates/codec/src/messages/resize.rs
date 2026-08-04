use mux::pane::PaneId;
use mux::tab::TabId;
use serde::{Deserialize, Serialize};
use wezterm_term::TerminalSize;

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct Resize {
    pub containing_tab_id: TabId,
    pub pane_id: PaneId,
    pub size: TerminalSize,
}
