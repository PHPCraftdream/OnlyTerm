use onlyterm_term::StableRowIndex;
use serde::{Deserialize, Serialize};
use termwiz::image::TextureCoordinate;

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq)]
pub struct SerializedImageCell {
    pub line_idx: StableRowIndex,
    pub cell_idx: usize,
    // The following fields are taken from termwiz::image::ImageCell
    pub top_left: TextureCoordinate,
    pub bottom_right: TextureCoordinate,
    /// Image::data::hash() for the ImageCell::data field
    pub data_hash: [u8; 32],
    pub z_index: i32,
    pub padding_left: u16,
    pub padding_top: u16,
    pub padding_right: u16,
    pub padding_bottom: u16,
    pub image_id: Option<u32>,
    pub placement_id: Option<u32>,
}
