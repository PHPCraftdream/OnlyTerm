use mux::pane::PaneId;
use serde::{Deserialize, Serialize};
use wezterm_term::StableRowIndex;

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct GetImageCell {
    pub pane_id: PaneId,
    pub line_idx: StableRowIndex,
    pub cell_idx: usize,
    pub data_hash: [u8; 32],
}
