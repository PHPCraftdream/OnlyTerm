use super::image_decode::IMAGE_DECODE_POLL_INTERVAL;
use super::*;

use crate::renderstate::RenderContext;
use crate::termwindow::render::paint::AllowImage;
use crate::utilsprites::RenderMetrics;
use ::window::bitmaps::atlas::{Atlas, OutOfTextureSpace, Sprite};
use ::window::bitmaps::{BitmapImage, Image, ImageTexture, Texture2d};
use ::window::color::SrgbaPixel;
use ::window::{Point, Rect};
use ahash::RandomState;
use anyhow::Context;
use config::{AllowSquareGlyphOverflow, TextStyle};
use euclid::num::Zero;
use lfucache::LfuCache;
use onlyterm_font::{FontConfiguration, GlyphInfo, LoadedFont};
use onlyterm_term::Underline;
use ordered_float::NotNan;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};
use termwiz::color::RgbColor;
use termwiz::image::{ImageData, ImageDataType};

// AHashMap: HashMap with ahash's RandomState for process-random keys
type AHashMap<K, V> = HashMap<K, V, RandomState>;

fn decoder_poll_deadline(now: Instant) -> Instant {
    now + IMAGE_DECODE_POLL_INTERVAL
}

impl GlyphCache {
    pub fn new_in_memory(fonts: &Rc<FontConfiguration>, size: usize) -> anyhow::Result<Self> {
        let surface: Rc<dyn Texture2d> = Rc::new(ImageTexture::new(size, size));
        let atlas = Atlas::new(&surface).expect("failed to create new texture atlas");

        Ok(Self {
            fonts: Rc::clone(fonts),
            glyph_cache: AHashMap::default(),
            glyph_cache_hit: metrics::histogram!("glyph_cache.glyph_cache.hit.rate"),
            glyph_cache_miss: metrics::histogram!("glyph_cache.glyph_cache.miss.rate"),
            image_cache: LfuCache::new(
                "glyph_cache.image_cache.hit.rate",
                "glyph_cache.image_cache.miss.rate",
                |config| config.glyph_cache_image_cache_size,
                &fonts.config(),
            ),
            frame_cache: HashMap::new(),
            atlas,
            line_glyphs: AHashMap::default(),
            block_glyphs: AHashMap::default(),
            cursor_glyphs: HashMap::new(),
            color: HashMap::new(),
            min_frame_duration: Duration::from_millis(1000 / fonts.config().max_fps),
        })
    }
}

impl GlyphCache {
    pub fn new_gl(
        backend: &RenderContext,
        fonts: &Rc<FontConfiguration>,
        size: usize,
    ) -> anyhow::Result<Self> {
        let surface = backend.allocate_texture_atlas(size)?;
        let atlas = Atlas::new(&surface).expect("failed to create new texture atlas");

        Ok(Self {
            fonts: Rc::clone(fonts),
            glyph_cache: AHashMap::default(),
            glyph_cache_hit: metrics::histogram!("glyph_cache.glyph_cache.hit.rate"),
            glyph_cache_miss: metrics::histogram!("glyph_cache.glyph_cache.miss.rate"),
            image_cache: LfuCache::new(
                "glyph_cache.image_cache.hit.rate",
                "glyph_cache.image_cache.miss.rate",
                |config| config.glyph_cache_image_cache_size,
                &fonts.config(),
            ),
            frame_cache: HashMap::new(),
            atlas,
            line_glyphs: AHashMap::default(),
            block_glyphs: AHashMap::default(),
            cursor_glyphs: HashMap::new(),
            color: HashMap::new(),
            min_frame_duration: Duration::from_millis(1000 / fonts.config().max_fps),
        })
    }
}

