use mux::pane::PaneId;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct SendPaste {
    pub pane_id: PaneId,
    pub data: String,
}
