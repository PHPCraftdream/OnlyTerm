use crate::quad::{HeapQuadAllocator, HeapQuadAllocatorExt, QuadTrait, TripleLayerQuadAllocator};
use crate::selection::SelectionRange;
use crate::termwindow::box_model::*;
use crate::termwindow::render::budget::{RowAction, RowSweep};
use crate::termwindow::render::{
    bidi_disabled_by_foreground_process, same_hyperlink, CursorProperties, LineQuadCacheKey,
    LineQuadCacheValue, LineToEleShapeCacheKey, RenderScreenLineParams,
};
use crate::termwindow::{ScrollHit, UIItem, UIItemType};
use ::window::bitmaps::TextureRect;
use ::window::DeadKeyStatus;
use anyhow::Context;
use config::VisualBellTarget;
use mux::pane::PaneId;
use mux::renderable::{RenderableDimensions, StableCursorPosition};
use mux::tab::PositionedPane;
use ordered_float::NotNan;
use std::rc::Rc;
use std::time::Instant;
use wezterm_dynamic::Value;
use wezterm_term::color::{ColorAttribute, ColorPalette};
use wezterm_term::{Line, StableRowIndex};
use window::color::LinearRgba;

impl crate::TermWindow {
    fn paint_pane_box_model(&mut self, pos: &PositionedPane) -> anyhow::Result<()> {
        let computed = self.build_pane(pos)?;
        let mut ui_items = computed.ui_items();
        self.ui_items_scratch.append(&mut ui_items);
        let gl_state = self.render_state.as_ref().unwrap();
        self.render_element(&computed, gl_state, None)
    }

