use config::keyassignment::RotationDirection;
use mux::pane::PaneId;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct RotatePanes {
    pub pane_id: PaneId,
    pub direction: RotationDirection,
}
