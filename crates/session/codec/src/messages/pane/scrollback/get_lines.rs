use std::ops::Range;

use onlyterm_mux::pane::PaneId;
use onlyterm_term::StableRowIndex;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct GetLines {
    pub pane_id: PaneId,
    pub lines: Vec<Range<StableRowIndex>>,
}
