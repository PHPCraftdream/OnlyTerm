use super::decoded_refill::{response_channel, submit, RefillPixels, RefillResponse, SubmitResult};
use super::*;

use ::window::bitmaps::BitmapImage;
use anyhow::Context;
use image::{
    AnimationDecoder, DynamicImage, Frame, Frames, ImageDecoder, ImageFormat, ImageResult, Limits,
};
use lru::LruCache;
use onlyterm_blob_leases::{BlobLease, BlobManager, BoxedReader};
use std::cell::RefCell;
use std::collections::HashSet;
use std::io::Seek;
use std::num::NonZeroUsize;
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TryRecvError};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use termwiz::image::{ImageData, ImageDataType};

/// A helper struct to implement BitmapImage for ImageDataType while
/// holding the mutex for the sake of safety.
pub(super) struct DecodedImageHandle<'a> {
    pub(super) current_frame: usize,
    pub(super) h: MutexGuard<'a, ImageDataType>,
}

/// A bounded process-wide cache for decoded animation frames. Decoded frames
/// remain available across atlas evictions, but the cache cannot grow with the
/// number of images or animation frames. Large frames are loaded by the
/// decoder worker into a single transient response instead of blocking paint.
const DECODED_PIXEL_CACHE_BYTES: usize = 64 * 1024 * 1024;
const DECODED_PIXEL_CACHE_ENTRIES: usize = 1024;

struct DecodedPixelCache {
    entries: LruCache<[u8; 32], Arc<Vec<u8>>>,
    bytes: usize,
    max_bytes: usize,
}

impl DecodedPixelCache {
    fn new() -> Self {
        Self::with_limits(DECODED_PIXEL_CACHE_BYTES, DECODED_PIXEL_CACHE_ENTRIES)
    }

    fn with_limits(max_bytes: usize, max_entries: usize) -> Self {
        Self {
            entries: LruCache::new(NonZeroUsize::new(max_entries.max(1)).unwrap()),
            bytes: 0,
            max_bytes,
        }
    }

    fn get(&mut self, key: [u8; 32]) -> Option<Arc<Vec<u8>>> {
        self.entries.get(&key).cloned()
    }

    fn insert(&mut self, key: [u8; 32], pixels: Arc<Vec<u8>>) {
        let size = pixels.capacity();
        if size > self.max_bytes {
            return;
        }

        if let Some(previous) = self.entries.pop(&key) {
            self.bytes -= previous.capacity();
        }

        while self.bytes + size > self.max_bytes || self.entries.len() >= self.entries.cap().get() {
            let Some((_, previous)) = self.entries.pop_lru() else {
                break;
            };
            self.bytes -= previous.capacity();
        }

        self.bytes += size;
        self.entries.put(key, pixels);
    }

    #[cfg(test)]
    fn bytes(&self) -> usize {
        self.bytes
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries.len()
    }
}

static DECODED_PIXEL_CACHE: LazyLock<Mutex<DecodedPixelCache>> =
    LazyLock::new(|| Mutex::new(DecodedPixelCache::new()));

pub(super) fn decoded_pixels(key: [u8; 32]) -> Option<Arc<Vec<u8>>> {
    DECODED_PIXEL_CACHE.lock().unwrap().get(key)
}

pub(super) fn retain_decoded_pixels(key: [u8; 32], data: Vec<u8>) -> Arc<Vec<u8>> {
    let pixels = Arc::new(data);
    retain_shared_decoded_pixels(key, pixels)
}

pub(super) fn retain_shared_decoded_pixels(key: [u8; 32], pixels: Arc<Vec<u8>>) -> Arc<Vec<u8>> {
    DECODED_PIXEL_CACHE
        .lock()
        .unwrap()
        .insert(key, Arc::clone(&pixels));
    pixels
}

pub(super) struct DecodedPixelsHandle {
    pixels: Arc<Vec<u8>>,
    width: usize,
    height: usize,
}

impl DecodedPixelsHandle {
    pub(super) fn new(pixels: Arc<Vec<u8>>, width: usize, height: usize) -> anyhow::Result<Self> {
        let expected_len = width
            .checked_mul(height)
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or_else(|| anyhow::anyhow!("decoded image dimensions overflow pixel length"))?;
        if pixels.len() != expected_len {
            anyhow::bail!(
                "decoded image has {} bytes for {width}x{height} RGBA pixels, expected {expected_len}",
                pixels.len()
            );
        }
        Ok(Self {
            pixels,
            width,
            height,
        })
    }
}