impl GlyphCache {
    /// Resolve a glyph from the cache, rendering the glyph on-demand if
    /// the cache doesn't already hold the desired glyph.
    pub fn cached_glyph(
        &mut self,
        info: &GlyphInfo,
        style: &TextStyle,
        followed_by_space: bool,
        font: &Rc<LoadedFont>,
        metrics: &RenderMetrics,
        num_cells: u8,
    ) -> anyhow::Result<Rc<CachedGlyph>> {
        let key = BorrowedGlyphKey {
            font_idx: info.font_idx,
            glyph_pos: info.glyph_pos,
            num_cells,
            style,
            followed_by_space,
            metric: metrics.into(),
            id: font.id(),
        };

        if let Some(entry) = self.glyph_cache.get(&key as &dyn GlyphKeyTrait) {
            self.glyph_cache_hit.record(1.);
            return Ok(Rc::clone(entry));
        }
        self.glyph_cache_miss.record(1.);

        let glyph = match self.load_glyph(info, font, followed_by_space, num_cells) {
            Ok(g) => g,
            Err(err) => {
                if err
                    .root_cause()
                    .downcast_ref::<OutOfTextureSpace>()
                    .is_some()
                {
                    // Ensure that we propagate this signal to expand
                    // our available teexture space
                    return Err(err);
                }

                // But otherwise: don't allow glyph loading errors to propagate,
                // as that will result in incomplete window painting.
                // Log the error and substitute instead.
                log::error!(
                    "load_glyph failed; using blank instead. Error: {:#}. {:?} {:?}",
                    err,
                    info,
                    style
                );
                Rc::new(CachedGlyph {
                    brightness_adjust: 1.0,
                    has_color: false,
                    texture: None,
                    x_advance: PixelLength::zero(),
                    x_offset: PixelLength::zero(),
                    y_offset: PixelLength::zero(),
                    bearing_x: PixelLength::zero(),
                    bearing_y: PixelLength::zero(),
                    scale: 1.0,
                })
            }
        };
        self.glyph_cache.insert(key.to_owned(), Rc::clone(&glyph));
        Ok(glyph)
    }

    pub fn config_changed(&mut self) {
        let config = self.fonts.config();
        self.image_cache.update_config(&config);
        self.cursor_glyphs.clear();
    }

