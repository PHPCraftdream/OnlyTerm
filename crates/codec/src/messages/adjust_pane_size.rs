use config::keyassignment::PaneDirection;
use mux::pane::PaneId;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct AdjustPaneSize {
    pub pane_id: PaneId,
    pub direction: PaneDirection,
    pub amount: usize,
}