impl BitmapImage for DecodedPixelsHandle {
    // SAFETY: The `Arc<Vec<u8>>` owns the immutable pixel allocation for the
    // lifetime of this handle, so its pointer remains valid while borrowed.
    unsafe fn pixel_data(&self) -> *const u8 {
        self.pixels.as_ptr()
    }

    // SAFETY: Decoded frame pixels are immutable and are shared with the
    // bounded cache. Returning a mutable pointer would violate that contract.
    unsafe fn pixel_data_mut(&mut self) -> *mut u8 {
        panic!("cannot mutate decoded frame pixels");
    }

    fn image_dimensions(&self) -> (usize, usize) {
        (self.width, self.height)
    }
}

impl<'a> BitmapImage for DecodedImageHandle<'a> {
    // SAFETY: Required to be `unsafe fn` by the `BitmapImage` trait contract.
    // The returned pointer borrows pixel data held alive by `self`: the
    // `MutexGuard` keeps the underlying `ImageDataType` pinned for the
    // handle's lifetime, so the pointer is valid while `&self` is live.
    // Callers must respect the `image_dimensions` bounds.
    unsafe fn pixel_data(&self) -> *const u8 {
        match &*self.h {
            ImageDataType::Rgba8 { data, .. } => data.as_ptr(),
            ImageDataType::AnimRgba8 { frames, .. } => frames[self.current_frame].as_ptr(),
            ImageDataType::EncodedLease(_) | ImageDataType::EncodedFile(_) => unreachable!(),
        }
    }

    // SAFETY: Trait-mandated `unsafe fn`. This implementation always panics:
    // decoded images are immutable, so no mutable pointer is ever produced.
    unsafe fn pixel_data_mut(&mut self) -> *mut u8 {
        panic!("cannot mutate DecodedImage");
    }

    fn image_dimensions(&self) -> (usize, usize) {
        match &*self.h {
            ImageDataType::Rgba8 { width, height, .. }
            | ImageDataType::AnimRgba8 { width, height, .. } => (*width as usize, *height as usize),
            ImageDataType::EncodedLease(_) | ImageDataType::EncodedFile(_) => unreachable!(),
        }
    }
}

#[derive(Clone)]
pub(super) struct DecodedFrame {
    pub(super) lease: BlobLease,
    duration: Duration,
    pub(super) width: usize,
    pub(super) height: usize,
}

pub(super) const IMAGE_DECODE_POLL_INTERVAL: Duration = Duration::from_millis(25);

pub(super) struct FrameDecoder {}

struct WebpFrames {
    decoder: image_webp::WebPDecoder<BoxedReader>,
    remaining: u32,
    raw_buf: Vec<u8>,
    width: u32,
    height: u32,
}

fn webp_decode_error(err: image_webp::DecodingError) -> image::ImageError {
    image::ImageError::Decoding(image::error::DecodingError::new(
        image::error::ImageFormatHint::Exact(ImageFormat::WebP),
        std::io::Error::other(err.to_string()),
    ))
}

impl Iterator for WebpFrames {
    type Item = ImageResult<Frame>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        self.remaining -= 1;

        let animated = self.decoder.is_animated();
        let delay_ms = if animated {
            match self.decoder.read_frame(&mut self.raw_buf) {
                Ok(delay_ms) => delay_ms,
                Err(err) => return Some(Err(webp_decode_error(err))),
            }
        } else {
            if let Err(err) = self.decoder.read_image(&mut self.raw_buf) {
                return Some(Err(webp_decode_error(err)));
            }
            u32::MAX
        };

        // A static WebP has no future decoder read, so move its output buffer
        // into the image instead of cloning it. Animated frames must retain
        // their own pixels while the decoder reuses `raw_buf` for the next.
        let frame_data = if !animated {
            std::mem::take(&mut self.raw_buf)
        } else {
            self.raw_buf.clone()
        };
        let image = if !self.decoder.has_alpha() {
            image::RgbImage::from_raw(self.width, self.height, frame_data)
                .map(|img_buf| image::DynamicImage::ImageRgb8(img_buf).to_rgba8())
        } else {
            image::RgbaImage::from_raw(self.width, self.height, frame_data)
        };

