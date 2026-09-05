use crate::colorease::ColorEase;
use crate::customglyph::{BlockKey, *};
use crate::glyphcache::{CachedGlyph, GlyphCache, LoadState};
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
use onlyterm_font::shaper::PresentationWidth;
use onlyterm_font::units::{IntPixelLength, PixelLength};
use onlyterm_font::{ClearShapeCache, GlyphInfo, LoadedFont};
use onlyterm_term::color::{ColorAttribute, ColorPalette};
use onlyterm_term::{CellAttributes, Line, StableRowIndex};
use ordered_float::NotNan;
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;
use termwiz::cellcluster::CellCluster;
use termwiz::hyperlink::Hyperlink;
use termwiz::surface::{CursorShape, CursorVisibility, SequenceNo};
use window::color::LinearRgba;

pub mod borders;
pub mod budget;
mod cells;
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
    /// The stable row index of the top visible slot (`rows[0]`) at the
    /// time the contents of `rows` were recorded. The stamp deliberately
    /// does NOT cover the viewport origin -- scrolling does not invalidate
    /// the pixels of a retained row -- but the slot each row belongs to
    /// moves with the origin, so a change here is handled by re-basing
    /// `rows` via `shift_origin` rather than by a stamp-mismatch reset.
    pub viewport_top: StableRowIndex,
    /// Indexed by visible row slot (`line_idx + pos.top`), NOT by
    /// StableRowIndex -- this must track screen position, not scrollback
    /// content position.
    pub rows: Vec<Option<RetainedRow>>,
    /// Row slot at which the NEXT frame's sweep should start spending its
    /// shaping budget. 0 means "a full clean sweep completed; start from
    /// the top again".
    pub resume_row: usize,
}

impl RetainedPaneRows {
    /// Bug E (investigation `2026-08-25-render-and-resource-bug-hunt`
    /// section 2.1): `rows` is indexed by visible row slot, so when the
    /// viewport origin moves (the pane scrolled by k rows), the content
    /// recorded at slot N now belongs at slot N-k. Re-base the recorded
    /// rows to the new origin; without this, a budget-deferred slot N
    /// could be re-emitted showing its OWN prior content -- duplicating
    /// the line now visible at N-k -- for a frame or two until rotation
    /// catches up.
    ///
    /// Retained quads are position-independent (built at origin (0,0),
    /// translated at emission time), so shifting the recorded rows by the
    /// same delta is semantically exact -- unlike resetting them, which is
    /// safe but discards every retained row on every scroll. Slots that
    /// scroll into view from outside the recorded window become `None`,
    /// which `RowSweep::decide` treats as must-build (never blank).
    pub fn shift_origin(&mut self, new_viewport_top: StableRowIndex) {
        let delta = new_viewport_top - self.viewport_top;
        if delta == 0 {
            return;
        }
        let len = self.rows.len();
        let shift = delta.unsigned_abs();
        if shift >= len {
            // Every previously recorded row scrolled out of the window.
            self.rows.clear();
            self.rows.resize(len, None);
            self.resume_row = 0;
        } else if delta > 0 {
            // Content moved from slot N to slot N - shift: drop the first
            // `shift` slots and append fresh slots at the bottom.
            self.rows.drain(0..shift);
            self.rows.resize(len, None);
            self.resume_row = self.resume_row.saturating_sub(shift);
        } else {
            // Content moved from slot N to slot N + shift: drop the last
            // `shift` slots and prepend fresh slots at the top.
            self.rows.truncate(len - shift);
            self.rows.splice(0..0, (0..shift).map(|_| None));
            self.resume_row = (self.resume_row + shift).min(len);
            if self.resume_row >= len {
                self.resume_row = 0;
            }
        }
        self.viewport_top = new_viewport_top;
    }
}

/// A retained row's quad data and its expiration (if animated).
/// Whether these retained quads have the terminal cursor sprite baked into them (rows whose quads contain a cursor must be rebuilt, never re-emitted stale, once the cursor moves away -- otherwise a ghost cursor block renders at the old row).
#[derive(Clone, Debug)]
pub struct RetainedRow {
    pub quads: Rc<HeapQuadAllocator>,
    pub expires: Option<Instant>,
    pub contains_cursor: bool,
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
    /// The theme's own resolved default foreground/background, used by
    /// `ensure_min_contrast` as the readable fallback pair for a cell whose
    /// computed foreground and background are identical -- see that
    /// function's doc comment.
    pub default_fg: LinearRgba,
    pub default_bg: LinearRgba,
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
    attrs: CellAttributes,
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