    /// Paint a single pane's content.
    ///
    /// `frame_deadline` is the shared wall-clock deadline (task #251,
    /// `tab_frame_build_budget_ms`) for finishing the *whole* active tab's
    /// content this frame, computed once by `paint_tab_content` -- not a
    /// fresh per-pane budget -- so that a split layout with several panes
    /// can't each consume a full budget's worth of time (which would make
    /// the total unbounded again). `None` means the budget is disabled.
    /// Rows are only ever skipped between whole-row iterations (see
    /// `render_lines`), never partway through shaping a row.
    pub fn paint_pane(
        &mut self,
        pos: &PositionedPane,
        layers: &mut TripleLayerQuadAllocator,
        frame_deadline: Option<Instant>,
    ) -> anyhow::Result<()> {
        if self.config.use_box_model_render {
            // The box-model render path doesn't go through
            // `LineQuadCacheKey`/`LineQuadCacheValue` or any other
            // per-row cache, so there's no existing "reuse last good
            // frame" mechanism to lean on here without inventing a new
            // one from scratch. `tab_frame_build_budget_ms` is therefore
            // not enforced in this legacy/alternate path; it only
            // protects the default row-based renderer below.
            return self.paint_pane_box_model(pos);
        }

        self.check_for_dirty_lines_and_invalidate_selection(&pos.pane);
        /*
        let zone = {
            let dims = pos.pane.get_dimensions();
            let position = self
                .get_viewport(pos.pane.pane_id())
                .unwrap_or(dims.physical_top);

            let zones = self.get_semantic_zones(&pos.pane);
            let idx = match zones.binary_search_by(|zone| zone.start_y.cmp(&position)) {
                Ok(idx) | Err(idx) => idx,
            };
            let idx = ((idx as isize) - 1).max(0) as usize;
            zones.get(idx).cloned()
        };
        */

        let global_cursor_fg = self.palette().cursor_fg;
        let global_cursor_bg = self.palette().cursor_bg;
        let config = self.config.clone();
        let palette = pos.pane.palette();

        let (padding_left, padding_top) = self.padding_left_top();

        let tab_bar_height = if self.show_tab_bar {
            self.tab_bar_pixel_height()
                .context("tab_bar_pixel_height")?
        } else {
            0.
        };
        let (top_bar_height, bottom_bar_height) = if self.config.tab_bar_at_bottom {
            (0.0, tab_bar_height)
        } else {
            (tab_bar_height, 0.0)
        };

        let border = self.get_os_border();
        let top_pixel_y = top_bar_height + padding_top + border.top.get() as f32;

        let cursor = pos.pane.get_cursor_position();
        if pos.is_active {
            self.prev_cursor.update(&cursor);
        }

        let pane_id = pos.pane.pane_id();
        let current_viewport = self.get_viewport(pane_id);
        let dims = pos.pane.get_dimensions();

        let gl_state = self.render_state.as_ref().unwrap();

        let cursor_border_color = palette.cursor_border.to_linear();
        let foreground = palette.foreground.to_linear();
        let white_space = gl_state.util_sprites.white_space.texture_coords();
        let filled_box = gl_state.util_sprites.filled_box.texture_coords();

        let window_is_transparent =
            !self.window_background.is_empty() || config.window_background_opacity != 1.0;

        let default_bg = palette
            .resolve_bg(ColorAttribute::Default)
            .to_linear()
            .mul_alpha(if window_is_transparent {
                0.
            } else {
                config.text_background_opacity
            });

        let cell_width = self.render_metrics.cell_size.width as f32;
        let cell_height = self.render_metrics.cell_size.height as f32;
        let background_rect = {
            // We want to fill out to the edges of the splits
            let (x, width_delta) = if pos.left == 0 {
                (
                    0.,
                    padding_left + border.left.get() as f32 + (cell_width / 2.0),
                )
            } else {
                (
                    padding_left + border.left.get() as f32 - (cell_width / 2.0)
                        + (pos.left as f32 * cell_width),
                    cell_width,
                )
            };

            let (y, height_delta) = if pos.top == 0 {
                (
                    (top_pixel_y - padding_top),
                    padding_top + (cell_height / 2.0),
                )
            } else {
                (
                    top_pixel_y + (pos.top as f32 * cell_height) - (cell_height / 2.0),
                    cell_height,
                )
            };
            euclid::rect(
                x,
                y,
                // Go all the way to the right edge if we're right-most
                if pos.left + pos.width >= self.terminal_size.cols {
                    self.dimensions.pixel_width as f32 - x
                } else {
                    (pos.width as f32 * cell_width) + width_delta
                },
                // Go all the way to the bottom if we're bottom-most
                if pos.top + pos.height >= self.terminal_size.rows {
                    self.dimensions.pixel_height as f32 - y
                } else {
                    (pos.height as f32 * cell_height) + height_delta
                },
            )
        };

        if self.window_background.is_empty() {
            // Per-pane, palette-specified background

            let mut quad = self
                .filled_rectangle(
                    layers,
                    0,
                    background_rect,
                    palette
                        .background
                        .to_linear()
                        .mul_alpha(config.window_background_opacity),
                )
                .context("filled_rectangle")?;
            quad.set_hsv(if pos.is_active {
                None
            } else {
                Some(config.inactive_pane_hsb)
            });
        }

        {
            // If the bell is ringing, we draw another background layer over the
            // top of this in the configured bell color
            if let Some(intensity) = self.get_intensity_if_bell_target_ringing(
                &pos.pane,
                &config,
                VisualBellTarget::BackgroundColor,
            ) {
                // target background color
                let LinearRgba(r, g, b, _) = config
                    .resolved_palette
                    .visual_bell
                    .as_deref()
                    .unwrap_or(&palette.foreground)
                    .to_linear();

                let background = if window_is_transparent {
                    // for transparent windows, we fade in the target color
                    // by adjusting its alpha
                    LinearRgba::with_components(r, g, b, intensity)
                } else {
                    // otherwise We'll interpolate between the background color
                    // and the target color
                    let (r1, g1, b1, a) = palette
                        .background
                        .to_linear()
                        .mul_alpha(config.window_background_opacity)
                        .tuple();
                    LinearRgba::with_components(
                        r1 + (r - r1) * intensity,
                        g1 + (g - g1) * intensity,
                        b1 + (b - b1) * intensity,
                        a,
                    )
                };
                log::trace!("bell color is {:?}", background);

                let mut quad = self
                    .filled_rectangle(layers, 0, background_rect, background)
                    .context("filled_rectangle")?;

                quad.set_hsv(if pos.is_active {
                    None
                } else {
                    Some(config.inactive_pane_hsb)
                });
            }
        }

        // TODO: we only have a single scrollbar in a single position.
        // We only update it for the active pane, but we should probably
        // do a per-pane scrollbar.  That will require more extensive
        // changes to ScrollHit, mouse positioning, PositionedPane
        // and tab size calculation.
        if pos.is_active && self.show_scroll_bar {
            let thumb_y_offset = top_bar_height as usize + border.top.get();

            let min_height = self.min_scroll_bar_height();

            let info = ScrollHit::thumb(
                &*pos.pane,
                current_viewport,
                self.dimensions.pixel_height.saturating_sub(
                    thumb_y_offset + border.bottom.get() + bottom_bar_height as usize,
                ),
                min_height as usize,
            );
            let abs_thumb_top = thumb_y_offset + info.top;
            let thumb_size = info.height;
            let color = palette.scrollbar_thumb.to_linear();

            // Adjust the scrollbar thumb position
            let config = &self.config;
            let padding = self.effective_right_padding(config) as f32;

            let thumb_x = self.dimensions.pixel_width - padding as usize - border.right.get();

            // Register the scroll bar location
            self.ui_items_scratch.push(UIItem {
                x: thumb_x,
                width: padding as usize,
                y: thumb_y_offset,
                height: info.top,
                item_type: UIItemType::AboveScrollThumb,
            });
            self.ui_items_scratch.push(UIItem {
                x: thumb_x,
                width: padding as usize,
                y: abs_thumb_top,
                height: thumb_size,
                item_type: UIItemType::ScrollThumb,
            });
            self.ui_items_scratch.push(UIItem {
                x: thumb_x,
                width: padding as usize,
                y: abs_thumb_top + thumb_size,
                height: self
                    .dimensions
                    .pixel_height
                    .saturating_sub(abs_thumb_top + thumb_size),
                item_type: UIItemType::BelowScrollThumb,
            });

            self.filled_rectangle(
                layers,
                2,
                euclid::rect(
                    thumb_x as f32,
                    abs_thumb_top as f32,
                    padding,
                    thumb_size as f32,
                ),
                color,
            )
            .context("filled_rectangle")?;
        }

        let (selrange, rectangular) = {
            let sel = self.selection(pos.pane.pane_id());
            (sel.range, sel.rectangular)
        };

        let start = Instant::now();
        let selection_fg = palette.selection_fg.to_linear();
        let selection_bg = palette.selection_bg.to_linear();
        let cursor_fg = palette.cursor_fg.to_linear();
        let cursor_bg = palette.cursor_bg.to_linear();
        let cursor_is_default_color =
            palette.cursor_fg == global_cursor_fg && palette.cursor_bg == global_cursor_bg;

        {
            let stable_range = match current_viewport {
                Some(top) => top..top + dims.viewport_rows as StableRowIndex,
                None => dims.physical_top..dims.physical_top + dims.viewport_rows as StableRowIndex,
            };

            pos.pane
                .apply_hyperlinks(stable_range.clone(), &self.config.hyperlink_rules);

            struct LineRender<'a, 'b> {
                term_window: &'a mut crate::TermWindow,
                selrange: Option<SelectionRange>,
                rectangular: bool,
                dims: RenderableDimensions,
                top_pixel_y: f32,
                left_pixel_x: f32,
                pos: &'a PositionedPane,
                pane_id: PaneId,
                cursor: &'a StableCursorPosition,
                palette: &'a ColorPalette,
                default_bg: LinearRgba,
                cursor_border_color: LinearRgba,
                selection_fg: LinearRgba,
                selection_bg: LinearRgba,
                cursor_fg: LinearRgba,
                cursor_bg: LinearRgba,
                foreground: LinearRgba,
                cursor_is_default_color: bool,
                white_space: TextureRect,
                filled_box: TextureRect,
                window_is_transparent: bool,
                layers: &'a mut TripleLayerQuadAllocator<'b>,
                error: Option<anyhow::Error>,
                /// Task #457: progressive sweep state for this pane's row-build budget.
                /// Replaces the old sticky `budget_exceeded: bool` flag with a
                /// bounded-lag policy that guarantees every row is refreshed
                /// within a bounded number of frames and never leaves a row blank.
                sweep: RowSweep,
                /// Task #457: wall-clock deadline for finishing this
                /// pane's row-building loop, derived from
                /// `tab_frame_build_budget_ms`. `None` when the budget is
                /// disabled (config value 0), in which case every row is
                /// always fully (re)built exactly as before this task.
                #[allow(dead_code)] // Used by sweep.decide for over_budget check
                deadline: Option<Instant>,
                /// Whether bidi reordering is disabled because the pane's
                /// live foreground process matches
                /// `disable_bidi_for_processes_named` (e.g. `claude.exe`).
                /// This is a property of the *pane*, not of any individual
                /// line -- computed once per `paint_pane` call rather than
                /// once per row, because `bidi_disabled_by_foreground_process`
                /// -> `divine_foreground_process` deep-clones the OS
                /// process tree (`LocalProcessInfo::clone`, recursive over
                /// `children: HashMap<u32, LocalProcessInfo>`) plus two
                /// mutex locks and several heap allocations. At ~50 rows
                /// this was ~50 process-tree clones per frame -- paid even
                /// on a 100%-cache-hit frame, since it ran before the
                /// `line_quad_cache` lookup -- and was expensive enough in
                /// a debug build to regularly blow the 40ms
                /// `tab_frame_build_budget_ms` deadline on its own while a
                /// process-spawning foreground process (Claude Code) was
                /// running, even with nothing else changing on screen.
                bidi_process_override: bool,
            }

            let left_pixel_x = padding_left
                + border.left.get() as f32
                + (pos.left as f32 * self.render_metrics.cell_size.width as f32);

            let deadline = frame_deadline;

            // Computed once per pane per frame, not once per row -- see the
            // doc comment on `LineRender::bidi_process_override`.
            let bidi_process_override =
                bidi_disabled_by_foreground_process(Some(&pos.pane), &self.config);

            // Task #457: compute the cursor's visible row slot (0-indexed from viewport top),
            // if the cursor is within the current viewport.
            let cursor_row_slot = if cursor.y < dims.physical_top
                || (cursor.y - dims.physical_top) >= dims.viewport_rows as StableRowIndex
            {
                None
            } else {
                Some((cursor.y - dims.physical_top) as usize)
            };

            use crate::termwindow::render::{RetainedPaneRows, RetainedRow, RetainedStamp};

            // Task #457: build the fail-safe stamp for retained row invalidation.
            // This captures everything that would invalidate the *pixels* of a
            // retained row without necessarily changing its text content.
            let current_stamp = RetainedStamp {
                config_generation: self.config.generation(),
                shape_generation: self.shape_generation,
                quad_generation: self.quad_generation,
                pixel_width: self.dimensions.pixel_width,
                pixel_height: self.dimensions.pixel_height,
                cell_height: self.render_metrics.cell_size.height,
                left_pixel_x: NotNan::new(left_pixel_x).unwrap(),
                top_pixel_y: NotNan::new(top_pixel_y).unwrap(),
                num_rows: dims.viewport_rows,
                num_cols: dims.cols,
            };

            // Task #457: get the work_start for the progressive sweep from retained_rows.
            // ALSO handle stamp mismatch by resetting the retained row entry.
            let work_start = {
                let mut retained_rows = self.retained_rows.borrow_mut();
                match retained_rows.get(&pane_id) {
                    Some(existing) => {
                        if existing.stamp == current_stamp {
                            existing.resume_row
                        } else {
                            // Stamp mismatch: reset to a fresh RetainedPaneRows with work_start = 0
                            retained_rows.insert(
                                pane_id,
                                RetainedPaneRows {
                                    stamp: current_stamp.clone(),
                                    rows: vec![None; dims.viewport_rows],
                                    resume_row: 0,
                                },
                            );
                            0
                        }
                    }
                    None => {
                        // No entry yet: initialize with work_start = 0
                        retained_rows.insert(
                            pane_id,
                            RetainedPaneRows {
                                stamp: current_stamp.clone(),
                                rows: vec![None; dims.viewport_rows],
                                resume_row: 0,
                            },
                        );
                        0
                    }
                }
            };

            let mut render = LineRender {
                term_window: self,
                selrange,
                rectangular,
                dims,
                top_pixel_y,
                left_pixel_x,
                pos,
                pane_id,
                cursor: &cursor,
                palette: &palette,
                cursor_border_color,
                selection_fg,
                selection_bg,
                cursor_fg,
                default_bg,
                cursor_bg,
                foreground,
                cursor_is_default_color,
                white_space,
                filled_box,
                window_is_transparent,
                layers,
                error: None,
                deadline,
                sweep: RowSweep::new(deadline, work_start, dims.viewport_rows, cursor_row_slot),
                bidi_process_override,
            };

            impl<'a, 'b> LineRender<'a, 'b> {
                fn render_line(
                    &mut self,
                    stable_top: StableRowIndex,
                    line_idx: usize,
                    line: &Line,
                    is_wrap_continuation: bool,
                ) -> anyhow::Result<()> {
                    let stable_row = stable_top + line_idx as StableRowIndex;
                    let selrange = self
                        .selrange
                        .map_or(0..0, |sel| sel.cols_for_row(stable_row, self.rectangular));
                    // Constrain to the pane width!
                    let selrange = selrange.start..selrange.end.min(self.dims.cols);

                    let (cursor, composing, password_input) = if self.cursor.y == stable_row {
                        (
                            Some(CursorProperties {
                                position: StableCursorPosition {
                                    y: 0,
                                    ..*self.cursor
                                },
                                dead_key_or_leader: self.term_window.dead_key_status
                                    != DeadKeyStatus::None
                                    || self.term_window.leader_is_active(),
                                cursor_fg: self.cursor_fg,
                                cursor_bg: self.cursor_bg,
                                cursor_border_color: self.cursor_border_color,
                                cursor_is_default_color: self.cursor_is_default_color,
                            }),
                            match (self.pos.is_active, &self.term_window.dead_key_status) {
                                (true, DeadKeyStatus::Composing(composing)) => {
                                    Some(composing.to_string())
                                }
                                _ => None,
                            },
                            if self.term_window.config.detect_password_input {
                                match self.pos.pane.get_metadata() {
                                    Value::Object(obj) => {
                                        match obj.get(&Value::String("password_input".to_string()))
                                        {
                                            Some(Value::Bool(b)) => *b,
                                            _ => false,
                                        }
                                    }
                                    _ => false,
                                }
                            } else {
                                false
                            },
                        )
                    } else {
                        (None, None, false)
                    };

                    let shape_hash =
                        self.term_window
                            .shape_hash_for_line(line, self.pane_id, stable_row);
                    // Computed once per pane per frame in `paint_pane`, not
                    // per row -- see `LineRender::bidi_process_override`.
                    let bidi_process_override = self.bidi_process_override;

                    let quad_key = LineQuadCacheKey {
                        pane_id: self.pane_id,
                        password_input,
                        pane_is_active: self.pos.is_active,
                        config_generation: self.term_window.config.generation(),
                        shape_generation: self.term_window.shape_generation,
                        quad_generation: self.term_window.quad_generation,
                        composing: composing.clone(),
                        selection: selrange.clone(),
                        cursor,
                        shape_hash,
                        reverse_video: self.dims.reverse_video,
                        is_wrap_continuation,
                        bidi_process_override,
                    };

                    // Task #457: slot is the 0-indexed row slot from viewport top.
                    // This matches the loop variable `line_idx` in `render_lines`
                    // and is what `RetainedPaneRows.rows` is indexed by.
                    let slot = line_idx;

                    // Task #457: look up whether this slot has retained quads.
                    let has_retained = self
                        .term_window
                        .retained_rows
                        .borrow()
                        .get(&self.pane_id)
                        .and_then(|r| r.rows.get(slot).and_then(|row| row.as_ref()))
                        .is_some();

                    // Task #457: do the cache lookup first, capturing cache_hit and the cached_quad if any.
                    let (cache_hit, maybe_cached_quad) = {
                        let mut cache = self.term_window.line_quad_cache.borrow_mut();
                        let maybe_cached = cache.get(&quad_key).cloned();
                        let cache_hit = maybe_cached
                            .as_ref()
                            .map(|cached_quad| {
                                let expired = cached_quad
                                    .expires
                                    .map(|i| Instant::now() >= i)
                                    .unwrap_or(false);
                                let hover_changed = if cached_quad.invalidate_on_hover_change {
                                    !same_hyperlink(
                                        cached_quad.current_highlight.as_ref(),
                                        self.term_window.current_highlight.as_ref(),
                                    )
                                } else {
                                    false
                                };
                                !expired && !hover_changed
                            })
                            .unwrap_or(false);
                        (cache_hit, maybe_cached)
                    };

                    // Task #457: ask the sweep what to do with this row.
                    let action = self
                        .sweep
                        .decide(slot, cache_hit, has_retained, Instant::now());

                    match action {
                        RowAction::EmitCached => {
                            // This branch is only reachable when cache_hit is true.
                            let cached_quad = maybe_cached_quad.unwrap();
                            // Compute real screen position: quads are built at origin (0,0)
                            let real_top = self.top_pixel_y
                                + (line_idx + self.pos.top) as f32
                                    * self.term_window.render_metrics.cell_size.height as f32;
                            let real_left = self.left_pixel_x;
                            cached_quad
                                .layers
                                .apply_to_translated(self.layers, real_left, real_top)
                                .context("cached_quad.layers.apply_to_translated")?;
                            self.term_window.update_next_frame_time(cached_quad.expires);

                            // Task #457: update retained_rows with this fresh cached data.
                            let retained_row = RetainedRow {
                                quads: Rc::clone(&cached_quad.layers),
                                expires: cached_quad.expires,
                            };
                            if let Some(slot_ref) = self
                                .term_window
                                .retained_rows
                                .borrow_mut()
                                .get_mut(&self.pane_id)
                                .and_then(|r| r.rows.get_mut(slot))
                            {
                                *slot_ref = Some(retained_row);
                            }
                        }
                        RowAction::Build => {
                            // Full rebuild path (existing logic).
                            let mut buf = HeapQuadAllocator::default();
                            let next_due = self.term_window.has_animation.borrow_mut().take();

                            let shape_key = LineToEleShapeCacheKey {
                                shape_hash,
                                shape_generation: quad_key.shape_generation,
                                composing: if self.cursor.y == stable_row && self.pos.is_active {
                                    if let DeadKeyStatus::Composing(composing) =
                                        &self.term_window.dead_key_status
                                    {
                                        Some((self.cursor.x, composing.to_string()))
                                    } else {
                                        None
                                    }
                                } else {
                                    None
                                },
                                is_wrap_continuation,
                                bidi_process_override,
                            };

                            let render_result = self
                                .term_window
                                .render_screen_line(
                                    RenderScreenLineParams {
                                        top_pixel_y: 0.0,
                                        left_pixel_x: 0.0,
                                        pixel_width: self.dims.cols as f32
                                            * self.term_window.render_metrics.cell_size.width
                                                as f32,
                                        stable_line_idx: Some(stable_row),
                                        line,
                                        selection: selrange.clone(),
                                        cursor: self.cursor,
                                        palette: self.palette,
                                        dims: &self.dims,
                                        config: &self.term_window.config,
                                        cursor_border_color: self.cursor_border_color,
                                        foreground: self.foreground,
                                        is_active: self.pos.is_active,
                                        pane: Some(&self.pos.pane),
                                        selection_fg: self.selection_fg,
                                        selection_bg: self.selection_bg,
                                        cursor_fg: self.cursor_fg,
                                        cursor_bg: self.cursor_bg,
                                        cursor_is_default_color: self.cursor_is_default_color,
                                        white_space: self.white_space,
                                        filled_box: self.filled_box,
                                        window_is_transparent: self.window_is_transparent,
                                        default_bg: self.default_bg,
                                        font: None,
                                        style: None,
                                        use_pixel_positioning: self
                                            .term_window
                                            .config
                                            .experimental_pixel_positioning,
                                        render_metrics: self.term_window.render_metrics,
                                        shape_key: Some(shape_key),
                                        password_input,
                                        is_wrap_continuation,
                                        bidi_process_override,
                                    },
                                    &mut TripleLayerQuadAllocator::Heap(&mut buf),
                                )
                                .context("render_screen_line")?;

                            let expires = self.term_window.has_animation.borrow().as_ref().cloned();
                            self.term_window.update_next_frame_time(next_due);

                            // Compute real screen position: quads are built at origin (0,0)
                            let real_top = self.top_pixel_y
                                + (line_idx + self.pos.top) as f32
                                    * self.term_window.render_metrics.cell_size.height as f32;
                            let real_left = self.left_pixel_x;
                            buf.apply_to_translated(self.layers, real_left, real_top)
                                .context("HeapQuadAllocator::apply_to_translated")?;

                            // Store into line_quad_cache (existing).
                            let quads = Rc::new(buf);
                            let quad_value = LineQuadCacheValue {
                                layers: Rc::clone(&quads),
                                expires,
                                invalidate_on_hover_change: render_result
                                    .invalidate_on_hover_change,
                                current_highlight: if render_result.invalidate_on_hover_change {
                                    self.term_window.current_highlight.clone()
                                } else {
                                    None
                                },
                            };
                            self.term_window
                                .line_quad_cache
                                .borrow_mut()
                                .put(quad_key, quad_value);

                            // Task #457: ALSO store into retained_rows.
                            let retained_row = RetainedRow { quads, expires };
                            if let Some(slot_ref) = self
                                .term_window
                                .retained_rows
                                .borrow_mut()
                                .get_mut(&self.pane_id)
                                .and_then(|r| r.rows.get_mut(slot))
                            {
                                *slot_ref = Some(retained_row);
                            }
                        }
                        RowAction::EmitRetained => {
                            // Task #457: fetch retained quads and re-emit them.
                            let retained_rows = self.term_window.retained_rows.borrow();
                            let retained_row = retained_rows
                                .get(&self.pane_id)
                                .and_then(|r| r.rows.get(slot))
                                .and_then(|r| r.as_ref())
                                .context("EmitRetained without retained quads for this slot")?;
                            // Compute real screen position: quads are built at origin (0,0)
                            let real_top = self.top_pixel_y
                                + (line_idx + self.pos.top) as f32
                                    * self.term_window.render_metrics.cell_size.height as f32;
                            let real_left = self.left_pixel_x;
                            retained_row
                                .quads
                                .apply_to_translated(self.layers, real_left, real_top)
                                .context("retained.quads.apply_to_translated")?;
                            self.term_window
                                .update_next_frame_time(retained_row.expires);
                        }
                    }

                    Ok(())
                }
            }

            impl<'a, 'b> LineRender<'a, 'b> {
                fn render_lines(&mut self, stable_top: StableRowIndex, lines: &[Line]) {
                    for line_idx in 0..lines.len() {
                        // A physical row only ever sees its own cells, so
                        // it can't tell on its own whether a Hebrew phrase
                        // touching its first cell is really a fresh
                        // phrase or the tail of one that wrapped down from
                        // the row above -- check the previous *visible*
                        // physical row directly (if any is currently on
                        // screen) rather than guessing.
                        //
                        // That lookup depends on which lines happen to be
                        // in the currently visible window, which shifts
                        // every time the pane scrolls -- so gate it behind
                        // a cheap check of this line's own first cell.
                        // Lines that don't start with Hebrew content can
                        // never be affected by wrap-continuation either
                        // way, so their `is_wrap_continuation` can stay a
                        // constant `false` that doesn't depend on scroll
                        // position, instead of flip-flopping every time
                        // the visible window shifts by one row and
                        // needlessly invalidating their shape/quad cache
                        // entries (this caused a visible scroll stutter).
                        let line = &lines[line_idx];
                        let first_cell_is_hebrew = line
                            .get_cell(0)
                            .map(|c| {
                                wezterm_surface::cellcluster::CellCluster::is_hebrew_cell(c.str())
                            })
                            .unwrap_or(false);
                        let is_wrap_continuation = first_cell_is_hebrew
                            && line_idx > 0
                            && lines[line_idx - 1].last_cell_was_wrapped();

                        if let Err(err) =
                            self.render_line(stable_top, line_idx, line, is_wrap_continuation)
                        {
                            self.error.replace(err);
                            return;
                        }
                    }
                }
            }

            // Snapshot the lines (a cheap clone of each `Line`) before doing
            // any shaping/quad-building: `Pane::get_lines` only holds the
            // pane's `Mutex<Terminal>` for the duration of that clone, unlike
            // the old `with_lines_mut` callback, which held the lock for the
            // whole render loop below and could starve the pty reader/parser
            // and input handling for as long as a frame took to build.
            let (stable_top, lines) = pos.pane.get_lines(stable_range.clone());
            render.render_lines(stable_top, &lines);
            let sweep_outcome = render.sweep.finish();

            // Task #457: record how many rows were actually built this sweep for diagnostics.
            metrics::histogram!("gui.paint.rows_built_per_sweep")
                .record(sweep_outcome.built_count as f64);

            if let Some(error) = render.error.take() {
                return Err(error).context("error while calling get_lines");
            }

            // Task #457: update retained_rows.resume_row for the next frame.
            if let Some(r) = self.retained_rows.borrow_mut().get_mut(&pane_id) {
                r.resume_row = sweep_outcome.next_resume;
            }

            // Task #251/#269: reflect whether this pane's content could be
            // fully (re)built within `tab_frame_build_budget_ms` through
            // the render-budget half of `is_unresponsive()` (OR'd with the
            // unrelated `terminal.lock()` timeout signal from task
            // #246/#248 at the read site) -- set it when the budget was
            // exceeded, and clear it back once a frame completes without
            // exceeding it, mirroring `try_lock_terminal_for`'s
            // set-on-timeout/clear-on-success behavior so this half of the
            // flag reflects only the most recent attempt rather than
            // latching permanently after one slow frame. This writes a
            // dedicated `render_budget_exceeded` cell, not the lock-timeout
            // one, precisely so this per-frame write (unconditional, ~60
            // times/second, `false` far more often than `true`) can never
            // clobber a genuine concurrent lock-timeout `true` for this
            // same pane (see task #269).
            //
            // Task #457: use `deferred_any` instead of `budget_exceeded`. The old
            // sticky flag only tracked whether the deadline passed, not whether
            // any row actually failed to refresh. With the progressive sweep,
            // a deadline that passes during a 100%-cache-hit frame should NOT
            // count toward unresponsiveness, but a frame that defers any row
            // SHOULD.
            pos.pane
                .set_render_budget_exceeded(sweep_outcome.deferred_any);

            // Task #457: schedule a follow-up repaint if any row was deferred.
            // This fixes Cause B's livelock by ensuring forward progress even
            // when the deadline is consistently tripped.
            if sweep_outcome.deferred_any {
                let interval_ms = 1000 / self.config.max_fps.max(1);
                let next_due = Instant::now() + std::time::Duration::from_millis(interval_ms);
                self.schedule_budget_repaint(next_due);
            }
        }

        /*
        if let Some(zone) = zone {
            // TODO: render a thingy to jump to prior prompt
        }
        */
        metrics::histogram!("paint_pane.lines").record(start.elapsed());
        log::trace!("lines elapsed {:?}", start.elapsed());

        Ok(())
    }