        let image = image?;
        let delay = image::Delay::from_numer_denom_ms(delay_ms, 1);
        Some(Ok(Frame::from_parts(image, 0, 0, delay)))
    }
}

impl FrameDecoder {
    pub fn start(
        lease: BlobLease,
    ) -> anyhow::Result<(
        Receiver<DecodedFrame>,
        SyncSender<RefillResponse>,
        Receiver<RefillResponse>,
    )> {
        let (tx, rx) = sync_channel(2);
        let (refill_response_tx, refill_response_rx) = response_channel();

        let buf_reader = lease.get_reader().context("lease.get_reader()")?;
        let reader = image::ImageReader::new(buf_reader)
            .with_guessed_format()
            .context("guess format from lease")?;
        let format = reader
            .format()
            .ok_or_else(|| anyhow::anyhow!("cannot determine image format"))?;

        std::thread::spawn(move || {
            if let Err(err) = Self::run_decoder_thread(reader, format, tx) {
                if err
                    .downcast_ref::<std::sync::mpsc::SendError<DecodedFrame>>()
                    .is_none()
                {
                    log::error!("Error decoding image: {err:#}");
                }
            }
        });

        Ok((rx, refill_response_tx, refill_response_rx))
    }

    fn run_decoder_thread(
        reader: image::ImageReader<BoxedReader>,
        format: ImageFormat,
        tx: SyncSender<DecodedFrame>,
    ) -> anyhow::Result<()> {
        let start = Instant::now();
        let limits = Limits::default();
        let mut frames = match format {
            ImageFormat::Gif => {
                let mut reader = reader.into_inner();
                reader.rewind().context("rewinding reader for gif")?;
                let mut decoder =
                    image::codecs::gif::GifDecoder::new(reader).context("GifDecoder::new")?;
                decoder
                    .set_limits(limits)
                    .context("GifDecoder::set_limits")?;
                decoder.into_frames()
            }
            ImageFormat::Png => {
                let mut reader = reader.into_inner();
                reader.rewind().context("rewinding reader for png")?;
                let decoder = image::codecs::png::PngDecoder::with_limits(reader, limits.clone())
                    .context("PngDecoder::with_limits")?;
                if decoder.is_apng().unwrap_or(false) {
                    decoder.apng()?.into_frames()
                } else {
                    let buf = DynamicImage::from_decoder(decoder)?.into_rgba8();
                    let delay = image::Delay::from_numer_denom_ms(u32::MAX, 1);
                    let frame = Frame::from_parts(buf, 0, 0, delay);
                    Frames::new(Box::new(std::iter::once(ImageResult::Ok(frame))))
                }
            }
            ImageFormat::WebP => {
                let mut reader = reader.into_inner();
                reader.rewind().context("rewinding reader for WebP")?;
                let mut decoder = image_webp::WebPDecoder::new(reader).context("WebPDecoder")?;
                if let Some(limit) = limits.max_alloc {
                    decoder.set_memory_limit(limit as usize)
                }

                let (width, height) = decoder.dimensions();
                let raw_len = decoder
                    .output_buffer_size()
                    .context("Invalid buffer size")?;
                let frame_count = if decoder.is_animated() {
                    decoder.num_frames()
                } else {
                    1
                };
                Frames::new(Box::new(WebpFrames {
                    decoder,
                    remaining: frame_count,
                    raw_buf: vec![0u8; raw_len],
                    width,
                    height,
                }))
            }
            _ => {
                let buf = reader.decode().context("decode image")?;
                let delay = image::Delay::from_numer_denom_ms(u32::MAX, 1);
                let frame = Frame::from_parts(buf.into_rgba8(), 0, 0, delay);
                Frames::new(Box::new(std::iter::once(ImageResult::Ok(frame))))
            }
        };

        let frame = frames
            .next()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Unable to decode image data. Either it is corrupt, or \
                    the Image format is not fully supported by \
                    https://github.com/image-rs/image/blob/master/README.md#supported-image-formats")
            })?;
        let frame = frame.context("first frame result")?;

        let mut decoded_frame_count = 1usize;
        let (width, height) = frame.buffer().dimensions();
        let width = width as usize;
        let height = height as usize;

        let duration: Duration = frame.delay().into();
        log::debug!("first frame took {:?} to decode.", start.elapsed());

        let data = frame.into_buffer().into_raw();
        let lease = BlobManager::store(&data).context("BlobManager::store")?;
        retain_decoded_pixels(lease.content_id().as_hash_bytes(), data);
        let decoded_frame = DecodedFrame {
            lease,
            duration,
            width,
            height,
        };
        tx.send(decoded_frame.clone())
            .context("sending first frame")?;
        drop(decoded_frame);

        for frame in frames.by_ref() {
            let frame = frame?;

            let duration: Duration = frame.delay().into();
            let data = frame.into_buffer().into_raw();
            let lease = BlobManager::store(&data).context("BlobManager::store")?;
            retain_decoded_pixels(lease.content_id().as_hash_bytes(), data);

            let decoded_frame = DecodedFrame {
                lease,
                duration,
                width,
                height,
            };
            tx.send(decoded_frame.clone()).context("sending a frame")?;
            decoded_frame_count += 1;
            drop(decoded_frame);
        }

        drop(frames);

        let elapsed = start.elapsed();
        let fps = decoded_frame_count as f32 / elapsed.as_secs_f32();

        log::debug!(
            "decoded {} frames, {} bytes in {elapsed:?}, {fps} fps",
            decoded_frame_count,
            decoded_frame_count * width * height * 4
        );

        Ok(())
    }
}

