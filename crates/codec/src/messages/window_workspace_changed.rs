use mux::window::WindowId;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct WindowWorkspaceChanged {
    pub window_id: WindowId,
    pub workspace: String,
}
