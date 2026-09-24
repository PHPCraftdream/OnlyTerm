use mux::pane::PaneId;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct SwapActivePaneWithIndex {
    pub active_pane_id: PaneId,
    pub with_pane_index: usize,
    pub keep_focus: bool,
}
