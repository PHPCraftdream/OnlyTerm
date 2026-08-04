use mux::pane::PaneId;
use serde::{Deserialize, Serialize};
use wezterm_term::ClipboardSelection;

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct SetClipboard {
    pub pane_id: PaneId,
    pub clipboard: Option<String>,
    pub selection: ClipboardSelection,
}
