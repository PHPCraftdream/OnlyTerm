use std::ops::Range;

use mux::pane::PaneId;
use serde::{Deserialize, Serialize};
use wezterm_term::StableRowIndex;

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct GetLines {
    pub pane_id: PaneId,
    pub lines: Vec<Range<StableRowIndex>>,
}
