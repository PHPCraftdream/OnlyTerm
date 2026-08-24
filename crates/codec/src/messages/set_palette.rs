use mux::pane::PaneId;
use onlyterm_term::color::ColorPalette;
use serde::{Deserialize, Serialize};

/// This is used both as a notification from server->client
/// and as a configuration request from client->server when
/// the client's preferred configuration changes
#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct SetPalette {
    pub pane_id: PaneId,
    pub palette: ColorPalette,
}