#[derive(Copy, Clone)]
pub(super) enum FrameSource {
    Decoder,
    FrameIndex(usize),
}

enum DecoderPoll {
    Frame(DecodedFrame),
    Empty,
    Disconnected,
}

fn poll_decoder(rx: &Receiver<DecodedFrame>) -> DecoderPoll {
    match rx.try_recv() {
        Ok(frame) => DecoderPoll::Frame(frame),
        Err(TryRecvError::Empty) => DecoderPoll::Empty,
        Err(TryRecvError::Disconnected) => DecoderPoll::Disconnected,
    }
}

pub(super) struct FrameState {
    pub(super) source: FrameSource,
    receiver: Receiver<DecodedFrame>,
    refill_receiver: Receiver<RefillResponse>,
    refill_tx: SyncSender<RefillResponse>,
    pending_refills: HashSet<[u8; 32]>,
    failed_refills: HashSet<[u8; 32]>,
    refilled_pixels: Option<([u8; 32], RefillPixels)>,
    pub(super) current_frame: DecodedFrame,
    current_index: usize,
    pub(super) frames: Vec<DecodedFrame>,
    pub(super) load_state: LoadState,
}

impl FrameState {
    fn new(
        rx: Receiver<DecodedFrame>,
        refill_tx: SyncSender<RefillResponse>,
        refill_receiver: Receiver<RefillResponse>,
    ) -> Self {
        const BLACK_SIZE: usize = 8;
        static BLACK: LazyLock<BlobLease> = LazyLock::new(|| {
            let mut data = vec![];
            for _ in 0..BLACK_SIZE * BLACK_SIZE {
                data.extend_from_slice(&[0, 0, 0, 0xff]);
            }
            BlobManager::store(&data).unwrap()
        });

        Self {
            source: FrameSource::Decoder,
            receiver: rx,
            refill_receiver,
            refill_tx,
            pending_refills: HashSet::new(),
            failed_refills: HashSet::new(),
            refilled_pixels: None,
            frames: vec![],
            current_frame: DecodedFrame {
                lease: BLACK.clone(),
                width: BLACK_SIZE,
                height: BLACK_SIZE,
                duration: Duration::from_millis(0),
            },
            current_index: 0,
            load_state: LoadState::Loading,
        }
    }

    pub(super) fn request_refill(&mut self, frame: &DecodedFrame) {
        let key = frame.lease.content_id().as_hash_bytes();
        if self.failed_refills.contains(&key) {
            return;
        }
        if !self.pending_refills.insert(key) {
            return;
        }
        let Some(expected_len) = frame
            .width
            .checked_mul(frame.height)
            .and_then(|pixels| pixels.checked_mul(4))
        else {
            self.pending_refills.remove(&key);
            self.failed_refills.insert(key);
            return;
        };
        match submit(key, frame.lease.clone(), expected_len, &self.refill_tx) {
            SubmitResult::Queued => {}
            SubmitResult::Busy => {
                self.pending_refills.remove(&key);
            }
            SubmitResult::Unavailable => {
                self.pending_refills.remove(&key);
                self.failed_refills.insert(key);
            }
        }
    }

