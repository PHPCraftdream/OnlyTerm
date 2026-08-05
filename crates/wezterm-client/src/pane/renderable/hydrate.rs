use crate::domain::ClientInner;
use codec::*;
use lru::LruCache;
use mux::pane::PaneId;
use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};
use termwiz::image::{ImageCell, ImageData};
use wezterm_term::{Line, StableRowIndex};

lazy_static::lazy_static! {
    static ref IMAGE_LRU: Mutex<LruCache<[u8;32], Arc<ImageData>>> = Mutex::new(LruCache::new(NonZeroUsize::new(128).unwrap()));
}

pub(crate) async fn hydrate_lines(
    client: Arc<ClientInner>,
    pane_id: PaneId,
    serialized_lines: SerializedLines,
) -> Vec<(StableRowIndex, Line)> {
    let (lines, image_cells) = serialized_lines.extract_data();

    if image_cells.is_empty() {
        return lines;
    }

    let mut requests = HashMap::new();
    let mut data_by_hash = HashMap::new();
    for im in &image_cells {
        if let Some(data) = IMAGE_LRU.lock().unwrap().get(&im.data_hash) {
            data_by_hash.insert(im.data_hash, Arc::clone(data));
        } else {
            requests
                .entry(&im.data_hash)
                .or_insert_with(|| GetImageCell {
                    pane_id,
                    line_idx: im.line_idx,
                    cell_idx: im.cell_idx,
                    data_hash: im.data_hash,
                });
        }
    }

    for (_, request) in requests {
        match client.client.get_image_cell(request).await {
            Ok(GetImageCellResponse {
                data: Some(data), ..
            }) => {
                IMAGE_LRU
                    .lock()
                    .unwrap()
                    .put(data.hash(), Arc::clone(&data));
                data_by_hash.insert(data.hash(), data);
            }
            Ok(GetImageCellResponse { data: None, .. }) => {
                log::error!("no image data!");
            }

            Err(err) => {
                log::error!("failed to retrieve image {err:#}");
            }
        }
    }

    let mut line_by_idx = HashMap::new();
    for (line_idx, line) in lines {
        line_by_idx.insert(line_idx, line);
    }

    for im in image_cells {
        if let Some(data) = data_by_hash.get(&im.data_hash) {
            if let Some(line) = line_by_idx.get_mut(&im.line_idx) {
                if let Some(cell) = line.cells_mut_for_attr_changes_only().get_mut(im.cell_idx) {
                    cell.attrs_mut()
                        .attach_image(Box::new(ImageCell::with_z_index(
                            im.top_left,
                            im.bottom_right,
                            Arc::clone(data),
                            im.z_index,
                            im.padding_left,
                            im.padding_top,
                            im.padding_right,
                            im.padding_bottom,
                            im.image_id,
                            im.placement_id,
                        )));
                }
            }
        }
    }

    line_by_idx.into_iter().collect()
}
