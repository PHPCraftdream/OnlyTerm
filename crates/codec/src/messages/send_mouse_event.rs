use mux::pane::PaneId;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct SendMouseEvent {
    pub pane_id: PaneId,
    pub event: onlyterm_term::input::MouseEvent,
}