    pub(super) fn take_refilled_pixels(&mut self, key: [u8; 32]) -> Option<RefillPixels> {
        if self
            .refilled_pixels
            .as_ref()
            .is_some_and(|(cached_key, _)| *cached_key == key)
        {
            self.refilled_pixels.take().map(|(_, pixels)| pixels)
        } else {
            None
        }
    }

    pub(super) fn refill_failed(&self, key: [u8; 32]) -> bool {
        self.failed_refills.contains(&key)
    }

    fn accept_frame(&mut self, frame: DecodedFrame) -> bool {
        let key = frame.lease.content_id().as_hash_bytes();
        if self.pending_refills.remove(&key) {
            if let Some(existing) = self
                .frames
                .iter_mut()
                .find(|existing| existing.lease.content_id().as_hash_bytes() == key)
            {
                *existing = frame.clone();
            }
            if self.current_frame.lease.content_id().as_hash_bytes() == key {
                self.current_frame = frame;
            }
            false
        } else {
            self.frames.push(frame.clone());
            self.current_frame = frame;
            self.current_index = self.frames.len() - 1;
            self.load_state = LoadState::Loaded;
            true
        }
    }

    pub(super) fn poll_refills(&mut self) {
        while let Ok(response) = self.refill_receiver.try_recv() {
            let key = response.key;
            self.pending_refills.remove(&key);
            let (pixels, error) = response.finish();
            if let Some(pixels) = pixels {
                self.failed_refills.remove(&key);
                self.refilled_pixels = Some((
                    key,
                    RefillPixels {
                        pixels: retain_shared_decoded_pixels(key, pixels.pixels),
                        reservation: pixels.reservation,
                    },
                ));
            } else {
                self.failed_refills.insert(key);
                if let Some(error) = error {
                    log::warn!("decoded frame refill failed: {error}");
                }
            }
        }
    }

    pub(super) fn load_next_frame(&mut self) -> bool {
        self.poll_refills();
        match self.source {
            FrameSource::Decoder => match poll_decoder(&self.receiver) {
                DecoderPoll::Frame(frame) => self.accept_frame(frame),
                DecoderPoll::Empty => false,
                DecoderPoll::Disconnected => {
                    self.source = FrameSource::FrameIndex(self.current_index);
                    if self.frames.is_empty() {
                        log::warn!("image decoder thread terminated");
                        self.current_frame.duration = Duration::from_secs(86400);
                        self.frames.push(self.current_frame.clone());
                        self.current_index = 0;
                        self.load_state = LoadState::Loaded;
                        false
                    } else if self.frames.len() == 1 {
                        // If there's only a single frame, we may as well ensure
                        // that it has a long duration so that we don't waste
                        // resources ticking to the same frame over and over
                        let duration = Duration::from_secs(86400);
                        self.frames[0].duration = duration;
                        self.current_frame.duration = duration;
                        false
                    } else {
                        false
                    }
                }
            },
            FrameSource::FrameIndex(mut idx) => {
                idx += 1;
                if idx >= self.frames.len() {
                    idx = 0;
                }
                self.current_frame = self.frames[idx].clone();
                self.current_index = idx;
                self.source = FrameSource::FrameIndex(idx);
                true
            }
        }
    }

    pub(super) fn frame_duration(&self) -> Duration {
        self.current_frame.duration
    }

    pub(super) fn frame_hash(&self) -> [u8; 32] {
        self.current_frame.lease.content_id().as_hash_bytes()
    }
}

impl std::fmt::Debug for FrameState {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
        fmt.debug_struct("FrameState").finish()
    }
}

#[derive(Debug)]
pub struct DecodedImage {
    pub(super) frame_start: RefCell<Instant>,
    pub(super) current_frame: RefCell<usize>,
    pub(super) image: Arc<ImageData>,
    pub(super) frames: RefCell<Option<FrameState>>,
}

impl DecodedImage {
    fn placeholder() -> Self {
        let image = ImageData::with_data(ImageDataType::placeholder());
        Self {
            frame_start: RefCell::new(Instant::now()),
            current_frame: RefCell::new(0),
            image: Arc::new(image),
            frames: RefCell::new(None),
        }
    }

