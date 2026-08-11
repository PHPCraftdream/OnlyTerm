use crate::colorease::ColorEase;
use crate::customglyph::{BlockKey, *};
use crate::glyphcache::{CachedGlyph, GlyphCache};
use crate::quad::{
    HeapQuadAllocator, QuadAllocator, QuadImpl, QuadTrait, TripleLayerQuadAllocator,
    TripleLayerQuadAllocatorTrait,
};
use crate::shapecache::*;
use crate::termwindow::render::paint::AllowImage;
use crate::termwindow::{BorrowedShapeCacheKey, RenderState, ShapedInfo, TermWindowNotif};
use crate::utilsprites::RenderMetrics;
use ::window::bitmaps::{TextureCoord, TextureRect, TextureSize};
use ::window::{DeadKeyStatus, PointF, RectF, SizeF, WindowOps};
use anyhow::{anyhow, Context};
use config::{
    BoldBrightening, ConfigHandle, DimensionContext, HorizontalWindowContentAlignment, TextStyle,
    VerticalWindowContentAlignment, VisualBellTarget,
};
use euclid::num::Zero;
use mux::pane::{CachePolicy, Pane, PaneId};
use mux::renderable::{RenderableDimensions, StableCursorPosition};
use ordered_float::NotNan;
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;
use termwiz::cellcluster::CellCluster;
use termwiz::hyperlink::Hyperlink;
use termwiz::surface::{CursorShape, CursorVisibility, SequenceNo};
use wezterm_font::shaper::PresentationWidth;
use wezterm_font::units::{IntPixelLength, PixelLength};
use wezterm_font::{ClearShapeCache, GlyphInfo, LoadedFont};
use wezterm_term::color::{ColorAttribute, ColorPalette};
use wezterm_term::{CellAttributes, Line, StableRowIndex};
use window::color::LinearRgba;

pub mod borders;
pub mod budget;
pub mod corners;
pub mod draw;
pub mod fancy_tab_bar;
pub mod paint;
pub mod pane;
pub mod screen_line;
pub mod split;
pub mod tab_bar;
pub mod window_buttons;

/// The data that we associate with a line; we use this to cache it shape hash
/// Task #439: DEPRECATED - replaced by ShapeHashEntry + shape_hash_cache.
/// Kept for now to avoid structural changes; will be cleaned up.
#[derive(Debug)]
#[allow(dead_code)]
pub struct CachedLineState {
    pub id: u64,
    pub seqno: SequenceNo,
    pub shape_hash: [u8; 16],
}

/// Task #439: Shape hash cache entry keyed by pane_id and stable_row.
/// This survives Line cloning because it's owned by TermWindow, not the Line.
#[derive(Debug, Clone)]
pub struct ShapeHashEntry {
    pub seqno: SequenceNo,
    pub shape_hash: [u8; 16],
}

/// Task #439: Cache key for shape hash - pane_id and stable_row are stable
/// across clones and won't change for a given logical line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ShapeHashCacheKey {
    pub pane_id: PaneId,
    pub stable_row: StableRowIndex,
}

#[derive(Debug, Hash, Clone, PartialEq, Eq)]
pub struct LineQuadCacheKey {
    pub config_generation: usize,
    pub shape_generation: usize,
    pub quad_generation: usize,
    /// Only set if cursor.y == stable_row
    pub composing: Option<String>,
    pub selection: Range<usize>,
    pub shape_hash: [u8; 16],
    // Position-independent: lines are built at origin (0,0) and translated
    // at emission time, so absolute screen position and physical line index
    // are no longer part of the cache key. This enables cache hits during scrolling.
    pub pane_id: PaneId,
    pub pane_is_active: bool,
    /// A cursor position with the y value fixed at 0.
    /// Only is_some() if the y value matches this row.
    pub cursor: Option<CursorProperties>,
    pub reverse_video: bool,
    pub password_input: bool,
    pub is_wrap_continuation: bool,
    /// Whether `disable_bidi_for_processes_named` currently suppresses bidi
    /// for this line's pane (see `bidi_disabled_by_foreground_process`).
    /// This is derived from the pane's *live* foreground process, which can
    /// change without the line's own content (and thus `shape_hash`)
    /// changing at all -- e.g. the same scrollback line redrawn before and
    /// after `claude` exits back to a shell prompt. Without this in the
    /// key, a stale cached quad from one state would incorrectly get
    /// reused after the foreground process changes.
    pub bidi_process_override: bool,
}

#[derive(Clone, Debug, Default)]
pub struct LineQuadCacheValue {
    pub expires: Option<Instant>,
    pub layers: Rc<HeapQuadAllocator>,
    // Only set if the line contains any hyperlinks, so
    // that we can invalidate when it changes
    pub current_highlight: Option<Arc<Hyperlink>>,
    pub invalidate_on_hover_change: bool,
}

/// What was last actually emitted for each visible row slot of a pane.
/// Not a content cache (that's `line_quad_cache`, keyed by content) --
/// this answers "what is currently on screen at visual row N", which is
/// the only thing that can be re-emitted when the frame-build budget
/// defers a row's rebuild. See task #457 / the @oh design review this
/// implements.
pub struct RetainedPaneRows {
    /// Fail-safe stamp: everything that would invalidate the *pixels* of
    /// a retained row without necessarily changing that row's text
    /// content. Compared once per pane per frame; on any mismatch the
    /// whole map for this pane is dropped (safe default: falls back to
    /// Cause-A-vulnerable behavior only for one frame, not silently wrong
    /// pixels). Deliberately a coarse stamp rather than requiring every
    /// invalidation call site to remember to bump `retained_rows` too --
    /// a missed call site there would render garbage, not just blank.
    pub stamp: RetainedStamp,
    /// Indexed by visible row slot (`line_idx + pos.top`), NOT by
    /// StableRowIndex -- this must track screen position, not scrollback
    /// content position.
    pub rows: Vec<Option<RetainedRow>>,
    /// Row slot at which the NEXT frame's sweep should start spending its
    /// shaping budget. 0 means "a full clean sweep completed; start from
    /// the top again".
    pub resume_row: usize,
}

/// A retained row's quad data and its expiration (if animated).
#[derive(Clone, Debug)]
pub struct RetainedRow {
    pub quads: Rc<HeapQuadAllocator>,
    pub expires: Option<Instant>,
}

/// Fail-safe stamp for retained rows - anything that would invalidate the
/// *pixels* of a retained row without necessarily changing its text content.
/// Compared once per pane per frame; on any mismatch, the whole map for this
/// pane is dropped (safe default).
#[derive(Clone, PartialEq)]
pub struct RetainedStamp {
    pub config_generation: usize,
    pub shape_generation: usize,
    pub quad_generation: usize,
    pub pixel_width: usize,
    pub pixel_height: usize,
    pub cell_height: isize,
    // Retained quads are now built at origin (0,0) and translated at emission
    // time, so these track pane origin movement rather than pixel staleness.
    // They still invalidate retained rows when the pane resizes or moves.
    pub left_pixel_x: NotNan<f32>,
    pub top_pixel_y: NotNan<f32>,
    pub num_rows: usize,
    pub num_cols: usize,
}

pub struct LineToElementParams<'a> {
    pub line: &'a Line,
    pub config: &'a ConfigHandle,
    pub palette: &'a ColorPalette,
    pub window_is_transparent: bool,
    pub reverse_video: bool,
    pub shape_key: &'a Option<LineToEleShapeCacheKey>,
    pub is_wrap_continuation: bool,
    /// See `LineQuadCacheKey::bidi_process_override`.
    pub bidi_process_override: bool,
}

#[derive(Debug, Hash, PartialEq, Eq, Clone)]
pub struct LineToEleShapeCacheKey {
    pub shape_hash: [u8; 16],
    pub composing: Option<(usize, String)>,
    pub shape_generation: usize,
    pub is_wrap_continuation: bool,
    /// See `LineQuadCacheKey::bidi_process_override`.
    pub bidi_process_override: bool,
}

pub struct LineToElementShapeItem {
    pub expires: Option<Instant>,
    pub shaped: Rc<Vec<LineToElementShape>>,
    // Only set if the line contains any hyperlinks, so
    // that we can invalidate when it changes
    pub current_highlight: Option<Arc<Hyperlink>>,
    pub invalidate_on_hover_change: bool,
}

pub struct LineToElementShape {
    pub underline_tex_rect: TextureRect,
    pub fg_color: LinearRgba,
    pub bg_color: LinearRgba,
    pub underline_color: LinearRgba,
    pub x_pos: f32,
    pub pixel_width: f32,
    pub glyph_info: Rc<Vec<ShapedInfo>>,
    pub cluster: CellCluster,
    /// Column this cluster is actually drawn at, counted from the start
    /// of the line. This is NOT `cluster.first_cell_idx`: a Hebrew phrase
    /// gets reordered within the line, so its clusters' logical cell
    /// indices run backwards while they are still painted one after
    /// another from the start of the line. Glyphs are positioned by
    /// accumulating widths in exactly this order, so anything else drawn
    /// per-cluster (backgrounds, underlines) has to use the same counter
    /// or it lands underneath a different piece of the line.
    pub first_visual_cell_idx: usize,
}