    /// Drops everything this window has cached that embeds glyph atlas
    /// coordinates, so that the next frame re-shapes and re-allocates
    /// against whatever atlas is current.
    ///
    /// Anything holding a `Sprite` is stale the moment the atlas it was
    /// allocated from goes away: the sprite is a rectangle *into a
    /// texture*, and drawing it against a different texture samples
    /// whatever happens to live at those coordinates -- in practice solid
    /// blocks of background, since a fresh atlas is empty. That is not
    /// limited to the shape caches: `fancy_tab_bar` holds a whole
    /// `ComputedElement` whose cells are `ElementCell::Sprite`/`Glyph`,
    /// and the modal is the same.
    ///
    /// The caches keyed by `shape_generation` (`line_quad_cache`, the
    /// retained per-row quads) are covered by bumping it; `shape_cache`
    /// is keyed by text and font only, so it has to be cleared outright.
    ///
    /// Call this from every path that installs a different atlas --
    /// including the ones that replace the whole `RenderState`, not just
    /// the ones that resize the atlas in place.
    pub(crate) fn invalidate_atlas_dependent_caches(&mut self) {
        // Reset frame signature on atlas resize - texture content changed,
        // so the previous frame is no longer comparable (task #450)
        self.last_frame_signature = None;
        self.shape_generation += 1;
        self.shape_cache.borrow_mut().clear();
        self.line_to_ele_shape_cache.borrow_mut().clear();
        // Task #439: clear shape_hash_cache on texture atlas resize
        self.shape_hash_cache.borrow_mut().clear();
        self.invalidate_fancy_tab_bar();
        self.invalidate_modal();
    }

    pub fn recreate_texture_atlas(&mut self, size: Option<usize>) -> anyhow::Result<()> {
        self.invalidate_atlas_dependent_caches();
        let mirror_atlas = self.wants_gpu_atlas_mirroring();
        if let Some(render_state) = self.render_state.as_mut() {
            render_state.recreate_texture_atlas(
                &self.fonts,
                &self.render_metrics,
                size,
                mirror_atlas,
            )?;
            // Do not use the Rc allocation address as the identity here:
            // recreating an atlas with the same dimensions can reuse that
            // address, while the child still needs a full mirror reset.
            self.atlas_generation = self.atlas_generation.wrapping_add(1);
        }
        Ok(())
    }

    /// Whether this window's atlas writes should be mirrored
    /// (`WebGpuTexture::enable_mirroring`) for this window's render backend
    /// to consume: true exactly when that backend renders in another
    /// process and therefore keeps its own copy of the atlas
    /// (`HostProcessBackend`), false for the in-process render thread, which
    /// draws from the very `wgpu::Texture` these writes land in.
    ///
    /// `self.render_thread` must already be installed by the time this is
    /// asked, which is why `new_window` sets it *before* calling `created()`
    /// -- see `RenderBackend::wants_atlas_mirroring`'s doc comment for what
    /// answering `false` here by mistake looks like on screen.
    pub(crate) fn wants_gpu_atlas_mirroring(&self) -> bool {
        self.render_thread
            .as_ref()
            .is_some_and(|backend| backend.wants_atlas_mirroring())
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
        onlyterm_term::color::ColorAttribute::Default => {
            if let Some(fg) = style.foreground {
                fg.into()
            } else {
                palette.resolve_fg(attrs.foreground())
            }
        }
        onlyterm_term::color::ColorAttribute::PaletteIndex(idx)
            if idx < 8 && config.bold_brightens_ansi_colors != BoldBrightening::No =>
        {
            // For compatibility purposes, switch to a brighter version
            // of one of the standard ANSI colors when Bold is enabled.
            // This lifts black to dark grey.
            let idx = if attrs.intensity() == onlyterm_term::Intensity::Bold {
                idx + 8
            } else {
                idx
            };

            palette.resolve_fg(onlyterm_term::color::ColorAttribute::PaletteIndex(idx))
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
mod cache_bench;
#[cfg(test)]
mod retained_pane_rows_tests;