    fn start_frame_decoder(lease: BlobLease, image_data: &Arc<ImageData>) -> Self {
        match FrameDecoder::start(lease.clone()) {
            Ok((rx, refill_tx, refill_receiver)) => Self {
                frame_start: RefCell::new(Instant::now()),
                current_frame: RefCell::new(0),
                image: Arc::clone(image_data),
                frames: RefCell::new(Some(FrameState::new(rx, refill_tx, refill_receiver))),
            },
            Err(err) => {
                log::error!("failed to start FrameDecoder: {err:#}");
                Self::placeholder()
            }
        }
    }

    pub(super) fn load(image_data: &Arc<ImageData>) -> Self {
        match &*image_data.data() {
            ImageDataType::EncodedLease(lease) => {
                Self::start_frame_decoder(lease.clone(), image_data)
            }
            ImageDataType::EncodedFile(data) => match BlobManager::store(data) {
                Ok(lease) => Self::start_frame_decoder(lease, image_data),
                Err(err) => {
                    log::error!("Unable to move file data to blob manager: {err:#}");
                    Self::placeholder()
                }
            },
            ImageDataType::AnimRgba8 { durations, .. } => {
                let current_frame = if durations.len() > 1 && durations[0].as_millis() == 0 {
                    // Skip possible 0-duration root frame
                    1
                } else {
                    0
                };
                Self {
                    frame_start: RefCell::new(Instant::now()),
                    current_frame: RefCell::new(current_frame),
                    image: Arc::clone(image_data),
                    frames: RefCell::new(None),
                }
            }

            _ => Self {
                frame_start: RefCell::new(Instant::now()),
                current_frame: RefCell::new(0),
                image: Arc::clone(image_data),
                frames: RefCell::new(None),
            },
        }
    }
}

#[cfg(test)]
pub(super) fn frame_state_for_test(rx: Receiver<DecodedFrame>) -> FrameState {
    let (refill_tx, _refill_rx) = sync_channel(2);
    let (_response_tx, response_rx) = sync_channel(2);
    FrameState::new(rx, refill_tx, response_rx)
}

#[cfg(test)]
pub(super) fn decoded_frame_for_test(
    lease: BlobLease,
    duration: Duration,
    width: usize,
    height: usize,
) -> DecodedFrame {
    DecodedFrame {
        lease,
        duration,
        width,
        height,
    }
}