pub struct RenderScreenLineResult {
    pub invalidate_on_hover_change: bool,
}

pub struct RenderScreenLineParams<'a> {
    /// zero-based offset from top of the window viewport to the line that
    /// needs to be rendered, measured in pixels
    pub top_pixel_y: f32,
    /// zero-based offset from left of the window viewport to the line that
    /// needs to be rendered, measured in pixels
    pub left_pixel_x: f32,
    pub pixel_width: f32,
    pub stable_line_idx: Option<StableRowIndex>,
    pub line: &'a Line,
    pub selection: Range<usize>,
    pub cursor: &'a StableCursorPosition,
    pub palette: &'a ColorPalette,
    pub dims: &'a RenderableDimensions,
    pub config: &'a ConfigHandle,
    pub pane: Option<&'a Arc<dyn Pane>>,

    pub white_space: TextureRect,
    pub filled_box: TextureRect,

    pub cursor_border_color: LinearRgba,
    pub foreground: LinearRgba,
    pub is_active: bool,

    pub selection_fg: LinearRgba,
    pub selection_bg: LinearRgba,
    pub cursor_fg: LinearRgba,
    pub cursor_bg: LinearRgba,
    pub cursor_is_default_color: bool,

    pub window_is_transparent: bool,
    pub default_bg: LinearRgba,

    /// Override font resolution; useful together with
    /// the resolved title font
    pub font: Option<Rc<LoadedFont>>,
    pub style: Option<&'a TextStyle>,

    /// If true, use the shaper-determined pixel positions,
    /// rather than using monospace cell based positions.
    pub use_pixel_positioning: bool,

    pub render_metrics: RenderMetrics,
    pub shape_key: Option<LineToEleShapeCacheKey>,
    pub password_input: bool,
    pub is_wrap_continuation: bool,
    /// Whether `disable_bidi_for_processes_named` currently suppresses bidi
    /// for `pane`. Callers must compute this themselves (see
    /// `bidi_disabled_by_foreground_process`) rather than have
    /// `render_screen_line` recompute it per row: `paint_pane` already
    /// hoists this to once-per-pane-per-frame (see
    /// `LineRender::bidi_process_override`) because the underlying
    /// foreground-process lookup is comparatively expensive. Callers with
    /// no pane (e.g. the retro tab bar) should simply pass `false`, since
    /// `bidi_disabled_by_foreground_process` always returns `false` when
    /// there is no pane to check.
    pub bidi_process_override: bool,
}

#[derive(Debug, Hash, PartialEq, Eq, Clone)]
pub struct CursorProperties {
    pub position: StableCursorPosition,
    pub dead_key_or_leader: bool,
    pub cursor_is_default_color: bool,
    pub cursor_fg: LinearRgba,
    pub cursor_bg: LinearRgba,
    pub cursor_border_color: LinearRgba,
}

pub struct ComputeCellFgBgParams<'a> {
    pub selected: bool,
    pub cursor: Option<&'a StableCursorPosition>,
    pub fg_color: LinearRgba,
    pub bg_color: LinearRgba,
    pub is_active_pane: bool,
    pub config: &'a ConfigHandle,
    pub selection_fg: LinearRgba,
    pub selection_bg: LinearRgba,
    pub cursor_fg: LinearRgba,
    pub cursor_bg: LinearRgba,
    pub cursor_is_default_color: bool,
    pub cursor_border_color: LinearRgba,
    pub pane: Option<&'a Arc<dyn Pane>>,
}

#[derive(Debug)]
pub struct ComputeCellFgBgResult {
    pub fg_color: LinearRgba,
    pub fg_color_alt: LinearRgba,
    pub bg_color: LinearRgba,
    pub bg_color_alt: LinearRgba,
    pub fg_color_mix: f32,
    pub bg_color_mix: f32,
    pub cursor_border_color: LinearRgba,
    pub cursor_border_color_alt: LinearRgba,
    pub cursor_border_mix: f32,
    pub cursor_shape: Option<CursorShape>,
}

/// Basic cache of computed data from prior cluster to avoid doing the same
/// work for space separated clusters with the same style
#[derive(Clone, Debug)]
pub struct ClusterStyleCache<'a> {
    attrs: &'a CellAttributes,
    style: &'a TextStyle,
    underline_tex_rect: TextureRect,
    fg_color: LinearRgba,
    bg_color: LinearRgba,
    underline_color: LinearRgba,
}

impl crate::TermWindow {
    pub fn update_next_frame_time(&self, next_due: Option<Instant>) {
        if next_due.is_some() {
            update_next_frame_time(&mut self.has_animation.borrow_mut(), next_due);
        }
    }

    /// Task #271: unconditionally (i.e. regardless of window focus) ask for
    /// a follow-up repaint at `next_due`. This exists specifically for the
    /// per-tab frame-build budget (`tab_frame_build_budget_ms`, task #251):
    /// when that budget trips partway through a pane's rows, the remaining
    /// rows are left undrawn for this frame (see the `budget_exceeded`
    /// skip in `render/pane.rs`'s cache-miss branch) and need a follow-up
    /// frame to have a chance to draw them. Task #268 originally wired that
    /// follow-up through `update_next_frame_time`/`has_animation`, but
    /// `paint_impl` only ever consumes `has_animation` inside `if
    /// self.focused.is_some()`, so that fix silently did nothing for an
    /// unfocused window. This method instead schedules the repaint
    /// directly via `promise::spawn::spawn` + `Timer::at` +
    /// `window.notify`, the same focus-independent pattern already used by
    /// `schedule_render_thread_hang_check`.
    ///
    /// `scheduled_budget_repaint` dedups/coalesces: if a repaint is already
    /// pending for a time at or before `next_due`, this is a no-op, so a
    /// budget trip that repeats every frame (e.g. content still streaming
    /// in) schedules at most one pending timer at a time rather than
    /// stacking one per frame.
    pub fn schedule_budget_repaint(&self, next_due: Instant) {
        let prior = self.scheduled_budget_repaint.borrow_mut().take();
        match prior {
            Some(prior) if prior <= next_due => {
                // Already due before that time; put it back and do nothing else.
                self.scheduled_budget_repaint.borrow_mut().replace(prior);
                return;
            }
            _ => {
                self.scheduled_budget_repaint.borrow_mut().replace(next_due);
            }
        }

        let window = match self.window.clone() {
            Some(window) => window,
            None => return,
        };
        promise::spawn::spawn(async move {
            smol::Timer::at(next_due).await;
            let win = window.clone();
            window.notify(TermWindowNotif::Apply(Box::new(move |tw| {
                tw.scheduled_budget_repaint.borrow_mut().take();
                win.invalidate();
            })));
        })
        .detach();
    }

    fn get_intensity_if_bell_target_ringing(
        &self,
        pane: &Arc<dyn Pane>,
        config: &ConfigHandle,
        target: VisualBellTarget,
    ) -> Option<f32> {
        let mut per_pane = self.pane_state(pane.pane_id());
        if let Some(ringing) = per_pane.bell_start {
            if config.visual_bell.target == target {
                let mut color_ease = ColorEase::new(
                    config.visual_bell.fade_in_duration_ms,
                    config.visual_bell.fade_in_function,
                    config.visual_bell.fade_out_duration_ms,
                    config.visual_bell.fade_out_function,
                    Some(ringing),
                );

                let intensity = color_ease.intensity_one_shot();

                match intensity {
                    None => {
                        per_pane.bell_start.take();
                    }
                    Some((intensity, next)) => {
                        self.update_next_frame_time(Some(next));
                        return Some(intensity);
                    }
                }
            }
        }
        None
    }

