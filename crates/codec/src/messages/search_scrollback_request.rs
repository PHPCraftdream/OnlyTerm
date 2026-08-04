use std::ops::Range;

use mux::pane::PaneId;
use serde::{Deserialize, Serialize};
use wezterm_term::StableRowIndex;

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct SearchScrollbackRequest {
    pub pane_id: PaneId,
    pub pattern: mux::pane::Pattern,
    pub range: Range<StableRowIndex>,
    pub limit: Option<u32>,
}