    /// Perform the load and render of a glyph
    #[allow(clippy::float_cmp)]
    fn load_glyph(
        &mut self,
        info: &GlyphInfo,
        font: &Rc<LoadedFont>,
        followed_by_space: bool,
        num_cells: u8,
    ) -> anyhow::Result<Rc<CachedGlyph>> {
        let base_metrics;
        let idx_metrics;
        let brightness_adjust;
        let glyph;

        {
            base_metrics = font.metrics();
            glyph = font.rasterize_glyph(info.glyph_pos, info.font_idx)?;

            idx_metrics = font.metrics_for_idx(info.font_idx)?;
            brightness_adjust = font.brightness_adjust(info.font_idx);
        }

        let aspect = (idx_metrics.cell_width / idx_metrics.cell_height).get();

        // 0.7 is used for this as that is ~ the threshold for \u24e9 on a mac,
        // which is looks squareish and for which it is desirable to allow to
        // overflow.  0.5 is the typical monospace font aspect ratio.
        let is_square_or_wide = aspect >= 0.7;

        let allow_width_overflow = if is_square_or_wide {
            match self.fonts.config().allow_square_glyphs_to_overflow_width {
                AllowSquareGlyphOverflow::Never => false,
                AllowSquareGlyphOverflow::Always => true,
                AllowSquareGlyphOverflow::WhenFollowedBySpace => followed_by_space,
            }
        } else {
            false
        };

        // We shouldn't need to render a glyph that occupies zero cells, but that
        // can happen somehow; see <https://github.com/wezterm/wezterm/issues/1042>
        // so let's treat 0 cells as 1 cell so that we don't try to divide by
        // zero below.
        let num_cells = num_cells.max(1) as f64;

        // Maximum width allowed for this glyph based on its unicode width and
        // the dimensions of a cell
        let max_pixel_width = base_metrics.cell_width.get() * (num_cells + 0.25);

        let scale;

        // This helps to compensate for the !idx_metrics.is_scaled && glyph.is_scaled
        // case which happens when using the harfbuzz rasterizer with a bitmap font.
        // The default value is no compensation.
        let mut metrics_only_scale = 1.0;

        if info.font_idx == 0 {
            // We are the base font
            scale = if allow_width_overflow || glyph.width as f64 <= max_pixel_width {
                1.0
            } else {
                // Scale the glyph to fit in its number of cells
                1.0 / num_cells
            };
        } else if !glyph.is_scaled {
            // A bitmap font that isn't scaled to the requested height.
            let y_scale = base_metrics.cell_height.get() / idx_metrics.cell_height.get();
            let y_scaled_width = y_scale * glyph.width as f64;

            if allow_width_overflow || y_scaled_width <= max_pixel_width {
                // prefer height-wise scaling
                scale = y_scale;
            } else {
                // otherwise just make it fit the width
                scale = max_pixel_width / glyph.width as f64;
            }
        } else {
            // a scalable fallback font

            let f_width = glyph.width as f64;

            if allow_width_overflow || f_width <= max_pixel_width {
                scale = 1.0;
            } else {
                scale = max_pixel_width / f_width;
            }

            if !idx_metrics.is_scaled {
                // A special case: the shaper (eg: harfbuzz) processed
                // a bitmap font (eg: older versions of Noto Color Emoji)
                // to produce shaping info at the bitmap strike size,
                // which is 128 for that font.  The advance is expressed
                // at that size and not at the size of the font.
                // If we get to this condition, the rasterizer used a mode
                // where it has already scaled the glyph, so the dimensions
                // in the bitmap are correct, but the shaper metrics need
                // to be adjusted.
                let y_scale = base_metrics.cell_height.get() / idx_metrics.cell_height.get();
                metrics_only_scale = y_scale;
            }

            #[cfg(debug_assertions)]
            {
                log::debug!(
                    "{text} allow_width_overflow={allow_width_overflow} \
                     is_square_or_wide={is_square_or_wide} aspect={aspect} \
                     max_pixel_width={max_pixel_width} glyph.width={glyph_width} \
                     -> scale={scale} metrics_only_scale={metrics_only_scale}",
                    text = info.text,
                    glyph_width = glyph.width,
                );
            }
        };

        let descender_adjust = if info.font_idx == 0 {
            PixelLength::new(0.0)
        } else {
            idx_metrics.force_y_adjust
        };

        let (cell_width, cell_height) = (base_metrics.cell_width, base_metrics.cell_height);

        let glyph = if glyph.width == 0 || glyph.height == 0 {
            // a whitespace glyph
            CachedGlyph {
                brightness_adjust: 1.0,
                has_color: glyph.has_color,
                texture: None,
                x_offset: info.x_offset * scale,
                y_offset: info.y_offset * scale,
                x_advance: info.x_advance * scale,
                bearing_x: PixelLength::zero(),
                bearing_y: descender_adjust,
                scale,
            }
        } else {
            let raw_im =
                Image::with_rgba32(glyph.width, glyph.height, 4 * glyph.width, &glyph.data);

            let bearing_x = glyph.bearing_x * scale * metrics_only_scale;
            // No metrics_only_scale adjustment to bearing_y is needed because
            // the value comes from the rasterized glyph and not from the
            // shaper stage.
            let bearing_y = descender_adjust + (glyph.bearing_y * scale);
            let x_offset = info.x_offset * scale * metrics_only_scale;
            let y_offset = info.y_offset * scale * metrics_only_scale;
            let x_advance = info.x_advance * scale * metrics_only_scale;

            log::trace!(
                "bearing_x={bearing_x:?} bearing_y={bearing_y:?} \
                 x_offset={x_offset:?} y_offset={y_offset:?} x_advance={x_advance:?}"
            );

            let (scale, raw_im) = if scale != 1.0 {
                log::trace!(
                    "physically scaling {:?} by {} bcos {}x{} > {:?}x{:?}. aspect={}",
                    info,
                    scale,
                    glyph.width,
                    glyph.height,
                    cell_width,
                    cell_height,
                    aspect,
                );
                (1.0, raw_im.scale_by(scale))
            } else {
                (scale, raw_im)
            };

            let tex = self.atlas.allocate(&raw_im)?;

            let g = CachedGlyph {
                brightness_adjust,
                has_color: glyph.has_color,
                texture: Some(tex),
                x_offset,
                y_offset,
                x_advance,
                bearing_x,
                bearing_y,
                scale,
            };

            if info.font_idx != 0 {
                // It's generally interesting to examine eg: emoji or ligatures
                // that we might have fallen back to
                log::trace!("{:?} {:?}", info, g);
            }

            g
        };

        Ok(Rc::new(glyph))
    }

