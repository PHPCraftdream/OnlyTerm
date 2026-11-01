use crate::webgpu::AtlasMirrorLog;
use onlyterm_gpu_protocol::wire;
use std::cell::RefCell;
use window::bitmaps::Texture2d;
use window::{BitmapImage, Rect};

/// CPU-backed atlas used when the hosting process must not create a GPU device.
/// Writes are retained in the common bounded replay log for a remote renderer.
pub struct CpuAtlasTexture {
    width: usize,
    height: usize,
    mirror: RefCell<AtlasMirrorLog>,
}

impl CpuAtlasTexture {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            mirror: RefCell::new(AtlasMirrorLog::default()),
        }
    }

    /// Construct an atlas with an explicit mirror-log budget.
    pub fn with_mirror_limit(width: usize, height: usize, max_bytes: usize) -> Self {
        Self {
            width,
            height,
            mirror: RefCell::new(AtlasMirrorLog::with_limit(max_bytes)),
        }
    }

    #[cfg(test)]
    fn from_mirror(width: usize, height: usize, mirror: RefCell<AtlasMirrorLog>) -> Self {
        Self {
            width,
            height,
            mirror,
        }
    }

    pub fn drain_dirty_updates(&self) -> Vec<wire::AtlasUpdate> {
        self.mirror.borrow_mut().drain()
    }

    pub fn full_atlas_updates(&self) -> Vec<wire::AtlasUpdate> {
        self.mirror.borrow_mut().full_updates()
    }

    pub fn mirroring_failed(&self) -> bool {
        self.mirror.borrow().is_over_budget()
    }
}

impl Texture2d for CpuAtlasTexture {
    fn write(&self, rect: Rect, im: &dyn BitmapImage) {
        let (width, height) = im.image_dimensions();
        self.mirror.borrow_mut().record(
            (
                rect.min_x() as u32,
                rect.min_y() as u32,
                width as u32,
                height as u32,
            ),
            im.pixel_data_slice(),
        );
    }

    fn read(&self, _rect: Rect, _im: &mut dyn BitmapImage) {
        unimplemented!("CPU atlas is write-only; updates are forwarded to its renderer");
    }

    fn width(&self) -> usize {
        self.width
    }

    fn height(&self) -> usize {
        self.height
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn image(bytes: [u8; 4]) -> window::Image {
        let mut image = window::Image::new(1, 1);
        image.pixel_data_slice_mut().copy_from_slice(&bytes);
        image
    }

    fn rect(x: isize, y: isize) -> Rect {
        Rect::new(window::Point::new(x, y), window::Size::new(1, 1))
    }

    #[test]
    fn writes_are_dirty_and_full_replay_is_sorted_and_clears_pending() {
        let atlas = CpuAtlasTexture::new(8, 8);
        atlas.write(rect(3, 1), &image([1, 2, 3, 4]));
        atlas.write(rect(0, 2), &image([5, 6, 7, 8]));

        let full = atlas.full_atlas_updates();
        assert_eq!(
            full.iter().map(|u| (u.x, u.y)).collect::<Vec<_>>(),
            vec![(0, 2), (3, 1)]
        );
        assert_eq!(&*full[0].pixels, &[5, 6, 7, 8]);
        assert!(
            atlas.drain_dirty_updates().is_empty(),
            "full replay must consume pending deltas"
        );
    }

    #[test]
    fn dirty_drain_coalesces_same_rect_and_retains_later_writes_for_replay() {
        let atlas = CpuAtlasTexture::new(8, 8);
        atlas.write(rect(1, 2), &image([1, 2, 3, 4]));
        atlas.write(rect(1, 2), &image([9, 8, 7, 6]));
        let dirty = atlas.drain_dirty_updates();
        assert_eq!(dirty.len(), 1);
        assert_eq!(&*dirty[0].pixels, &[9, 8, 7, 6]);
        assert!(atlas.drain_dirty_updates().is_empty());
        let replay = atlas.full_atlas_updates();
        assert_eq!(replay.len(), 1);
        assert_eq!(&*replay[0].pixels, &[9, 8, 7, 6]);
    }

    #[test]
    fn over_budget_stops_deltas_and_full_replay() {
        let mirror = RefCell::new(AtlasMirrorLog::with_limit(4));
        let atlas = CpuAtlasTexture::from_mirror(2, 1, mirror);
        atlas.write(rect(0, 0), &image([1, 2, 3, 4]));
        atlas.write(rect(1, 0), &image([5, 6, 7, 8]));
        assert!(atlas.mirroring_failed());
        assert!(atlas.drain_dirty_updates().is_empty());
        assert!(atlas.full_atlas_updates().is_empty());
        atlas.write(rect(1, 0), &image([9, 10, 11, 12]));
        assert!(atlas.mirroring_failed());
    }
}
