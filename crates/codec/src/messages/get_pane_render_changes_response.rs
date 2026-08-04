use std::collections::HashMap;
use std::ops::Range;

use mux::pane::PaneId;
use mux::renderable::{RenderableDimensions, StableCursorPosition};
use mux::tab::SerdeUrl;
use serde::{Deserialize, Serialize};
use termwiz::surface::SequenceNo;
use wezterm_term::StableRowIndex;

use crate::input_serial::InputSerial;
use crate::lines::SerializedLines;

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct GetPaneRenderChangesResponse {
    pub pane_id: PaneId,
    pub mouse_grabbed: bool,
    pub cursor_position: StableCursorPosition,
    pub dimensions: RenderableDimensions,
    pub dirty_lines: Vec<Range<StableRowIndex>>,
    pub title: String,
    pub working_dir: Option<SerdeUrl>,
    /// Lines that the server thought we'd almost certainly
    /// want to fetch as soon as we received this response
    pub bonus_lines: SerializedLines,

    pub input_serial: Option<InputSerial>,
    pub seqno: SequenceNo,
    #[serde(default)]
    pub user_vars: HashMap<String, String>,
}