    pub fn filled_rectangle<'a>(
        &self,
        layers: &'a mut TripleLayerQuadAllocator,
        layer_num: usize,
        rect: RectF,
        color: LinearRgba,
    ) -> anyhow::Result<QuadImpl<'a>> {
        let mut quad = layers.allocate(layer_num)?;
        let left_offset = self.dimensions.pixel_width as f32 / 2.;
        let top_offset = self.dimensions.pixel_height as f32 / 2.;
        let gl_state = self.render_state.as_ref().unwrap();
        quad.set_position(
            rect.min_x() - left_offset,
            rect.min_y() - top_offset,
            rect.max_x() - left_offset,
            rect.max_y() - top_offset,
        );
        quad.set_texture(gl_state.util_sprites.filled_box.texture_coords());
        quad.set_is_background();
        quad.set_fg_color(color);
        quad.set_hsv(None);
        Ok(quad)
    }

    #[allow(clippy::too_many_arguments)] // rendering pipeline: params are the inherent quad/cell data model
    pub fn poly_quad<'a>(
        &self,
        layers: &'a mut TripleLayerQuadAllocator,
        layer_num: usize,
        point: PointF,
        polys: &'static [Poly],
        underline_height: IntPixelLength,
        cell_size: SizeF,
        color: LinearRgba,
    ) -> anyhow::Result<QuadImpl<'a>> {
        let left_offset = self.dimensions.pixel_width as f32 / 2.;
        let top_offset = self.dimensions.pixel_height as f32 / 2.;
        let gl_state = self.render_state.as_ref().unwrap();
        let sprite = gl_state
            .glyph_cache
            .borrow_mut()
            .cached_block(
                BlockKey::PolyWithCustomMetrics {
                    polys,
                    underline_height,
                    cell_size: euclid::size2(cell_size.width as isize, cell_size.height as isize),
                },
                &self.render_metrics,
            )?
            .texture_coords();

        let mut quad = layers.allocate(layer_num)?;

        quad.set_position(
            point.x - left_offset,
            point.y - top_offset,
            (point.x + cell_size.width) - left_offset,
            (point.y + cell_size.height) - top_offset,
        );
        quad.set_texture(sprite);
        quad.set_fg_color(color);
        quad.set_alt_color_and_mix_value(color, 0.);
        quad.set_hsv(None);
        quad.set_has_color(false);
        Ok(quad)
    }

    pub fn min_scroll_bar_height(&self) -> f32 {
        self.config
            .min_scroll_bar_height
            .evaluate_as_pixels(DimensionContext {
                dpi: self.dimensions.dpi as f32,
                pixel_max: self.terminal_size.pixel_height as f32,
                pixel_cell: self.render_metrics.cell_size.height as f32,
            })
    }

    pub fn padding_left_top(&self) -> (f32, f32) {
        let h_context = DimensionContext {
            dpi: self.dimensions.dpi as f32,
            pixel_max: self.terminal_size.pixel_width as f32,
            pixel_cell: self.render_metrics.cell_size.width as f32,
        };
        let v_context = DimensionContext {
            dpi: self.dimensions.dpi as f32,
            pixel_max: self.terminal_size.pixel_height as f32,
            pixel_cell: self.render_metrics.cell_size.height as f32,
        };

        let padding_left = self
            .config
            .window_padding
            .left
            .evaluate_as_pixels(h_context);
        let padding_right = self.config.window_padding.right;
        let padding_top = self.config.window_padding.top.evaluate_as_pixels(v_context);
        let padding_bottom = self
            .config
            .window_padding
            .bottom
            .evaluate_as_pixels(v_context);

        let horizontal_gap = self.dimensions.pixel_width as f32
            - self.terminal_size.pixel_width as f32
            - padding_left
            - if self.show_scroll_bar && padding_right.is_zero() {
                h_context.pixel_cell
            } else {
                padding_right.evaluate_as_pixels(h_context)
            };
        let vertical_gap = self.dimensions.pixel_height as f32
            - self.terminal_size.pixel_height as f32
            - padding_top
            - padding_bottom
            - if self.show_tab_bar {
                self.tab_bar_pixel_height().unwrap_or(0.)
            } else {
                0.
            };
        let left_gap = match self.config.window_content_alignment.horizontal {
            HorizontalWindowContentAlignment::Left => 0.,
            HorizontalWindowContentAlignment::Center => (horizontal_gap / 2.).round(),
            HorizontalWindowContentAlignment::Right => horizontal_gap,
        };
        let top_gap = match self.config.window_content_alignment.vertical {
            VerticalWindowContentAlignment::Top => 0.,
            VerticalWindowContentAlignment::Center => (vertical_gap / 2.).round(),
            VerticalWindowContentAlignment::Bottom => vertical_gap,
        };

        (padding_left + left_gap, padding_top + top_gap)
    }

    fn resolve_lock_glyph(
        &self,
        style: &TextStyle,
        attrs: &CellAttributes,
        font: Option<&Rc<LoadedFont>>,
        gl_state: &RenderState,
        metrics: &RenderMetrics,
    ) -> anyhow::Result<Rc<CachedGlyph>> {
        let fa_lock = "\u{f023}";
        let line = Line::from_text(fa_lock, attrs, 0, None);
        let cluster = line.cluster(None);
        let shape_info = self.cached_cluster_shape(style, &cluster[0], gl_state, font, metrics)?;
        Ok(Rc::clone(&shape_info[0].glyph))
    }

    #[allow(clippy::too_many_arguments)] // rendering pipeline: params are the inherent block/glyph data model
    pub fn populate_block_quad(
        &self,
        block: BlockKey,
        gl_state: &RenderState,
        quads: &mut dyn QuadAllocator,
        pos_x: f32,
        params: &RenderScreenLineParams,
        hsv: Option<config::HsbTransform>,
        glyph_color: LinearRgba,
    ) -> anyhow::Result<()> {
        let sprite = gl_state
            .glyph_cache
            .borrow_mut()
            .cached_block(block, &params.render_metrics)?
            .texture_coords();

        let mut quad = quads.allocate()?;
        let cell_width = params.render_metrics.cell_size.width as f32;
        let cell_height = params.render_metrics.cell_size.height as f32;
        let pos_y = (self.dimensions.pixel_height as f32 / -2.) + params.top_pixel_y;
        quad.set_position(pos_x, pos_y, pos_x + cell_width, pos_y + cell_height);
        quad.set_hsv(hsv);
        quad.set_fg_color(glyph_color);
        quad.set_texture(sprite);
        quad.set_has_color(false);
        Ok(())
    }

    /// Render iTerm2 style image attributes
    #[allow(clippy::too_many_arguments)] // rendering pipeline: params are the inherent image/glyph data model
    pub fn populate_image_quad(
        &self,
        image: &termwiz::image::ImageCell,
        gl_state: &RenderState,
        layers: &mut TripleLayerQuadAllocator,
        layer_num: usize,
        cell_idx: usize,
        params: &RenderScreenLineParams,
        hsv: Option<config::HsbTransform>,
        glyph_color: LinearRgba,
    ) -> anyhow::Result<()> {
        if self.allow_images == AllowImage::No {
            return Ok(());
        }

        let padding = self
            .render_metrics
            .cell_size
            .height
            .max(params.render_metrics.cell_size.width) as usize;
        let padding = if padding.is_power_of_two() {
            padding
        } else {
            padding.next_power_of_two()
        };

        let (sprite, next_due, _load_state) = gl_state
            .glyph_cache
            .borrow_mut()
            .cached_image(image.image_data(), Some(padding), self.allow_images)
            .context("cached_image")?;
        self.update_next_frame_time(next_due);
        let width = sprite.coords.size.width;
        let height = sprite.coords.size.height;

        let top_left = image.top_left();
        let bottom_right = image.bottom_right();

        // We *could* call sprite.texture.to_texture_coords() here,
        // but since that takes integer pixel coordinates, we'd
        // lose precision and end up with visual artifacts.
        // Instead, we compute the texture coords here in floating point.

        let texture_width = sprite.texture.width() as f32;
        let texture_height = sprite.texture.height() as f32;
        let origin = TextureCoord::new(
            (sprite.coords.origin.x as f32 + (*top_left.x * width as f32)) / texture_width,
            (sprite.coords.origin.y as f32 + (*top_left.y * height as f32)) / texture_height,
        );

        let size = TextureSize::new(
            (*bottom_right.x - *top_left.x) * width as f32 / texture_width,
            (*bottom_right.y - *top_left.y) * height as f32 / texture_height,
        );

        let texture_rect = TextureRect::new(origin, size);

        let mut quad = layers.allocate(layer_num)?;
        let cell_width = params.render_metrics.cell_size.width as f32;
        let cell_height = params.render_metrics.cell_size.height as f32;
        let pos_y = (self.dimensions.pixel_height as f32 / -2.) + params.top_pixel_y;

        let pos_x = (self.dimensions.pixel_width as f32 / -2.)
            + params.left_pixel_x
            + (cell_idx as f32 * cell_width);

        let (padding_left, padding_top, padding_right, padding_bottom) = image.padding();

        quad.set_position(
            pos_x + padding_left as f32,
            pos_y + padding_top as f32,
            pos_x + cell_width + padding_left as f32 - padding_right as f32,
            pos_y + cell_height + padding_top as f32 - padding_bottom as f32,
        );
        quad.set_hsv(hsv);
        quad.set_fg_color(glyph_color);
        quad.set_texture(texture_rect);
        quad.set_has_color(true);

        Ok(())
    }

    fn ensure_min_contrast(&self, fg_color: LinearRgba, bg_color: LinearRgba) -> LinearRgba {
        match self.config.text_min_contrast_ratio {
            Some(ratio) => fg_color
                .ensure_contrast_ratio(&bg_color, ratio)
                .unwrap_or(fg_color),
            None => fg_color,
        }
    }

    pub fn compute_cell_fg_bg(&self, params: ComputeCellFgBgParams) -> ComputeCellFgBgResult {
        if params.cursor.is_some() {
            if let Some(bg_color_mix) = self.get_intensity_if_bell_target_ringing(
                params.pane.expect("cursor only set if pane present"),
                params.config,
                VisualBellTarget::CursorColor,
            ) {
                let (fg_color, bg_color) = if self.use_reverse_video_cursor(&params) {
                    (params.bg_color, params.fg_color)
                } else {
                    (params.cursor_fg, params.cursor_bg)
                };

                let fg_color = self.ensure_min_contrast(fg_color, bg_color);

                // interpolate between the background color and the target color
                let bg_color_alt = params
                    .config
                    .resolved_palette
                    .visual_bell
                    .map(|c| c.to_linear())
                    .unwrap_or(fg_color);

                return ComputeCellFgBgResult {
                    fg_color,
                    fg_color_alt: fg_color,
                    fg_color_mix: 0.,
                    bg_color,
                    bg_color_alt,
                    bg_color_mix,
                    cursor_shape: Some(CursorShape::Default),
                    cursor_border_color: bg_color,
                    cursor_border_color_alt: bg_color_alt,
                    cursor_border_mix: bg_color_mix,
                };
            }

            let dead_key_or_leader =
                self.dead_key_status != DeadKeyStatus::None || self.leader_is_active();

            if dead_key_or_leader && params.is_active_pane {
                let (fg_color, bg_color) = if self.use_reverse_video_cursor(&params) {
                    (params.bg_color, params.fg_color)
                } else {
                    (params.cursor_fg, params.cursor_bg)
                };

                let fg_color = self.ensure_min_contrast(fg_color, bg_color);

                let color = params
                    .config
                    .resolved_palette
                    .compose_cursor
                    .map(|c| c.to_linear())
                    .unwrap_or(bg_color);

                return ComputeCellFgBgResult {
                    fg_color,
                    fg_color_alt: fg_color,
                    fg_color_mix: 0.,
                    bg_color,
                    bg_color_alt: bg_color,
                    bg_color_mix: 0.,
                    cursor_shape: Some(CursorShape::Default),
                    cursor_border_color: color,
                    cursor_border_color_alt: color,
                    cursor_border_mix: 0.,
                };
            }
        }

        let (cursor_shape, visibility) = match params.cursor {
            Some(cursor) => (
                params
                    .config
                    .default_cursor_style
                    .effective_shape(cursor.shape),
                cursor.visibility,
            ),
            _ => (CursorShape::default(), CursorVisibility::Hidden),
        };

        let focused_and_active = self.focused.is_some() && params.is_active_pane;

        let (fg_color, bg_color, cursor_bg) = match (
            params.selected,
            focused_and_active,
            cursor_shape,
            visibility,
        ) {
            // Selected text overrides colors
            (true, _, _, CursorVisibility::Hidden) => (
                params.selection_fg.when_fully_transparent(params.fg_color),
                params.selection_bg,
                params.cursor_bg,
            ),
            // block Cursor cell overrides colors
            (
                _,
                true,
                CursorShape::BlinkingBlock | CursorShape::SteadyBlock,
                CursorVisibility::Visible,
            ) => {
                if self.use_reverse_video_cursor(&params) {
                    (params.bg_color, params.fg_color, params.fg_color)
                } else {
                    (
                        params.cursor_fg.when_fully_transparent(params.fg_color),
                        params.cursor_bg,
                        params.cursor_bg,
                    )
                }
            }
            (
                _,
                true,
                CursorShape::BlinkingUnderline
                | CursorShape::SteadyUnderline
                | CursorShape::BlinkingBar
                | CursorShape::SteadyBar,
                CursorVisibility::Visible,
            ) => {
                if self.use_reverse_video_cursor(&params) {
                    (params.fg_color, params.bg_color, params.fg_color)
                } else {
                    (params.fg_color, params.bg_color, params.cursor_bg)
                }
            }
            // Normally, render the cell as configured (or if the window is unfocused)
            _ => (params.fg_color, params.bg_color, params.cursor_border_color),
        };

        let fg_color = self.ensure_min_contrast(fg_color, bg_color);

        let blinking = params.cursor.is_some()
            && params.is_active_pane
            && cursor_shape.is_blinking()
            && params.config.cursor_blink_rate != 0
            && self.focused.is_some();

        let mut fg_color_alt = fg_color;
        let bg_color_alt = bg_color;
        let mut fg_color_mix = 0.;
        let bg_color_mix = 0.;
        let mut cursor_border_color_alt = cursor_bg;
        let mut cursor_border_mix = 0.;

        if blinking {
            let mut color_ease = self.cursor_blink_state.borrow_mut();
            color_ease.update_start(self.prev_cursor.last_cursor_movement());
            let (intensity, next) = color_ease.intensity_continuous();

            cursor_border_mix = intensity;
            cursor_border_color_alt = params.bg_color;

            if matches!(
                cursor_shape,
                CursorShape::BlinkingBlock | CursorShape::SteadyBlock,
            ) {
                fg_color_alt = params.fg_color;
                fg_color_mix = intensity;
            }

            self.update_next_frame_time(Some(next));
        }

        ComputeCellFgBgResult {
            fg_color,
            fg_color_alt,
            bg_color,
            bg_color_alt,
            fg_color_mix,
            bg_color_mix,
            cursor_border_color: cursor_bg,
            cursor_border_color_alt,
            cursor_border_mix,
            cursor_shape: if visibility == CursorVisibility::Visible {
                match cursor_shape {
                    CursorShape::BlinkingBlock | CursorShape::SteadyBlock if focused_and_active => {
                        Some(CursorShape::Default)
                    }
                    // When not focused, convert bar to block to make it more visually
                    // distinct from the focused bar in another pane
                    _shape if !focused_and_active => Some(CursorShape::SteadyBlock),
                    shape => Some(shape),
                }
            } else {
                None
            },
        }
    }

    fn use_reverse_video_cursor(&self, params: &ComputeCellFgBgParams) -> bool {
        self.config.force_reverse_video_cursor
            && params.cursor_is_default_color
            && params.fg_color.contrast_ratio(&params.bg_color)
                >= self.config.reverse_video_cursor_min_contrast
    }

    fn glyph_infos_to_glyphs(
        &self,
        style: &TextStyle,
        glyph_cache: &mut GlyphCache,
        infos: &[GlyphInfo],
        font: &Rc<LoadedFont>,
        metrics: &RenderMetrics,
    ) -> anyhow::Result<Vec<Rc<CachedGlyph>>> {
        let mut glyphs = Vec::with_capacity(infos.len());
        let mut iter = infos.iter().peekable();
        while let Some(info) = iter.next() {
            if self.config.custom_block_glyphs
                && info.only_char.and_then(BlockKey::from_char).is_some()
            {
                // Don't bother rendering the glyph from the font, as it can
                // have incorrect advance metrics.
                // Instead, just use our pixel-perfect cell metrics
                glyphs.push(Rc::new(CachedGlyph {
                    brightness_adjust: 1.0,
                    has_color: false,
                    texture: None,
                    x_advance: PixelLength::new(metrics.cell_size.width as f64),
                    x_offset: PixelLength::zero(),
                    y_offset: PixelLength::zero(),
                    bearing_x: PixelLength::zero(),
                    bearing_y: PixelLength::zero(),
                    scale: 1.0,
                }));
                continue;
            }

            let followed_by_space = match iter.peek() {
                Some(next_info) => next_info.is_space,
                None => false,
            };

            glyphs.push(glyph_cache.cached_glyph(
                info,
                style,
                followed_by_space,
                font,
                metrics,
                info.num_cells,
            )?);
        }
        Ok(glyphs)
    }

    /// Shape the printable text from a cluster
    fn cached_cluster_shape(
        &self,
        style: &TextStyle,
        cluster: &CellCluster,
        gl_state: &RenderState,
        font: Option<&Rc<LoadedFont>>,
        metrics: &RenderMetrics,
    ) -> anyhow::Result<Rc<Vec<ShapedInfo>>> {
        let shape_resolve_start = Instant::now();
        let key = BorrowedShapeCacheKey {
            style,
            text: &cluster.text,
        };
        let glyph_info = match self.lookup_cached_shape(&key) {
            Some(Ok(info)) => info,
            Some(Err(err)) => return Err(err),
            None => {
                let font = match font {
                    Some(f) => Rc::clone(f),
                    None => self.fonts.resolve_font(style)?,
                };
                let window = self.window.as_ref().unwrap().clone();

                let presentation_width = PresentationWidth::with_cluster(cluster);

                match font.shape(
                    &cluster.text,
                    move || window.notify(TermWindowNotif::InvalidateShapeCache),
                    BlockKey::filter_out_synthetic,
                    Some(cluster.presentation),
                    cluster.direction,
                    None, // FIXME: need more paragraph context
                    Some(&presentation_width),
                ) {
                    Ok(info) => {
                        let glyphs = self.glyph_infos_to_glyphs(
                            style,
                            &mut gl_state.glyph_cache.borrow_mut(),
                            &info,
                            &font,
                            metrics,
                        )?;
                        let shaped = Rc::new(ShapedInfo::process(&info, &glyphs));

                        self.shape_cache
                            .borrow_mut()
                            .put(key.to_owned(), Ok(Rc::clone(&shaped)));
                        shaped
                    }
                    Err(err) => {
                        if err.root_cause().downcast_ref::<ClearShapeCache>().is_some() {
                            return Err(err);
                        }

                        let res = anyhow!("shaper error: {}", err);
                        self.shape_cache.borrow_mut().put(key.to_owned(), Err(err));
                        return Err(res);
                    }
                }
            }
        };
        metrics::histogram!("cached_cluster_shape").record(shape_resolve_start.elapsed());
        log::trace!(
            "shape_resolve for cluster len {} -> elapsed {:?}",
            cluster.text.len(),
            shape_resolve_start.elapsed()
        );
        Ok(glyph_info)
    }

    fn lookup_cached_shape(
        &self,
        key: &dyn ShapeCacheKeyTrait,
    ) -> Option<anyhow::Result<Rc<Vec<ShapedInfo>>>> {
        match self.shape_cache.borrow_mut().get(key) {
            Some(Ok(info)) => Some(Ok(Rc::clone(info))),
            Some(Err(err)) => Some(Err(anyhow!("cached shaper error: {}", err))),
            None => None,
        }
    }

    pub fn recreate_texture_atlas(&mut self, size: Option<usize>) -> anyhow::Result<()> {
        // Reset frame signature on atlas resize - texture content changed,
        // so the previous frame is no longer comparable (task #450)
        self.last_frame_signature = None;
        self.shape_generation += 1;
        self.shape_cache.borrow_mut().clear();
        self.line_to_ele_shape_cache.borrow_mut().clear();
        // Task #439: clear shape_hash_cache on texture atlas resize
        self.shape_hash_cache.borrow_mut().clear();
        if let Some(render_state) = self.render_state.as_mut() {
            render_state.recreate_texture_atlas(&self.fonts, &self.render_metrics, size)?;
        }
        Ok(())
    }

    /// Task #439: Core cache lookup logic extracted for testability.
    /// Returns cached shape_hash if seqno matches, otherwise computes via closure.
    fn shape_hash_for_line(
        &mut self,
        line: &Line,
        pane_id: PaneId,
        stable_row: StableRowIndex,
    ) -> [u8; 16] {
        let seqno = line.current_seqno();
        let key = ShapeHashCacheKey {
            pane_id,
            stable_row,
        };
        shape_hash_lookup(&mut self.shape_hash_cache.borrow_mut(), key, seqno, || {
            line.compute_shape_hash()
        })
    }
}

