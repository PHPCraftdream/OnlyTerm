use mux::pane::PaneId;
use serde::{Deserialize, Serialize};

use crate::lines::SerializedLines;

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct GetLinesResponse {
    pub pane_id: PaneId,
    pub lines: SerializedLines,
}