    fn cached_image_impl(
        frame_cache: &mut HashMap<[u8; 32], Sprite>,
        atlas: &mut Atlas,
        decoded: &DecodedImage,
        padding: Option<usize>,
        min_frame_duration: Duration,
        allow_image: AllowImage,
    ) -> anyhow::Result<(Sprite, Option<Instant>, LoadState)> {
        let mut handle = DecodedImageHandle {
            h: decoded.image.data(),
            current_frame: *decoded.current_frame.borrow(),
        };

        let scale_down = match allow_image {
            AllowImage::Scale(n) => Some(n),
            _ => None,
        };

        match &*handle.h {
            ImageDataType::Rgba8 { hash, .. } => {
                if let Some(sprite) = frame_cache.get(hash) {
                    return Ok((sprite.clone(), None, LoadState::Loaded));
                }
                let sprite = atlas
                    .allocate_with_padding(&handle, padding, scale_down)
                    .context("atlas.allocate_with_padding")?;
                frame_cache.insert(*hash, sprite.clone());

                Ok((sprite, None, LoadState::Loaded))
            }
            ImageDataType::AnimRgba8 {
                hashes,
                frames,
                durations,
                ..
            } => {
                let mut next = None;
                let mut decoded_frame_start = decoded.frame_start.borrow_mut();
                let mut decoded_current_frame = decoded.current_frame.borrow_mut();
                if frames.len() > 1 {
                    let now = Instant::now();

                    // We round up the frame duration to at least the minimum
                    // frame duration that onlyterm can use when rendering.
                    // There's no point trying to deal with smaller intervals
                    // because we simply cannot render them without dropping
                    // frames.
                    // In addition, with a 1ms frame delay, there's a good chance
                    // that any given cell may switch to a different frame from
                    // its neighbor while we are rendering the entire terminal
                    // frame, so we want to avoid that.
                    // <https://github.com/wezterm/wezterm/issues/3260>
                    let mut next_due = *decoded_frame_start
                        + durations[*decoded_current_frame].max(min_frame_duration);
                    if now >= next_due {
                        // Advance to next frame
                        *decoded_current_frame += 1;
                        if *decoded_current_frame >= frames.len() {
                            *decoded_current_frame = 0;
                            // Skip potential 0-duration root frame
                            if durations[0].as_millis() == 0 && frames.len() > 1 {
                                *decoded_current_frame += 1;
                            }
                        }
                        *decoded_frame_start = now;
                        next_due = *decoded_frame_start
                            + durations[*decoded_current_frame].max(min_frame_duration);
                        handle.current_frame = *decoded_current_frame;
                    }

                    next.replace(next_due);
                }

                let hash = hashes[*decoded_current_frame];

                if let Some(sprite) = frame_cache.get(&hash) {
                    return Ok((sprite.clone(), next, LoadState::Loaded));
                }

                let sprite = atlas
                    .allocate_with_padding(&handle, padding, scale_down)
                    .context("atlas.allocate_with_padding")?;

                frame_cache.insert(hash, sprite.clone());

                Ok((
                    sprite,
                    Some(
                        *decoded_frame_start
                            + durations[*decoded_current_frame].max(min_frame_duration),
                    ),
                    LoadState::Loaded,
                ))
            }
            ImageDataType::EncodedLease(_) | ImageDataType::EncodedFile(_) => {
                let mut frames = decoded.frames.borrow_mut();
                let frames = frames.as_mut().expect("to have frames");

                let mut decoded_frame_start = decoded.frame_start.borrow_mut();
                let mut decoded_current_frame = decoded.current_frame.borrow_mut();

                let now = Instant::now();
                let was_loading = frames.load_state == LoadState::Loading;
                if was_loading {
                    // The decoder runs on its own thread. Polling the bounded
                    // channel keeps the GUI responsive while the first frame
                    // is being decoded; the caller schedules the next repaint
                    // from the returned deadline below.
                    frames.load_next_frame();
                }

                let mut next = None;
                if frames.load_state == LoadState::Loading {
                    next.replace(decoder_poll_deadline(now));
                } else {
                    if was_loading {
                        // The first frame became available during this poll.
                        // Start its animation clock when it is actually shown,
                        // rather than counting decoder time as display time.
                        *decoded_current_frame = 0;
                        *decoded_frame_start = now;
                        handle.current_frame = 0;
                    }

                    // We round up the frame duration to at least the minimum
                    // frame duration that onlyterm can use when rendering.
                    // There's no point trying to deal with smaller intervals
                    // because we simply cannot render them without dropping
                    // frames.
                    // In addition, with a 1ms frame delay, there's a good chance
                    // that any given cell may switch to a different frame from
                    // its neighbor while we are rendering the entire terminal
                    // frame, so we want to avoid that.
                    // <https://github.com/wezterm/wezterm/issues/3260>
                    let mut next_due =
                        *decoded_frame_start + frames.frame_duration().max(min_frame_duration);
                    if !was_loading && now >= next_due {
                        // Advance to next frame
                        if frames.load_next_frame() {
                            *decoded_current_frame += 1;
                            *decoded_frame_start = now;
                            next_due = *decoded_frame_start
                                + frames.frame_duration().max(min_frame_duration);
                            handle.current_frame = *decoded_current_frame;
                        } else {
                            // An animated decoder can finish the first frame
                            // before the next one is available. Keep the GUI
                            // responsive while waiting for that bounded
                            // channel without repeatedly scheduling an
                            // already-expired deadline.
                            next_due = decoder_poll_deadline(now);
                        }
                    }

                    next.replace(next_due);
                }

                let hash = frames.frame_hash();

                if let Some(sprite) = frame_cache.get(&hash) {
                    return Ok((sprite.clone(), next, frames.load_state));
                }

                let expected_byte_size =
                    frames.current_frame.width * frames.current_frame.height * 4;

                let frame_data = match frames.current_frame.lease.get_data() {
                    Ok(data) => {
                        // If the size isn't right, ignore this frame and replace
                        // it with a blank one instead. This might happen if
                        // some process is truncating the files, or perhaps if
                        // the disk is full.
                        // We need to check for this because the consequence of
                        // a mismatched size is a panic in a layer where we
                        // cannot handle the error case.
                        if data.len() != expected_byte_size {
                            report_frame_error(format!("frame data is corrupted: expected size {expected_byte_size} but have {}", data.len()));
                            vec![0u8; expected_byte_size]
                        } else {
                            data
                        }
                    }
                    Err(err) => {
                        report_frame_error(format!("frame data error: {err:#}"));
                        vec![0u8; expected_byte_size]
                    }
                };

                let frame = Image::from_raw(
                    frames.current_frame.width,
                    frames.current_frame.height,
                    frame_data,
                );
                let sprite = atlas.allocate_with_padding(&frame, padding, scale_down)?;

                frame_cache.insert(hash, sprite.clone());

                Ok((sprite, next, frames.load_state))
            }
        }
    }