/// Task #439: Extracted cache lookup logic for testing.
/// This function contains the actual production cache decision logic.
/// Takes a closure that computes the hash on miss/seqno-mismatch.
pub fn shape_hash_lookup<F>(
    cache: &mut lfucache::LfuCache<ShapeHashCacheKey, ShapeHashEntry>,
    key: ShapeHashCacheKey,
    seqno: SequenceNo,
    compute: F,
) -> [u8; 16]
where
    F: FnOnce() -> [u8; 16],
{
    // Try to hit the cache
    if let Some(entry) = cache.get(&key) {
        if entry.seqno == seqno {
            // Cache hit! seqno matches, return cached hash
            return entry.shape_hash;
        }
        // Seqno mismatch: line changed, need to recompute
    }

    // Cache miss or seqno mismatch: compute and store
    let shape_hash = compute();
    let entry = ShapeHashEntry { seqno, shape_hash };
    cache.put(key, entry);
    shape_hash
}

fn resolve_fg_color_attr(
    attrs: &CellAttributes,
    fg: ColorAttribute,
    palette: &ColorPalette,
    config: &ConfigHandle,
    style: &config::TextStyle,
) -> LinearRgba {
    match fg {
        wezterm_term::color::ColorAttribute::Default => {
            if let Some(fg) = style.foreground {
                fg.into()
            } else {
                palette.resolve_fg(attrs.foreground())
            }
        }
        wezterm_term::color::ColorAttribute::PaletteIndex(idx)
            if idx < 8 && config.bold_brightens_ansi_colors != BoldBrightening::No =>
        {
            // For compatibility purposes, switch to a brighter version
            // of one of the standard ANSI colors when Bold is enabled.
            // This lifts black to dark grey.
            let idx = if attrs.intensity() == wezterm_term::Intensity::Bold {
                idx + 8
            } else {
                idx
            };

            palette.resolve_fg(wezterm_term::color::ColorAttribute::PaletteIndex(idx))
        }
        _ => palette.resolve_fg(fg),
    }
    .to_linear()
}

