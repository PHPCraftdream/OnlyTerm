use mux::tab::TabId;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct TabReflowed {
    pub tab_id: TabId,
}
