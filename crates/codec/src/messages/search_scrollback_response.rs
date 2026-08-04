use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct SearchScrollbackResponse {
    pub results: Vec<mux::pane::SearchResult>,
}