#[cfg(test)]
pub(super) fn ensure_test_storage() {
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| {
        onlyterm_blob_leases::register_storage(Arc::new(
            onlyterm_blob_leases::simple_tempdir::SimpleTempDir::new()
                .expect("create temp blob storage"),
        ))
        .expect("register blob storage");
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decoded_pixel_cache_evicts_old_frames_at_the_byte_budget() {
        let mut cache = DecodedPixelCache::with_limits(8, 2);
        let first = [1u8; 32];
        let second = [2u8; 32];
        let third = [3u8; 32];
        let chunk = 4;

        cache.insert(first, Arc::new(vec![1u8; chunk]));
        cache.insert(second, Arc::new(vec![2u8; chunk]));
        assert_eq!(cache.bytes(), 8);

        cache.insert(third, Arc::new(vec![3u8; chunk]));
        assert!(cache.get(first).is_none());
        assert!(cache.get(second).is_some());
        assert!(cache.get(third).is_some());
        assert_eq!(cache.bytes(), 8);
        assert_eq!(cache.len(), 2);

        let oversized = [9u8; 32];
        cache.insert(oversized, Arc::new(vec![9u8; 9]));
        assert!(cache.get(oversized).is_none());
    }

    #[test]
    fn decoded_pixel_handle_rejects_invalid_dimensions() {
        assert!(DecodedPixelsHandle::new(Arc::new(vec![0u8; 3]), 1, 1).is_err());
        assert!(DecodedPixelsHandle::new(Arc::new(vec![0u8; 4]), 1, 1).is_ok());
    }

    #[test]
    fn decoded_pixel_handle_keeps_the_original_buffer_pointer() {
        let pixels = Arc::new(vec![1u8, 2, 3, 4]);
        let pointer = pixels.as_ptr();
        let handle = DecodedPixelsHandle::new(Arc::clone(&pixels), 1, 1).unwrap();
        assert_eq!(handle.pixels.as_ptr(), pointer);
    }

    #[test]
    fn terminal_refill_failure_is_not_requeued_for_the_same_content() {
        super::ensure_test_storage();
        let (_initial_tx, initial_rx) = sync_channel(1);
        let (request_tx, _request_rx) = sync_channel(2);
        let (_response_tx, response_rx) = response_channel();
        let mut state = FrameState::new(initial_rx, request_tx, response_rx);
        let lease = BlobManager::store(&[1, 2, 3, 4]).unwrap();
        let frame = decoded_frame_for_test(lease, Duration::from_millis(1), 1, 1);
        let key = frame.lease.content_id().as_hash_bytes();
        state.failed_refills.insert(key);
        state.request_refill(&frame);
        assert!(state.failed_refills.contains(&key));
        assert!(!state.pending_refills.contains(&key));
    }

    #[test]
    fn global_refill_executor_reads_a_frame_independently_of_decode_channel() {
        super::ensure_test_storage();
        let lease = BlobManager::store(&[1, 2, 3, 4]).unwrap();
        let key = lease.content_id().as_hash_bytes();
        let (response_tx, response_rx) = response_channel();
        assert!(matches!(
            submit(key, lease, 4, &response_tx),
            SubmitResult::Queued
        ));
        let response = response_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("refill response");
        let (pixels, error) = response.finish();
        assert!(error.is_none());
        assert_eq!(pixels.unwrap().pixels.as_ref(), &[1, 2, 3, 4]);
    }

    #[test]
    fn decoder_poll_returns_immediately_when_no_frame_is_ready() {
        let (_tx, rx) = sync_channel(1);

        assert!(matches!(poll_decoder(&rx), DecoderPoll::Empty));
    }

    #[test]
    fn decoder_poll_reports_completion_without_waiting() {
        let (tx, rx) = sync_channel::<DecodedFrame>(1);
        drop(tx);

        assert!(matches!(poll_decoder(&rx), DecoderPoll::Disconnected));
    }

    #[test]
    fn loading_retry_is_bounded_and_nonzero() {
        assert!(IMAGE_DECODE_POLL_INTERVAL > Duration::ZERO);
        assert!(IMAGE_DECODE_POLL_INTERVAL <= Duration::from_millis(100));
    }

    #[test]
    fn disconnected_decoder_leaves_loading_state() {
        super::ensure_test_storage();
        let (tx, rx) = sync_channel(1);
        drop(tx);
        let mut state = frame_state_for_test(rx);

        assert_eq!(state.load_state, LoadState::Loading);
        assert!(!state.load_next_frame());
        assert_eq!(state.load_state, LoadState::Loaded);
        assert!(matches!(state.source, FrameSource::FrameIndex(0)));
        assert_eq!(state.frames.len(), 1);
    }

    #[test]
    fn disconnected_decoder_resumes_at_frame_zero_without_skipping_it() {
        super::ensure_test_storage();
        let (tx, rx) = sync_channel(2);
        let frame_zero = DecodedFrame {
            lease: BlobManager::store(&[0, 0, 0, 0]).expect("store first frame"),
            duration: Duration::from_millis(100),
            width: 1,
            height: 1,
        };
        let frame_one = DecodedFrame {
            lease: BlobManager::store(&[255, 255, 255, 255]).expect("store second frame"),
            duration: Duration::from_millis(100),
            width: 1,
            height: 1,
        };
        tx.send(frame_zero.clone()).expect("send first frame");
        tx.send(frame_one.clone()).expect("send second frame");

        let mut state = frame_state_for_test(rx);
        assert!(state.load_next_frame());
        assert!(state.load_next_frame());
        assert_eq!(state.current_index, 1);
        drop(tx);

        assert!(!state.load_next_frame());
        assert!(state.load_next_frame());
        assert_eq!(state.current_index, 0);
        assert_eq!(
            state.current_frame.lease.content_id().as_hash_bytes(),
            frame_zero.lease.content_id().as_hash_bytes()
        );
        assert!(state.load_next_frame());
        assert_eq!(state.current_index, 1);
        assert_eq!(
            state.current_frame.lease.content_id().as_hash_bytes(),
            frame_one.lease.content_id().as_hash_bytes()
        );
    }
}
