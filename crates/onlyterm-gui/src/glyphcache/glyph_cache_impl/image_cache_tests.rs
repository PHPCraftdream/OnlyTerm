use super::super::image_decode::{
    decoded_frame_for_test, ensure_test_storage, frame_state_for_test,
};
use super::*;
use onlyterm_blob_leases::BlobManager;
use std::cell::RefCell;
use std::sync::mpsc::sync_channel;

#[test]
fn pending_decode_retries_on_cache_miss_hit_and_after_first_frame() {
    ensure_test_storage();
    let (tx, rx) = sync_channel(1);
    // Supply the decoder channel directly so readiness is controlled by the
    // test, without sleeping or depending on an image-decoding thread.
    let decoded = DecodedImage {
        frame_start: RefCell::new(Instant::now() - Duration::from_secs(1)),
        current_frame: RefCell::new(0),
        image: Arc::new(ImageData::with_data(ImageDataType::EncodedLease(
            BlobManager::store(&[1]).unwrap(),
        ))),
        frames: RefCell::new(Some(frame_state_for_test(rx))),
    };
    let texture: Rc<dyn Texture2d> = Rc::new(ImageTexture::new(64, 64));
    let mut atlas = Atlas::new(&texture).unwrap();
    let mut cache = HashMap::new();

    for _ in 0..2 {
        // First call misses the sprite cache; second hits the placeholder.
        // Both must request a future retry rather than an expired animation
        // deadline. The old path returned frame_start + min_frame_duration;
        // the deliberately old start makes that deadline unambiguously past.
        let before = Instant::now();
        let (_, next, state) = GlyphCache::cached_image_impl(
            &mut cache,
            &mut atlas,
            &decoded,
            None,
            Duration::from_millis(16),
            AllowImage::Yes,
        )
        .unwrap();
        assert_eq!(state, LoadState::Loading);
        assert!(next.unwrap() > before);
    }

    tx.send(decoded_frame_for_test(
        BlobManager::store(&[255, 255, 255, 255]).unwrap(),
        Duration::from_millis(100),
        1,
        1,
    ))
    .unwrap();
    let (sprite, _, state) = GlyphCache::cached_image_impl(
        &mut cache,
        &mut atlas,
        &decoded,
        None,
        Duration::from_millis(16),
        AllowImage::Yes,
    )
    .unwrap();
    assert_eq!(state, LoadState::Loaded);
    assert_eq!(sprite.coords.size.width, 1);
    assert_eq!(sprite.coords.size.height, 1);

    // Make the next frame due without sleeping. Its producer is still alive
    // but has supplied nothing: keep the existing image and retry in future.
    *decoded.frame_start.borrow_mut() = Instant::now() - Duration::from_secs(1);
    let before = Instant::now();
    let (_, next, state) = GlyphCache::cached_image_impl(
        &mut cache,
        &mut atlas,
        &decoded,
        None,
        Duration::from_millis(16),
        AllowImage::Yes,
    )
    .unwrap();
    assert_eq!(state, LoadState::Loaded);
    assert!(next.unwrap() > before);
}
