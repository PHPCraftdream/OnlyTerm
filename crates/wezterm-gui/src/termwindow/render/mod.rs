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
#[derive(Debug)]
pub struct CachedLineState {
    pub id: u64,
    pub seqno: SequenceNo,
    pub shape_hash: [u8; 16],
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
    pub top_pixel_y: NotNan<f32>,
    pub left_pixel_x: NotNan<f32>,
    pub phys_line_idx: usize,
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

pub struct LineQuadCacheValue {
    pub expires: Option<Instant>,
    pub layers: HeapQuadAllocator,
    // Only set if the line contains any hyperlinks, so
    // that we can invalidate when it changes
    pub current_highlight: Option<Arc<Hyperlink>>,
    pub invalidate_on_hover_change: bool,
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
        if let Some(render_state) = self.render_state.as_mut() {
            render_state.recreate_texture_atlas(&self.fonts, &self.render_metrics, size)?;
        }
        Ok(())
    }

    fn shape_hash_for_line(&mut self, line: &Line) -> [u8; 16] {
        let seqno = line.current_seqno();
        let mut id = None;
        if let Some(cached_arc) = line.get_appdata() {
            if let Some(line_state) = cached_arc.downcast_ref::<CachedLineState>() {
                if line_state.seqno == seqno {
                    // Touch the LRU
                    self.line_state_cache.borrow_mut().get(&line_state.id);
                    return line_state.shape_hash;
                }
                id.replace(line_state.id);
            }
        }

        let id = id.unwrap_or_else(|| {
            let id = self.next_line_state_id;
            self.next_line_state_id += 1;
            id
        });

        let shape_hash = line.compute_shape_hash();

        let state = Arc::new(CachedLineState {
            id,
            seqno,
            shape_hash,
        });

        line.set_appdata(Arc::clone(&state));

        self.line_state_cache.borrow_mut().put(id, state);
        shape_hash
    }
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

/// LRU cache wrapper that preserves the metrics behavior of LfuCacheU64
/// by manually emitting hit/miss metrics, similar to the original implementation.
pub struct LruCacheWithMetrics<V> {
    inner: lru::LruCache<u64, V>,
    hit_metric: &'static str,
    miss_metric: &'static str,
    capacity_fn: fn(&ConfigHandle) -> usize,
    config: ConfigHandle,
}

impl<V: Clone> LruCacheWithMetrics<V> {
    pub fn new(
        hit_metric: &'static str,
        miss_metric: &'static str,
        capacity_fn: fn(&ConfigHandle) -> usize,
        config: &ConfigHandle,
    ) -> Self {
        let capacity = capacity_fn(config);
        let capacity_nonzero = std::num::NonZeroUsize::new(capacity)
            .unwrap_or(std::num::NonZeroUsize::new(1).unwrap());
        Self {
            inner: lru::LruCache::new(capacity_nonzero),
            hit_metric,
            miss_metric,
            capacity_fn,
            config: config.clone(),
        }
    }

    fn get(&mut self, k: &u64) -> Option<&V> {
        let result = self.inner.get(k);
        if result.is_some() {
            metrics::histogram!(self.hit_metric).record(1.0);
        } else {
            metrics::histogram!(self.miss_metric).record(1.0);
        }
        result
    }

    fn put(&mut self, k: u64, v: V) {
        self.inner.put(k, v);
    }

    #[allow(dead_code)]
    fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn update_config(&mut self, config: &ConfigHandle) {
        let new_capacity = (self.capacity_fn)(config);
        self.config = config.clone();

        // `lru::LruCache::resize` both grows and shrinks the actual cap
        // used by future `put`s (evicting immediately if the new cap is
        // smaller than the current length). The previous manual
        // `pop_lru`-until-under-cap loop only ever shrank the *current
        // length*, never the cache's own stored capacity -- so a config
        // reload to a smaller size would look like it worked (the loop
        // evicted down to the target) but every `put` after that would
        // immediately be allowed to grow the cache straight back to the
        // old, larger cap, and a reload to a *larger* size would never
        // grow the cache at all, since nothing here changed what the
        // cache itself considered full.
        let new_capacity = std::num::NonZeroUsize::new(new_capacity)
            .unwrap_or(std::num::NonZeroUsize::new(1).unwrap());
        self.inner.resize(new_capacity);
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
}
