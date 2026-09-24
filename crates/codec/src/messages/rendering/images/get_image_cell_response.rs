use std::sync::Arc;

use mux::pane::PaneId;
use serde::{Deserialize, Serialize};
use termwiz::image::ImageData;

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct GetImageCellResponse {
    pub pane_id: PaneId,
    pub data: Option<Arc<ImageData>>,
}