    pub fn cached_image(
        &mut self,
        image_data: &Arc<ImageData>,
        padding: Option<usize>,
        allow_image: AllowImage,
    ) -> anyhow::Result<(Sprite, Option<Instant>, LoadState)> {
        let hash = image_data.hash();

        if let Some(decoded) = self.image_cache.get(&hash) {
            Self::cached_image_impl(
                &mut self.frame_cache,
                &mut self.atlas,
                decoded,
                padding,
                self.min_frame_duration,
                allow_image,
            )
        } else {
            let decoded = DecodedImage::load(image_data);
            let res = Self::cached_image_impl(
                &mut self.frame_cache,
                &mut self.atlas,
                &decoded,
                padding,
                self.min_frame_duration,
                allow_image,
            )?;
            self.image_cache.put(hash, decoded);
            Ok(res)
        }
    }

    pub fn cached_color(&mut self, color: RgbColor, alpha: f32) -> anyhow::Result<Sprite> {
        let key = (color, NotNan::new(alpha).unwrap());

        if let Some(s) = self.color.get(&key) {
            return Ok(s.clone());
        }

        let (red, green, blue) = color.to_tuple_rgb8();
        let alpha = (alpha * 255.0) as u8;

        let data = vec![
            red, green, blue, alpha, red, green, blue, alpha, red, green, blue, alpha, red, green,
            blue, alpha,
        ];
        let image = Image::from_raw(2, 2, data);

        let sprite = self.atlas.allocate(&image)?;
        self.color.insert(key, sprite.clone());
        Ok(sprite)
    }

