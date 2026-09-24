use onlyterm_mux::pane::PaneId;
use onlyterm_mux::renderable::{RenderableDimensions, StableCursorPosition};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct GetPaneRenderableDimensionsResponse {
    pub pane_id: PaneId,
    pub cursor_position: StableCursorPosition,
    pub dimensions: RenderableDimensions,
}
