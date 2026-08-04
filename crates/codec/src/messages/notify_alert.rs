use mux::pane::PaneId;
use serde::{Deserialize, Serialize};
use wezterm_term::Alert;

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct NotifyAlert {
    pub pane_id: PaneId,
    pub alert: Alert,
}