    pub fn cached_block(
        &mut self,
        block: BlockKey,
        metrics: &RenderMetrics,
    ) -> anyhow::Result<Sprite> {
        let key = SizedBlockKey {
            block,
            size: metrics.into(),
        };
        if let Some(s) = self.block_glyphs.get(&key) {
            return Ok(s.clone());
        }
        self.block_sprite(metrics, key)
    }

    fn line_sprite(&mut self, key: LineKey, metrics: &RenderMetrics) -> anyhow::Result<Sprite> {
        let mut buffer = Image::new(
            metrics.cell_size.width as usize,
            metrics.cell_size.height as usize,
        );
        let black = SrgbaPixel::rgba(0, 0, 0, 0);
        let white = SrgbaPixel::rgba(0xff, 0xff, 0xff, 0xff);

        let cell_rect = Rect::new(Point::new(0, 0), metrics.cell_size);

        let draw_single = |buffer: &mut Image| {
            for row in 0..metrics.underline_height {
                buffer.draw_line(
                    Point::new(
                        cell_rect.origin.x,
                        cell_rect.origin.y + metrics.descender_row + row,
                    ),
                    Point::new(
                        cell_rect.origin.x + metrics.cell_size.width,
                        cell_rect.origin.y + metrics.descender_row + row,
                    ),
                    white,
                );
            }
        };

        let draw_dotted = |buffer: &mut Image| {
            for row in 0..metrics.underline_height {
                let y = (cell_rect.origin.y + metrics.descender_row + row) as usize;
                if y >= metrics.cell_size.height as usize {
                    break;
                }

                let mut color = white;
                let segment_length = (metrics.cell_size.width / 4) as usize;
                let mut count = segment_length;
                let range =
                    buffer.horizontal_pixel_range_mut(0, metrics.cell_size.width as usize, y);
                for c in range.iter_mut() {
                    *c = color.as_srgba32();
                    count -= 1;
                    if count == 0 {
                        color = if color == white { black } else { white };
                        count = segment_length;
                    }
                }
            }
        };

        let draw_dashed = |buffer: &mut Image| {
            for row in 0..metrics.underline_height {
                let y = (cell_rect.origin.y + metrics.descender_row + row) as usize;
                if y >= metrics.cell_size.height as usize {
                    break;
                }
                let mut color = white;
                let third = (metrics.cell_size.width / 3) as usize + 1;
                let mut count = third;
                let range =
                    buffer.horizontal_pixel_range_mut(0, metrics.cell_size.width as usize, y);
                for c in range.iter_mut() {
                    *c = color.as_srgba32();
                    count -= 1;
                    if count == 0 {
                        color = if color == white { black } else { white };
                        count = third;
                    }
                }
            }
        };

        let draw_curly = |buffer: &mut Image| {
            let max_y = metrics.cell_size.height as usize - 1;
            let x_factor = (2. * std::f32::consts::PI) / metrics.cell_size.width as f32;

            // Have the wave go from the descender to the bottom of the cell
            let wave_height =
                metrics.cell_size.height - (cell_rect.origin.y + metrics.descender_row);

            let half_height = (wave_height as f32 / 4.).max(1.);
            let y = ((cell_rect.origin.y + metrics.descender_row) as usize)
                .saturating_sub(half_height as usize);

            fn add(x: usize, y: usize, val: u8, max_y: usize, buffer: &mut Image) {
                let y = y.min(max_y);
                let pixel = buffer.pixel_mut(x, y);
                let (current, _, _, _) = SrgbaPixel::with_srgba_u32(*pixel).as_rgba();
                let value = current.saturating_add(val);
                *pixel = SrgbaPixel::rgba(value, value, value, value).as_srgba32();
            }

            for x in 0..metrics.cell_size.width as usize {
                let vertical = -half_height * (x as f32 * x_factor).sin() + half_height;
                let v1 = vertical.floor();
                let v2 = vertical.ceil();

                for row in 0..metrics.underline_height as usize {
                    let value = (255. * (vertical - v1).abs()) as u8;
                    add(
                        x,
                        row.saturating_add(y).saturating_add(v1 as usize),
                        255u8.saturating_sub(value),
                        max_y,
                        buffer,
                    );
                    add(
                        x,
                        row.saturating_add(y).saturating_add(v2 as usize),
                        value,
                        max_y,
                        buffer,
                    );
                }
            }
        };

        let draw_double = |buffer: &mut Image| {
            let first_line = metrics
                .descender_row
                .min(metrics.descender_plus_two - 2 * metrics.underline_height);

            for row in 0..metrics.underline_height {
                buffer.draw_line(
                    Point::new(cell_rect.origin.x, cell_rect.origin.y + first_line + row),
                    Point::new(
                        cell_rect.origin.x + metrics.cell_size.width,
                        cell_rect.origin.y + first_line + row,
                    ),
                    white,
                );
                buffer.draw_line(
                    Point::new(
                        cell_rect.origin.x,
                        cell_rect.origin.y + metrics.descender_plus_two + row,
                    ),
                    Point::new(
                        cell_rect.origin.x + metrics.cell_size.width,
                        cell_rect.origin.y + metrics.descender_plus_two + row,
                    ),
                    white,
                );
            }
        };

        let draw_strike = |buffer: &mut Image| {
            for row in 0..metrics.underline_height {
                buffer.draw_line(
                    Point::new(
                        cell_rect.origin.x,
                        cell_rect.origin.y + metrics.strike_row + row,
                    ),
                    Point::new(
                        cell_rect.origin.x + metrics.cell_size.width,
                        cell_rect.origin.y + metrics.strike_row + row,
                    ),
                    white,
                );
            }
        };

        let draw_overline = |buffer: &mut Image| {
            for row in 0..metrics.underline_height {
                buffer.draw_line(
                    Point::new(cell_rect.origin.x, cell_rect.origin.y + row),
                    Point::new(
                        cell_rect.origin.x + metrics.cell_size.width,
                        cell_rect.origin.y + row,
                    ),
                    white,
                );
            }
        };

        buffer.clear_rect(cell_rect, black);
        if key.overline {
            draw_overline(&mut buffer);
        }
        match key.underline {
            Underline::None => {}
            Underline::Single => draw_single(&mut buffer),
            Underline::Curly => draw_curly(&mut buffer),
            Underline::Dashed => draw_dashed(&mut buffer),
            Underline::Dotted => draw_dotted(&mut buffer),
            Underline::Double => draw_double(&mut buffer),
        }
        if key.strike_through {
            draw_strike(&mut buffer);
        }
        let sprite = self.atlas.allocate(&buffer)?;
        self.line_glyphs.insert(key, sprite.clone());
        Ok(sprite)
    }

