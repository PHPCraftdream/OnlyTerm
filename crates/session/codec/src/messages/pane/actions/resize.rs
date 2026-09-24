use onlyterm_mux::pane::PaneId;
use onlyterm_mux::tab::TabId;
use onlyterm_term::TerminalSize;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct Resize {
    pub containing_tab_id: TabId,
    pub pane_id: PaneId,
    pub size: TerminalSize,
}