    pub fn build_pane(&mut self, pos: &PositionedPane) -> anyhow::Result<ComputedElement> {
        // First compute the bounds for the pane background

        let cell_width = self.render_metrics.cell_size.width as f32;
        let cell_height = self.render_metrics.cell_size.height as f32;
        let (padding_left, padding_top) = self.padding_left_top();
        let tab_bar_height = if self.show_tab_bar {
            self.tab_bar_pixel_height()?
        } else {
            0.
        };
        let (top_bar_height, _bottom_bar_height) = if self.config.tab_bar_at_bottom {
            (0.0, tab_bar_height)
        } else {
            (tab_bar_height, 0.0)
        };

        let border = self.get_os_border();
        let top_pixel_y = top_bar_height + padding_top + border.top.get() as f32;

        // We want to fill out to the edges of the splits
        let (x, width_delta) = if pos.left == 0 {
            (
                0.,
                padding_left + border.left.get() as f32 + (cell_width / 2.0),
            )
        } else {
            (
                padding_left + border.left.get() as f32 - (cell_width / 2.0)
                    + (pos.left as f32 * cell_width),
                cell_width,
            )
        };

        let (y, height_delta) = if pos.top == 0 {
            (
                (top_pixel_y - padding_top),
                padding_top + (cell_height / 2.0),
            )
        } else {
            (
                top_pixel_y + (pos.top as f32 * cell_height) - (cell_height / 2.0),
                cell_height,
            )
        };

        let background_rect = euclid::rect(
            x,
            y,
            // Go all the way to the right edge if we're right-most
            if pos.left + pos.width >= self.terminal_size.cols {
                self.dimensions.pixel_width as f32 - x
            } else {
                (pos.width as f32 * cell_width) + width_delta
            },
            // Go all the way to the bottom if we're bottom-most
            if pos.top + pos.height >= self.terminal_size.rows {
                self.dimensions.pixel_height as f32 - y
            } else {
                (pos.height as f32 * cell_height) + height_delta
            },
        );

        // Bounds for the terminal cells
        let content_rect = euclid::rect(
            padding_left + border.left.get() as f32 - (cell_width / 2.0)
                + (pos.left as f32 * cell_width),
            top_pixel_y + (pos.top as f32 * cell_height) - (cell_height / 2.0),
            pos.width as f32 * cell_width,
            pos.height as f32 * cell_height,
        );

        let palette = pos.pane.palette();

        // TODO: visual bell background layer
        // TODO: scrollbar

        Ok(ComputedElement {
            item_type: None,
            zindex: 0,
            bounds: background_rect,
            border: PixelDimension::default(),
            border_rect: background_rect,
            border_corners: None,
            colors: ElementColors {
                border: BorderColor::default(),
                bg: if self.window_background.is_empty() {
                    palette
                        .background
                        .to_linear()
                        .mul_alpha(self.config.window_background_opacity)
                        .into()
                } else {
                    InheritableColor::Inherited
                },
                text: InheritableColor::Inherited,
            },
            hover_colors: None,
            padding: background_rect,
            content_rect,
            baseline: 1.0,
            content: ComputedElementContent::Children(vec![]),
        })
    }
}
