use mux::pane::PaneId;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct WriteToPane {
    pub pane_id: PaneId,
    pub data: Vec<u8>,
}
