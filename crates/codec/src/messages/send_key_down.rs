use mux::tab::TabId;
use serde::{Deserialize, Serialize};

use crate::input_serial::InputSerial;

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct SendKeyDown {
    pub pane_id: TabId,
    pub event: termwiz::input::KeyEvent,
    pub input_serial: InputSerial,
}
