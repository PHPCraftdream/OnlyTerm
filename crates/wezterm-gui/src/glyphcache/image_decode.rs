use super::*;

use ::window::bitmaps::BitmapImage;
use anyhow::Context;
use image::{
    AnimationDecoder, DynamicImage, Frame, Frames, ImageDecoder, ImageFormat, ImageResult, Limits,
};
use std::cell::RefCell;
use std::io::Seek;
use std::sync::mpsc::{sync_channel, Receiver, RecvTimeoutError, SyncSender, TryRecvError};
use std::sync::{Arc, LazyLock, MutexGuard};
use std::time::{Duration, Instant};
use termwiz::image::{ImageData, ImageDataType};
use wezterm_blob_leases::{BlobLease, BlobManager, BoxedReader};

/// A helper struct to implement BitmapImage for ImageDataType while
/// holding the mutex for the sake of safety.
pub(super) struct DecodedImageHandle<'a> {
    pub(super) current_frame: usize,
    pub(super) h: MutexGuard<'a, ImageDataType>,
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

pub(super) struct FrameDecoder {}

impl FrameDecoder {
    pub fn start(lease: BlobLease) -> anyhow::Result<Receiver<DecodedFrame>> {
        let (tx, rx) = sync_channel(2);

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

        Ok(rx)
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
                let frame_count = if decoder.is_animated() {
                    decoder.num_frames()
                } else {
                    1
                };

                let raw_len = decoder
                    .output_buffer_size()
                    .context("Invalid buffer size")?;
                let mut raw_buf = vec![0u8; raw_len];

                let mut frames_vec: Vec<ImageResult<Frame>> =
                    Vec::with_capacity(frame_count as usize);

                for _ in 0..frame_count {
                    let rgba_image = if !decoder.has_alpha() {
                        image::RgbImage::from_raw(width, height, raw_buf.clone())
                            .map(|img_buf| image::DynamicImage::ImageRgb8(img_buf).to_rgba8())
                    } else {
                        image::RgbaImage::from_raw(width, height, raw_buf.clone())
                    };

                    if let Some(frame) = rgba_image {
                        let delay_ms = if decoder.is_animated() {
                            decoder
                                .read_frame(&mut raw_buf)
                                .context("reading WebP frame")?
                        } else {
                            decoder
                                .read_image(&mut raw_buf)
                                .context("reading static WebP image")?;
                            u32::MAX
                        };
                        let delay = image::Delay::from_numer_denom_ms(delay_ms, 1);

                        frames_vec.push(Ok(Frame::from_parts(frame, 0, 0, delay)));
                    }
                }

                Frames::new(Box::new(frames_vec.into_iter()))
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

        let mut decoded_frames = vec![];
        let (width, height) = frame.buffer().dimensions();
        let width = width as usize;
        let height = height as usize;

        let duration: Duration = frame.delay().into();
        log::debug!("first frame took {:?} to decode.", start.elapsed());

        let data = frame.into_buffer().into_raw();
        let lease = BlobManager::store(&data).context("BlobManager::store")?;
        let decoded_frame = DecodedFrame {
            lease,
            duration,
            width,
            height,
        };
        tx.send(decoded_frame.clone())
            .context("sending first frame")?;
        decoded_frames.push(decoded_frame);

        for frame in frames.by_ref() {
            let frame = frame?;

            let duration: Duration = frame.delay().into();
            let data = frame.into_buffer().into_raw();
            let lease = BlobManager::store(&data).context("BlobManager::store")?;

            let decoded_frame = DecodedFrame {
                lease,
                duration,
                width,
                height,
            };
            tx.send(decoded_frame.clone()).context("sending a frame")?;
            decoded_frames.push(decoded_frame);
        }

        drop(frames);

        let elapsed = start.elapsed();
        let fps = decoded_frames.len() as f32 / elapsed.as_secs_f32();

        log::debug!(
            "decoded {} frames, {} bytes in {elapsed:?}, {fps} fps",
            decoded_frames.len(),
            decoded_frames.len() * width * height * 4
        );
        Ok(())
    }
}

pub(super) enum FrameSource {
    Decoder(Receiver<DecodedFrame>),
    FrameIndex(usize),
}

pub(super) struct FrameState {
    pub(super) source: FrameSource,
    pub(super) current_frame: DecodedFrame,
    pub(super) frames: Vec<DecodedFrame>,
    pub(super) load_state: LoadState,
}

impl FrameState {
    fn new(rx: Receiver<DecodedFrame>) -> Self {
        const BLACK_SIZE: usize = 8;
        static BLACK: LazyLock<BlobLease> = LazyLock::new(|| {
            let mut data = vec![];
            for _ in 0..BLACK_SIZE * BLACK_SIZE {
                data.extend_from_slice(&[0, 0, 0, 0xff]);
            }
            BlobManager::store(&data).unwrap()
        });

        Self {
            source: FrameSource::Decoder(rx),
            frames: vec![],
            current_frame: DecodedFrame {
                lease: BLACK.clone(),
                width: BLACK_SIZE,
                height: BLACK_SIZE,
                duration: Duration::from_millis(0),
            },
            load_state: LoadState::Loading,
        }
    }

    pub(super) fn wait_for_first_frame(&mut self, duration: Duration) {
        if !self.frames.is_empty() {
            // Already decoded the first frame
            return;
        }

        match &mut self.source {
            FrameSource::Decoder(rx) => match rx.recv_timeout(duration) {
                Ok(frame) => {
                    self.frames.push(frame.clone());
                    self.current_frame = frame;
                    self.load_state = LoadState::Loaded;
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    self.source = FrameSource::FrameIndex(0);
                    log::warn!("image decoder thread terminated");
                    self.current_frame.duration = Duration::from_secs(86400);
                    self.frames.push(self.current_frame.clone());
                }
            },
            FrameSource::FrameIndex(_) => {}
        }
    }

    pub(super) fn load_next_frame(&mut self) -> bool {
        match &mut self.source {
            FrameSource::Decoder(rx) => match rx.try_recv() {
                Ok(frame) => {
                    self.frames.push(frame.clone());
                    self.current_frame = frame;
                    self.load_state = LoadState::Loaded;
                    true
                }
                Err(TryRecvError::Empty) => false,
                Err(TryRecvError::Disconnected) => {
                    self.source = FrameSource::FrameIndex(0);
                    if self.frames.is_empty() {
                        log::warn!("image decoder thread terminated");
                        self.current_frame.duration = Duration::from_secs(86400);
                        self.frames.push(self.current_frame.clone());
                        false
                    } else if self.frames.len() == 1 {
                        // If there's only a single frame, we may as well ensure
                        // that it has a long duration so that we don't waste
                        // resources ticking to the same frame over and over
                        self.frames[0].duration = Duration::from_secs(86400);
                        true
                    } else {
                        true
                    }
                }
            },
            FrameSource::FrameIndex(idx) => {
                *idx += 1;
                if *idx >= self.frames.len() {
                    *idx = 0;
                }
                self.current_frame = self.frames[*idx].clone();
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
            Ok(rx) => Self {
                frame_start: RefCell::new(Instant::now()),
                current_frame: RefCell::new(0),
                image: Arc::clone(image_data),
                frames: RefCell::new(Some(FrameState::new(rx))),
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