fn update_next_frame_time(storage: &mut Option<Instant>, next_due: Option<Instant>) {
    if let Some(next_due) = next_due {
        match storage.take() {
            None => {
                storage.replace(next_due);
            }
            Some(t) if next_due < t => {
                storage.replace(next_due);
            }
            Some(t) => {
                storage.replace(t);
            }
        }
    }
}

/// Whether `disable_bidi_for_processes_named` currently suppresses bidi for
/// `pane`, based on its *live* foreground process (re-derived here rather
/// than cached anywhere in this module -- the underlying
/// `get_foreground_process_name` call already has its own short-lived
/// cache, see `CachePolicy::AllowStale`). Callers that use this to decide
/// whether to reorder a line must also fold the result into any cache key
/// covering that decision (see `LineQuadCacheKey::bidi_process_override`),
/// since the same line content can need different treatment as the
/// foreground process changes.
fn bidi_disabled_by_foreground_process(
    pane: Option<&Arc<dyn Pane>>,
    config: &ConfigHandle,
) -> bool {
    if config.disable_bidi_for_processes_named.is_empty() {
        return false;
    }
    let Some(pane) = pane else {
        return false;
    };
    let Some(proc_name) = pane.get_foreground_process_name(CachePolicy::AllowStale) else {
        return false;
    };
    let basename = std::path::Path::new(&proc_name)
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or(proc_name);
    config
        .disable_bidi_for_processes_named
        .iter()
        .any(|name| name.eq_ignore_ascii_case(&basename))
}

fn same_hyperlink(a: Option<&Arc<Hyperlink>>, b: Option<&Arc<Hyperlink>>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => Arc::ptr_eq(a, b),
        _ => false,
    }
}

/// Benchmark LfuCacheU64 vs lru::LruCache for line_state_cache patterns.
/// Compares hit latency, insert/evict latency, bytes/entry, and hit ratio
/// on two access patterns: (a) miss-heavy burst (flood output) and (b)
/// stable screen (small repeatedly-accessed working set).
#[cfg(test)]
mod cache_bench {
    use super::*;
    use config::ConfigHandle;
    use lfucache::LfuCacheU64;
    use lru::LruCache;
    use std::mem::size_of;
    use std::sync::Arc;
    use std::time::Duration;

    // Test helpers for LfuCache with simple capacity
    fn test_cache_capacity(_config: &ConfigHandle) -> usize {
        1024
    }

    // Static capacity for use with fn pointer
    static BENCH_CAPACITY: std::sync::atomic::AtomicUsize =
        std::sync::atomic::AtomicUsize::new(1000);