    /// Figure out what we're going to draw for the underline.
    /// If the current cell is part of the current URL highlight
    /// then we want to show the underline.
    pub fn cached_line_sprite(
        &mut self,
        is_highlited_hyperlink: bool,
        is_strike_through: bool,
        underline: Underline,
        overline: bool,
        metrics: &RenderMetrics,
    ) -> anyhow::Result<Sprite> {
        let effective_underline = match (is_highlited_hyperlink, underline) {
            (true, Underline::None) => Underline::Single,
            (true, Underline::Single) => Underline::Double,
            (true, _) => Underline::Single,
            (false, u) => u,
        };

        let key = LineKey {
            strike_through: is_strike_through,
            overline,
            underline: effective_underline,
            size: metrics.into(),
        };

        if let Some(s) = self.line_glyphs.get(&key) {
            return Ok(s.clone());
        }

        self.line_sprite(key, metrics)
    }
}

#[cfg(test)]
mod tests {
    use super::super::image_decode::FrameDecoder;
    use super::*;

    /// Exercise the rewritten `ImageFormat::WebP` branch in `FrameDecoder`.
    ///
    /// Generates a small RGBA image, encodes it as WebP, stores it through
    /// `BlobManager`, then runs the real `FrameDecoder::start` pipeline and
    /// asserts the produced frame has the expected dimensions and a full RGBA8
    /// buffer. This verifies the `image_webp::WebPDecoder`-based branch produces
    /// valid `Frames` without panicking.
    ///
    /// Note: the `image` and `image-webp` crates ship only a *lossless* encoder
    /// (lossy encoding needs `libwebp`, which is not a dependency here), so the
    /// generated bytes are lossless VP8L; the new `image_webp` decoder handles
    /// both lossless and lossy streams, and this test covers the branch wiring
    /// end-to-end.
    #[test]
    fn webp_decoding_produces_valid_frame() {
        let w = 4u32;
        let h = 4u32;
        let rgba: Vec<u8> = (0..(w * h * 4)).map(|i| (i % 256) as u8).collect();

        let mut webp_bytes = Vec::new();
        image::codecs::webp::WebPEncoder::new_lossless(&mut webp_bytes)
            .encode(&rgba, w, h, image::ExtendedColorType::Rgba8)
            .expect("encode lossless webp");
        assert!(
            webp_bytes.starts_with(b"RIFF"),
            "encoded bytes should be a RIFF/WebP container"
        );

        // FrameDecoder relies on BlobManager storage; register a temp backend.
        super::super::image_decode::ensure_test_storage();

        let lease =
            onlyterm_blob_leases::BlobManager::store(&webp_bytes).expect("store webp bytes");
        let rx = FrameDecoder::start(lease).expect("start decoder");

        let frame = rx
            .recv_timeout(Duration::from_secs(15))
            .expect("receive decoded frame within timeout");

        assert_eq!(frame.width, w as usize);
        assert_eq!(frame.height, h as usize);

        let decoded = frame.lease.get_data().expect("read decoded frame data");
        assert_eq!(
            decoded.len(),
            (w * h * 4) as usize,
            "decoded frame should be RGBA8"
        );
    }

    #[test]
    fn decoder_poll_deadline_is_strictly_after_now() {
        let now = Instant::now();
        let deadline = decoder_poll_deadline(now);

        assert_eq!(deadline.duration_since(now), IMAGE_DECODE_POLL_INTERVAL);
        assert!(deadline > now);
    }
}