    fn bench_capacity_func(_config: &ConfigHandle) -> usize {
        BENCH_CAPACITY.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Value type matching CachedLineState shape
    #[derive(Debug, Clone)]
    #[allow(dead_code)]
    struct TestCachedLineState {
        id: u64,
        seqno: SequenceNo,
        shape_hash: [u8; 16],
    }

    /// Create a test value matching CachedLineState structure
    fn make_test_value(id: u64) -> Arc<TestCachedLineState> {
        Arc::new(TestCachedLineState {
            id,
            seqno: SequenceNo::from(id as usize),
            shape_hash: [id as u8; 16],
        })
    }

    /// Simulate miss-heavy burst: mostly new keys, few repeats.
    /// During flood output, renderer sees mostly new line states
    /// with very few cache hits.
    fn miss_heavy_sequence(count: usize, capacity: usize) -> Vec<u64> {
        // Generate mostly unique keys with ~5% repeats to simulate rare hits
        let mut keys = Vec::with_capacity(count);
        for i in 0..count {
            if i % 20 == 0 && i > capacity {
                // Repeat a recent key ~5% of the time
                keys.push((i - capacity / 2) as u64);
            } else {
                keys.push(i as u64);
            }
        }
        keys
    }

    /// Simulate stable screen: small working set accessed repeatedly.
    /// During static screen redraws, renderer repeatedly hits the same
    /// small set of line states.
    fn stable_screen_sequence(count: usize, working_set_size: usize) -> Vec<u64> {
        let mut keys = Vec::with_capacity(count);
        for i in 0..count {
            // Cycle through a small working set (e.g., 24 lines for 80x24 screen)
            keys.push((i % working_set_size) as u64);
        }
        keys
    }

    /// Measure LfuCacheU64 performance
    fn benchmark_lfu(keys: &[u64], capacity: usize) -> (Duration, usize, usize, usize) {
        benchmarking::warm_up();

        // Set static capacity for fn pointer
        BENCH_CAPACITY.store(capacity, std::sync::atomic::Ordering::Relaxed);
        let config = ConfigHandle::default_config();
        let keys = keys.to_vec();
        let keys_for_bench = keys.clone();
        let bench_result = benchmarking::measure_function(move |measurer| {
            measurer.measure(|| {
                let config = ConfigHandle::default_config();
                let mut cache =
                    LfuCacheU64::new("bench_hit", "bench_miss", bench_capacity_func, &config);
                let mut hits = 0;
                let mut misses = 0;

                for key in &keys_for_bench {
                    if cache.get(key).is_some() {
                        hits += 1;
                    } else {
                        misses += 1;
                        cache.put(*key, make_test_value(*key));
                    }
                }

                std::hint::black_box(&mut cache);
                std::hint::black_box(hits);
                std::hint::black_box(misses);
            })
        })
        .unwrap();

        let mut cache = LfuCacheU64::new("bench_hit", "bench_miss", bench_capacity_func, &config);
        let mut hits = 0;
        let mut misses = 0;
        for key in &keys {
            if cache.get(key).is_some() {
                hits += 1;
            } else {
                misses += 1;
                cache.put(*key, make_test_value(*key));
            }
        }
        let final_len = cache.len();

        (bench_result.elapsed(), hits, misses, final_len)
    }

    /// Measure LruCache performance
    fn benchmark_lru(keys: &[u64], capacity: usize) -> (Duration, usize, usize, usize) {
        benchmarking::warm_up();
        let capacity_nonzero = std::num::NonZeroUsize::new(capacity).unwrap();

        let keys = keys.to_vec();
        let keys_for_bench = keys.clone();
        let bench_result = benchmarking::measure_function(move |measurer| {
            measurer.measure(|| {
                let mut cache = LruCache::new(capacity_nonzero);
                let mut hits = 0;
                let mut misses = 0;

                for key in &keys_for_bench {
                    if cache.get(key).is_some() {
                        hits += 1;
                    } else {
                        misses += 1;
                        cache.put(*key, make_test_value(*key));
                    }
                }

                std::hint::black_box(&mut cache);
                std::hint::black_box(hits);
                std::hint::black_box(misses);
            })
        })
        .unwrap();

        let mut cache = LruCache::new(capacity_nonzero);
        let mut hits = 0;
        let mut misses = 0;
        for key in &keys {
            if cache.get(key).is_some() {
                hits += 1;
            } else {
                misses += 1;
                cache.put(*key, make_test_value(*key));
            }
        }
        let final_len = cache.len();

        (bench_result.elapsed(), hits, misses, final_len)
    }

    #[test]
    #[ignore = "benchmark, not a correctness test -- takes ~16s; run explicitly with \
                `cargo test -p wezterm-gui --lib -- --ignored bench_cache_comparison`"]
    fn bench_cache_comparison() {
        println!("\n=== LfuCacheU64 vs lru::LruCache Benchmark ===");
        println!(
            "Value size: {} bytes (Arc<TestCachedLineState>)",
            size_of::<Arc<TestCachedLineState>>()
        );

        // Size of internal node structures
        println!("\nApproximate entry sizes:");
        println!("  LfuCache Entry: estimated ~{}+ bytes (Rc<Entry<K,V>> with 3 links, 2 RefCells, key, value)",
            size_of::<Rc<()>>() + size_of::<Arc<TestCachedLineState>>() + 32 // approx for links+RefCells
        );
        println!(
            "  LruCache Node: estimated ~{}+ bytes (Key + Value + Node links)",
            size_of::<u64>() + size_of::<Arc<TestCachedLineState>>() + 32 // approx for Node overhead
        );

        // Test parameters
        let capacity = 1000; // Typical cache size
        let operations = 10000;

        // Pattern (a): Miss-heavy burst (flood output)
        println!("\n--- Pattern (a): Miss-heavy burst (flood output) ---");
        println!("Operations: {}, Cache capacity: {}", operations, capacity);
        let miss_heavy_keys = miss_heavy_sequence(operations, capacity);

        let (lfu_time, lfu_hits, lfu_misses, lfu_final) = benchmark_lfu(&miss_heavy_keys, capacity);
        let (lru_time, lru_hits, lru_misses, lru_final) = benchmark_lru(&miss_heavy_keys, capacity);

        let lfu_hit_ratio = (lfu_hits as f64) / (lfu_hits + lfu_misses) as f64;
        let lru_hit_ratio = (lru_hits as f64) / (lru_hits + lru_misses) as f64;

        println!("\nLfuCacheU64:");
        println!("  Time: {:?}", lfu_time);
        println!(
            "  Hits: {}, Misses: {}, Hit ratio: {:.2}%",
            lfu_hits,
            lfu_misses,
            lfu_hit_ratio * 100.0
        );
        println!("  Final cache size: {}", lfu_final);

        println!("\nlru::LruCache:");
        println!("  Time: {:?}", lru_time);
        println!(
            "  Hits: {}, Misses: {}, Hit ratio: {:.2}%",
            lru_hits,
            lru_misses,
            lru_hit_ratio * 100.0
        );
        println!("  Final cache size: {}", lru_final);

        // Show speedup/slowdown
        let lfu_ns = lfu_time.as_nanos();
        let lru_ns = lru_time.as_nanos();
        if lfu_ns > 0 {
            let ratio = (lru_ns as f64 / lfu_ns as f64) * 100.0;
            if ratio < 100.0 {
                println!("  → LruCache is {:.1}% faster", 100.0 - ratio);
            } else {
                println!("  → LruCache is {:.1}% slower", ratio - 100.0);
            }
        }

        // Pattern (b): Stable screen (small working set)
        println!("\n--- Pattern (b): Stable screen (small working set) ---");
        let working_set_size = 24; // Typical terminal height
        println!(
            "Operations: {}, Cache capacity: {}, Working set: {}",
            operations, capacity, working_set_size
        );
        let stable_keys = stable_screen_sequence(operations, working_set_size);

        let (lfu_time_stable, lfu_hits_stable, lfu_misses_stable, lfu_final_stable) =
            benchmark_lfu(&stable_keys, capacity);
        let (lru_time_stable, lru_hits_stable, lru_misses_stable, lru_final_stable) =
            benchmark_lru(&stable_keys, capacity);

        let lfu_hit_ratio_stable =
            (lfu_hits_stable as f64) / (lfu_hits_stable + lfu_misses_stable) as f64;
        let lru_hit_ratio_stable =
            (lru_hits_stable as f64) / (lru_hits_stable + lru_misses_stable) as f64;

        println!("\nLfuCacheU64:");
        println!("  Time: {:?}", lfu_time_stable);
        println!(
            "  Hits: {}, Misses: {}, Hit ratio: {:.2}%",
            lfu_hits_stable,
            lfu_misses_stable,
            lfu_hit_ratio_stable * 100.0
        );
        println!("  Final cache size: {}", lfu_final_stable);

        println!("\nlru::LruCache:");
        println!("  Time: {:?}", lru_time_stable);
        println!(
            "  Hits: {}, Misses: {}, Hit ratio: {:.2}%",
            lru_hits_stable,
            lru_misses_stable,
            lru_hit_ratio_stable * 100.0
        );
        println!("  Final cache size: {}", lru_final_stable);

        // Show speedup/slowdown
        let lfu_ns_stable = lfu_time_stable.as_nanos();
        let lru_ns_stable = lru_time_stable.as_nanos();
        if lfu_ns_stable > 0 {
            let ratio = (lru_ns_stable as f64 / lfu_ns_stable as f64) * 100.0;
            if ratio < 100.0 {
                println!("  → LruCache is {:.1}% faster", 100.0 - ratio);
            } else {
                println!("  → LruCache is {:.1}% slower", ratio - 100.0);
            }
        }

        // Summary and recommendation
        println!("\n=== Summary ===");
        println!("Miss-heavy pattern (flood):");
        println!(
            "  LfuCache: {:.2}%, LruCache: {:.2}%, Time: LruCache {:.1}% {}",
            lfu_hit_ratio * 100.0,
            lru_hit_ratio * 100.0,
            if lfu_ns > 0 {
                (lru_ns as f64 / lfu_ns as f64 * 100.0 - 100.0).abs()
            } else {
                0.0
            },
            if lru_ns < lfu_ns { "faster" } else { "slower" }
        );
        println!("Stable screen pattern:");
        println!(
            "  LfuCache: {:.2}%, LruCache: {:.2}%, Time: LruCache {:.1}% {}",
            lfu_hit_ratio_stable * 100.0,
            lru_hit_ratio_stable * 100.0,
            if lfu_ns_stable > 0 {
                (lru_ns_stable as f64 / lfu_ns_stable as f64 * 100.0 - 100.0).abs()
            } else {
                0.0
            },
            if lru_ns_stable < lfu_ns_stable {
                "faster"
            } else {
                "slower"
            }
        );
    }

    /// Task #439: Test that empirically demonstrates the clone-broken cache issue.
    /// This test shows that the current Line::appdata-based cache never hits
    /// because Line::clone copies the Weak reference, and set_appdata on the
    /// clone doesn't propagate back to the original Line in the Screen.
    #[test]
    fn test_clone_broken_cache() {
        use std::sync::Arc;
        use wezterm_surface::Line;

        // Create a line with some test content
        let original = Line::with_width_and_cell(80, wezterm_term::Cell::default(), 1usize);

        // First call: compute hash, store in cache via appdata
        let state = Arc::new(CachedLineState {
            id: 42,
            seqno: 1,
            shape_hash: [1u8; 16],
        });
        original.set_appdata(Arc::clone(&state));

        // Verify it worked: original has the appdata
        let original_appdata = original.get_appdata();
        assert!(original_appdata.is_some(), "original should have appdata");
        if let Some(arc) = original_appdata {
            if let Some(line_state) = arc.downcast_ref::<CachedLineState>() {
                assert_eq!(line_state.id, 42, "original appdata should have id 42");
            } else {
                panic!("original appdata should be CachedLineState");
            }
        }

        // Simulate what get_lines() does: clone the line
        let clone1 = original.clone();

        // Clone initially has the same Weak reference, so it can upgrade
        let clone1_appdata = clone1.get_appdata();
        assert!(
            clone1_appdata.is_some(),
            "clone should be able to upgrade Weak initially"
        );
        if let Some(arc) = clone1_appdata {
            if let Some(line_state) = arc.downcast_ref::<CachedLineState>() {
                assert_eq!(line_state.id, 42, "clone should see original's appdata");
            } else {
                panic!("clone appdata should be CachedLineState");
            }
        }

        // This is what the render path does on cache miss: set appdata on the CLONE
        let new_state = Arc::new(CachedLineState {
            id: 43,
            seqno: 1,
            shape_hash: [2u8; 16],
        });
        clone1.set_appdata(Arc::clone(&new_state));

        // The clone now has the new appdata
        let clone1_appdata = clone1.get_appdata();
        assert!(clone1_appdata.is_some());
        if let Some(arc) = clone1_appdata {
            if let Some(line_state) = arc.downcast_ref::<CachedLineState>() {
                assert_eq!(line_state.id, 43, "clone should have new appdata");
            } else {
                panic!("clone appdata should be CachedLineState");
            }
        }

        // KEY BUG: The ORIGINAL Line still has the OLD appdata, not the new one
        let original_appdata_after = original.get_appdata();
        assert!(
            original_appdata_after.is_some(),
            "original should still have some appdata"
        );
        if let Some(arc) = original_appdata_after {
            if let Some(line_state) = arc.downcast_ref::<CachedLineState>() {
                assert_eq!(
                    line_state.id, 42,
                    "original should still have old appdata (BUG!)"
                );
            } else {
                panic!("original appdata should be CachedLineState");
            }
        }

        // Simulate the next frame: clone again
        let clone2 = original.clone();

        // This clone can still upgrade the ORIGINAL's Weak reference (id: 42)
        // NOT the clone's updated reference (id: 43) - it's completely lost
        let clone2_appdata = clone2.get_appdata();
        assert!(
            clone2_appdata.is_some(),
            "clone2 can upgrade original's Weak"
        );
        if let Some(arc) = clone2_appdata {
            if let Some(line_state) = arc.downcast_ref::<CachedLineState>() {
                assert_eq!(
                    line_state.id, 42,
                    "clone2 sees original's old appdata, not clone1's new appdata (BUG!)"
                );
            } else {
                panic!("clone2 appdata should be CachedLineState");
            }
        }

        // If we had a seqno bump on the original, the cache entry would be invalid anyway,
        // but for static screens (no seqno bumps), the cache SHOULD hit but DOESN'T.
        // The effective hit rate is 0% for any line that doesn't get modified between frames.

        println!("✓ Test confirmed: set_appdata on Line clones doesn't propagate back to original");
        println!("  - Original appdata id: 42 (unchanged)");
        println!("  - Clone1 appdata id: 43 (updated on clone, lost)");
        println!("  - Clone2 appdata id: 42 (saw original, not clone1's update)");
        println!("  → This is why shape_hash_for_line cache never hits on static screens");
    }

    /// Task #439: Regression test that the extracted shape_hash_lookup function
    /// actually skips recompute on cache hits (proves production code works).
    #[test]
    fn test_shape_hash_lookup_skips_recompute_on_hit() {
        use lfucache::LfuCache;
        use std::cell::Cell;

        let _capacity = std::num::NonZeroUsize::new(100).unwrap();
        let mut cache = LfuCache::new(
            "test_hit",
            "test_miss",
            test_cache_capacity,
            &ConfigHandle::default_config(),
        );

        let pane_id: PaneId = 1;
        let stable_row: StableRowIndex = 0;
        let key = ShapeHashCacheKey {
            pane_id,
            stable_row,
        };
        let seqno: SequenceNo = 1;

        let compute_count = Cell::new(0);
        let expected_hash = [42u8; 16];

        // First call: miss, should compute
        let hash1 = shape_hash_lookup(&mut cache, key, seqno, || {
            compute_count.set(compute_count.get() + 1);
            expected_hash
        });

        assert_eq!(compute_count.get(), 1, "should compute on first miss");
        assert_eq!(hash1, expected_hash, "should return computed hash");

        // Second call: hit, should NOT recompute
        let hash2 = shape_hash_lookup(&mut cache, key, seqno, || {
            compute_count.set(compute_count.get() + 1);
            expected_hash
        });

        assert_eq!(compute_count.get(), 1, "should NOT recompute on cache hit");
        assert_eq!(hash2, expected_hash, "should return cached hash on hit");
    }

    /// Task #439: Regression test that seqno mismatch forces recompute even with cache hit.
    #[test]
    fn test_shape_hash_lookup_recomputes_on_seqno_mismatch() {
        use lfucache::LfuCache;
        use std::cell::Cell;

        let _capacity = std::num::NonZeroUsize::new(100).unwrap();
        let mut cache = LfuCache::new(
            "test_hit",
            "test_miss",
            test_cache_capacity,
            &ConfigHandle::default_config(),
        );

        let pane_id: PaneId = 1;
        let stable_row: StableRowIndex = 0;
        let key = ShapeHashCacheKey {
            pane_id,
            stable_row,
        };

        let compute_count = Cell::new(0);

        // First call with seqno=1
        let hash1 = shape_hash_lookup(&mut cache, key, 1, || {
            compute_count.set(compute_count.get() + 1);
            [1u8; 16]
        });

        assert_eq!(compute_count.get(), 1, "should compute on first miss");

        // Second call with same seqno=1: cache hit, no recompute
        let hash2 = shape_hash_lookup(&mut cache, key, 1, || {
            compute_count.set(compute_count.get() + 1);
            [2u8; 16]
        });

        assert_eq!(compute_count.get(), 1, "should NOT recompute on cache hit");
        assert_eq!(hash1, hash2, "should return same cached hash");

        // Third call with different seqno=2: seqno mismatch, must recompute
        let hash3 = shape_hash_lookup(&mut cache, key, 2, || {
            compute_count.set(compute_count.get() + 1);
            [3u8; 16]
        });

        assert_eq!(compute_count.get(), 2, "should recompute on seqno mismatch");
        assert_eq!(hash3, [3u8; 16], "should return newly computed hash");
    }

    /// Task #439: Empirical test measuring actual cache hit rate over multiple frames.
    /// Simulates 50 frames over 50 unchanged lines (static screen).
    /// Frame 1 is expected to be 0% hits (cache cold), frames 2..50 expected ~100% hits.
    #[test]
    fn test_shape_hash_cache_hit_rate_static_screen() {
        use lfucache::LfuCache;
        use std::cell::Cell;

        let num_lines = 50;
        let num_frames = 50;
        let _capacity = std::num::NonZeroUsize::new(1024).unwrap();
        let mut cache = LfuCache::new(
            "static_hit",
            "static_miss",
            test_cache_capacity,
            &ConfigHandle::default_config(),
        );

        let compute_count = Cell::new(0);
        let hit_count = Cell::new(0);
        let miss_count = Cell::new(0);

        // Simulate static screen: same 50 lines with same seqno across all frames
        for _frame in 0..num_frames {
            for line_idx in 0..num_lines {
                let pane_id: PaneId = 1;
                let stable_row: StableRowIndex = line_idx as StableRowIndex;
                let key = ShapeHashCacheKey {
                    pane_id,
                    stable_row,
                };
                let seqno: SequenceNo = 1; // Static screen: seqno never changes

                let frame_compute_count = compute_count.get();

                shape_hash_lookup(&mut cache, key, seqno, || {
                    compute_count.set(compute_count.get() + 1);
                    // Simulate expensive computation: hash based on line index
                    let mut hash = [0u8; 16];
                    hash[0] = line_idx as u8;
                    hash
                });

                // Track hits vs misses
                if compute_count.get() > frame_compute_count {
                    miss_count.set(miss_count.get() + 1);
                } else {
                    hit_count.set(hit_count.get() + 1);
                }
            }
        }

        let total_lookups = hit_count.get() + miss_count.get();
        let overall_hit_rate = (hit_count.get() as f64) / (total_lookups as f64) * 100.0;

        // Frame 1: all misses (cache cold)
        assert_eq!(miss_count.get(), num_lines, "frame 1 should be all misses");

        // Frames 2..50: all hits (static screen, cache warm)
        // Total lookups: 50 frames * 50 lines = 2500
        // Misses: 50 (frame 1 only)
        // Hits: 2500 - 50 = 2450
        // Expected hit rate: 2450 / 2500 = 98%
        assert_eq!(total_lookups, num_frames * num_lines, "total lookups count");
        assert_eq!(
            hit_count.get(),
            (num_frames - 1) * num_lines,
            "frames 2..50 should be all hits"
        );

        println!(
            "✓ Static screen cache hit rate: {:.2}% ({}/{} hits, {} computes)",
            overall_hit_rate,
            hit_count.get(),
            total_lookups,
            compute_count.get()
        );
        println!("  Frame 1: {} misses (cache warmup)", num_lines);
        println!("  Frames 2..50: {} hits (cache warm)", hit_count.get());
    }

    /// Task #439: Regression test that different panes don't share cache entries.
    #[test]
    fn test_shape_hash_cache_key_includes_pane_id() {
        use lfucache::LfuCache;
        use std::cell::Cell;

        let _capacity = std::num::NonZeroUsize::new(100).unwrap();
        let mut cache = LfuCache::new(
            "pane_hit",
            "pane_miss",
            test_cache_capacity,
            &ConfigHandle::default_config(),
        );

        let pane_id_1: PaneId = 1;
        let pane_id_2: PaneId = 2;
        let stable_row: StableRowIndex = 0;

        let key1 = ShapeHashCacheKey {
            pane_id: pane_id_1,
            stable_row,
        };
        let key2 = ShapeHashCacheKey {
            pane_id: pane_id_2,
            stable_row,
        };

        // These should be different keys
        assert_ne!(
            key1, key2,
            "keys with different pane_id should be different"
        );

        let compute_count = Cell::new(0);

        // Store for pane 1
        let hash1 = shape_hash_lookup(&mut cache, key1, 1, || {
            compute_count.set(compute_count.get() + 1);
            [1u8; 16]
        });

        assert_eq!(compute_count.get(), 1, "should compute for pane 1");

        // Pane 2 should not see pane 1's entry
        let hash2 = shape_hash_lookup(&mut cache, key2, 1, || {
            compute_count.set(compute_count.get() + 1);
            [2u8; 16]
        });

        assert_eq!(
            compute_count.get(),
            2,
            "pane 2 should not hit pane 1's cache entry"
        );
        assert_ne!(hash1, hash2, "different panes should have different hashes");

        // Both should now have entries
        let cached_hash1 = shape_hash_lookup(&mut cache, key1, 1, || {
            panic!("should hit cache for pane 1")
        });
        let cached_hash2 = shape_hash_lookup(&mut cache, key2, 1, || {
            panic!("should hit cache for pane 2")
        });

        assert_eq!(cached_hash1, hash1, "pane 1 should have its own entry");
        assert_eq!(cached_hash2, hash2, "pane 2 should have its own entry");

        println!("✓ Test passed: cache key correctly includes pane_id");
    }

    /// Task #476 regression: end-to-end check that the *production*
    /// `shape_hash_lookup` never serves a stale hash for a row of a real
    /// `Terminal`.
    ///
    /// `ShapeHashCacheKey{pane_id, stable_row}` validated by
    /// `entry.seqno == line.current_seqno()` is only sound if the terminal
    /// model guarantees that `(StableRowIndex, seqno)` identifies a unique
    /// line content. `Screen::scroll_up` used to break that guarantee: with
    /// a top-anchored scroll region that stops short of the bottom of the
    /// screen (`CSI 1;Nr`, N < rows) and a full scrollback, every scroll
    /// advanced `stable_row_index_offset` for the whole screen while the
    /// rows *below* the region stayed physically put and were never
    /// dirtied. Their StableRowIndex therefore slid by one per scroll with
    /// no seqno change, so this cache would serve one row's shaping for a
    /// different row -- a line duplicated onto a neighbouring row that no
    /// amount of further scrolling could clear.
    ///
    /// This drives a real `Terminal` with real escape sequences, feeds the
    /// real `Line`s and their real seqnos through the real
    /// `shape_hash_lookup`, and asserts the answer always matches a freshly
    /// computed `Line::compute_shape_hash` for the line actually being
    /// rendered. It fails (many rows, every frame) without the
    /// `Screen::scroll_up` fix.
    #[test]
    fn test_shape_hash_lookup_never_stale_under_top_anchored_scroll_region() {
        use lfucache::LfuCache;
        use wezterm_term::color::ColorPalette;
        use wezterm_term::{Terminal, TerminalConfiguration, TerminalSize};

        #[derive(Debug)]
        struct Cfg;
        impl TerminalConfiguration for Cfg {
            fn scrollback_size(&self) -> usize {
                10
            }
            fn color_palette(&self) -> ColorPalette {
                ColorPalette::default()
            }
        }

        const ROWS: usize = 10;
        const PANE_ID: PaneId = 7;

        /// One frame of the renderer's per-row work: for every visible row,
        /// ask the production cache for that row's shape hash and check it
        /// against the hash of the line that row actually holds right now.
        fn render_frame(
            term: &Terminal,
            cache: &mut LfuCache<ShapeHashCacheKey, ShapeHashEntry>,
            label: &str,
        ) {
            let screen = term.screen();
            let top = screen.visible_row_to_stable_row(0);
            for i in 0..ROWS {
                let stable_row = top + i as StableRowIndex;
                let phys = match screen.stable_row_to_phys(stable_row) {
                    Some(phys) => phys,
                    None => continue,
                };
                let line = screen.lines_in_phys_range(phys..phys + 1).remove(0);
                let truth = line.compute_shape_hash();
                let served = shape_hash_lookup(
                    cache,
                    ShapeHashCacheKey {
                        pane_id: PANE_ID,
                        stable_row,
                    },
                    line.current_seqno(),
                    || line.compute_shape_hash(),
                );
                assert_eq!(
                    served,
                    truth,
                    "[{}] stable_row={} (seqno={}) was served a stale shape hash; \
                     the row actually contains {:?}",
                    label,
                    stable_row,
                    line.current_seqno(),
                    line.as_str(),
                );
            }
        }

        let mut term = Terminal::new(
            TerminalSize {
                rows: ROWS,
                cols: 20,
                pixel_width: 160,
                pixel_height: 160,
                dpi: 0,
            },
            Arc::new(Cfg),
            "OnlyTerm",
            "0",
            Box::new(Vec::new()),
        );

        let mut cache = LfuCache::new(
            "t476_hit",
            "t476_miss",
            test_cache_capacity,
            &ConfigHandle::default_config(),
        );

        // Fill the screen and overflow the scrollback so that subsequent
        // scrolls have to recycle lines off the front of the buffer.
        for i in 0..25 {
            term.advance_bytes(format!("row{:02}\r\n", i));
        }
        render_frame(&term, &mut cache, "filled");

        // Top-anchored scroll region covering only the upper half of the
        // screen, leaving rows 5..10 below it untouched.
        term.advance_bytes("\x1b[1;5r");

        for step in 0..12 {
            // Newline on the last row of the region scrolls the region.
            term.advance_bytes(format!("\x1b[5;1Hnew{:02}\n", step));
            render_frame(&term, &mut cache, &format!("region scroll {}", step));
        }
    }
}
