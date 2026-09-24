use crate::quad::TripleLayerQuadAllocator;
use crate::termwindow::{RenderFrame, TermWindowNotif};
use ::window::bitmaps::atlas::OutOfTextureSpace;
use ::window::WindowOps;
use anyhow::Context;
use mux::tab::PositionedPane;
use onlyterm_font::ClearShapeCache;
use smol::Timer;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AllowImage {
    Yes,
    Scale(usize),
    No,
}

fn atlas_retry_size(pass: usize, current: usize, requested: usize) -> usize {
    // Retain glyphs across CJK pages before falling back to eviction.
    // A 2048-square RGBA atlas costs 16 MiB on the rendering device.
    if pass > 0 || (current < 2048 && requested <= 2048) {
        requested
    } else {
        current
    }
}

impl crate::TermWindow {
    pub fn paint_impl(&mut self, frame: &mut RenderFrame) {
        // Check render-thread back-pressure BEFORE building the frame.
        // `paint_pass` below is the expensive part -- shaping, quad
        // building and signature hashing for every visible row -- and if
        // the previously submitted frame is still in flight, `call_draw`
        // would throw all of that work away (see the is_in_flight()
        // bailout in call_draw_webgpu), only for identical work to be
        // redone once the in-flight frame is acknowledged. When the
        // render thread is backpressured, build nothing: mark the
        // repaint as pending and return; the ack will invalidate the
        // window and we will paint then.
        if let Some(rt) = self.render_thread.as_ref() {
            if rt.is_in_flight() {
                rt.set_repaint_pending();
                metrics::counter!("gui.render_thread.frames_dropped").increment(1);
                // Same lost-wakeup double-check as in call_draw_webgpu:
                // the render thread can finish (and check repaint_pending)
                // in the gap between the two calls above, observing
                // repaint_pending as not yet set and skipping its own
                // invalidate(). Check again: if it's no longer in flight,
                // request a repaint ourselves.
                if !rt.is_in_flight() {
                    if let Some(win) = self.window.as_ref() {
                        win.invalidate();
                    }
                }
                return;
            }
        }
        self.num_frames += 1;
        // If nothing on screen needs animating, then we can avoid
        // invalidating as frequently
        *self.has_animation.borrow_mut() = None;
        // Start with the assumption that we should allow images to render
        self.allow_images = AllowImage::Yes;

        let start = Instant::now();

        {
            let diff = start.duration_since(self.last_fps_check_time);
            if diff > Duration::from_secs(1) {
                let seconds = diff.as_secs_f32();
                self.fps = self.num_frames as f32 / seconds;
                self.num_frames = 0;
                self.last_fps_check_time = start;
            }
        }

        // Task #472: `ClearShapeCache` (unlike `OutOfTextureSpace`, which
        // self-limits via the `AllowImage` ladder below -- 4 transitions,
        // Yes -> Scale(2) -> Scale(4) -> Scale(8) -> No, before it gives up)
        // has no built-in bound on how many times it can be requested in a
        // row. Normally it's raised once per pass (font fallback resolution
        // adds a handle, shape caches get cleared, the next pass shapes
        // correctly), but nothing stops some future bug in fallback
        // resolution from requesting it on every single pass forever, which
        // would spin this loop indefinitely inside `WM_PAINT` -- each retry
        // gets its own fresh 40ms budget from `paint_tab_content`, so this
        // isn't just slow, it's unbounded. Cap it at the same depth as the
        // `OutOfTextureSpace` ladder tolerates so the two arms give up after
        // a comparable amount of retrying.
        const MAX_CLEAR_SHAPE_CACHE_RETRIES: u32 = 4;
        let mut clear_shape_cache_retries = 0;

        'pass: for pass in 0.. {
            match self.paint_pass() {
                // A successful pass is always final now. Before the per-layer
                // quad capacity clamp was removed, this arm additionally had
                // to ask `allocated_more_quads()` whether the pass had
                // overflowed a layer's fixed quad capacity and, if so,
                // reallocate and run another pass. Layers no longer have a
                // capacity ceiling -- `WebGpuInstanceBuffer::ensure_capacity`
                // grows the real GPU buffer to whatever was actually
                // collected -- so a pass returning `Ok` has genuinely drawn
                // everything. The retries below (texture atlas growth,
                // shape-cache invalidation) are the only reason this loop
                // still exists.
                Ok(_) => break 'pass,
                Err(err) => {
                    if let Some(&OutOfTextureSpace {
                        size: Some(size),
                        current_size,
                    }) = err.root_cause().downcast_ref::<OutOfTextureSpace>()
                    {
                        let retry_size = atlas_retry_size(pass, current_size, size);
                        log::debug!("recreate glyph atlas {} -> {}", current_size, retry_size);
                        let result = self.recreate_texture_atlas(Some(retry_size));
                        self.invalidate_fancy_tab_bar();
                        self.invalidate_modal();

                        if let Err(err) = result {
                            self.allow_images = match self.allow_images {
                                AllowImage::Yes => AllowImage::Scale(2),
                                AllowImage::Scale(2) => AllowImage::Scale(4),
                                AllowImage::Scale(4) => AllowImage::Scale(8),
                                AllowImage::Scale(8) => AllowImage::No,
                                _ => {
                                    log::error!(
                                        "Failed to {} texture: {}",
                                        if retry_size == current_size {
                                            "clear"
                                        } else {
                                            "resize"
                                        },
                                        err
                                    );
                                    break 'pass;
                                }
                            };

                            log::info!(
                                "Not enough texture space ({:#}); \
                                     will retry render with {:?}",
                                err,
                                self.allow_images,
                            );
                        }
                    } else if err.root_cause().downcast_ref::<ClearShapeCache>().is_some() {
                        clear_shape_cache_retries += 1;
                        if clear_shape_cache_retries > MAX_CLEAR_SHAPE_CACHE_RETRIES {
                            // Task #472: give up rather than spin forever if
                            // something keeps requesting a shape cache clear
                            // on every pass. This is preventive hardening --
                            // there's no known trigger for this today -- but
                            // if there ever is a bug that causes this, we'd
                            // rather drop this one frame than stall
                            // `WM_PAINT` indefinitely.
                            log::error!(
                                "paint_pass requested ClearShapeCache {} times in a \
                                 row; giving up on this frame",
                                clear_shape_cache_retries
                            );
                            break 'pass;
                        }
                        self.invalidate_fancy_tab_bar();
                        self.invalidate_modal();
                        self.shape_generation += 1;
                        self.shape_cache.borrow_mut().clear();
                        self.line_to_ele_shape_cache.borrow_mut().clear();
                        // Task #439: clear shape_hash_cache on ClearShapeCache error
                        self.shape_hash_cache.borrow_mut().clear();
                    } else {
                        log::error!("paint_pass failed: {:#}", err);
                        break 'pass;
                    }
                }
            }
        }
        log::debug!("paint_impl before call_draw elapsed={:?}", start.elapsed());

        let draw_result = self.call_draw(frame);
        if let Err(err) = &draw_result {
            log::error!("call_draw failed: {:#}", err);
        }

        // Task #425: only after a fully-built frame has actually been
        // *presented* is it safe to tear down the Windows GDI placeholder
        // (task #330) that has been the only thing painting this window's
        // client area up to this point. Doing this from `TermWindow::created`
        // instead (the pre-#425 behavior) cleared it as soon as the
        // renderer/pipeline merely *existed*, which on Windows is well before
        // the first real frame is built here: the render thread is spawned
        // strictly after `created` returns, and even the synchronous
        // fallback path still needs a `WM_PAINT` message the OS message loop
        // hadn't dispatched yet at that point.
        //
        // Task #407: "handed off for presentation" is NOT the same event as
        // "presented" when `webgpu_render_thread` is active (the default on
        // Windows) -- `call_draw` -> `RenderThreadHandle::send_frame` only
        // enqueues the frame onto a channel and returns immediately; the
        // actual `WebGpuState::submit_frame` call (which does the real
        // `Queue::submit`/`SurfaceTexture::present`) runs later, on the
        // separate render thread. Clearing the placeholder here, right after
        // the enqueue, tore down the GDI placeholder while the WebGpu child
        // window's swapchain had never actually been presented to yet --
        // during that gap DWM has nothing of this window's own to composite
        // for that region, so it fell back to whatever was visually behind
        // it. Against the desktop that's invisible (both show as "empty"),
        // but with a second OnlyTerm window overlapping underneath, that
        // window's real content showed through -- the startup flicker and
        // "white/transparent rectangles" reported in task #407. So on the
        // render-thread path, clearing is deferred to `submit_one_frame`
        // (see `renderthread.rs`), which calls
        // `Window::clear_placeholder_background` itself immediately after
        // the first successful `submit_frame` -- i.e. after the first real
        // `present()` -- rather than doing it here. The synchronous
        // (non-render-thread) path is unaffected: there, `call_draw`
        // returning `Ok` really does mean `submit_frame`/`present()` already
        // ran inline, so clearing immediately below is still correct and
        // necessary (nothing else will do it for that path).
        //
        // Either way this is gated on `draw_result` being `Ok` -- if the very
        // first `call_draw` fails (e.g. `build_webgpu_frame` erroring before
        // anything was ever queued for presentation), leave the placeholder
        // in place rather than tearing down the only thing painting the
        // window over a frame that was never actually sent; the next
        // successful paint will clear it instead. Checked-then-set so this
        // only ever fires once per window, matching the old call's
        // idempotency (a later renderer rebuild's `created()` call has
        // nothing left to clear -- the placeholder was already dropped by
        // the first successful frame, long before any rebuild).
        if !self.placeholder_cleared && draw_result.is_ok() && self.render_thread.is_none() {
            self.placeholder_cleared = true;
            if let Some(window) = self.window.as_ref() {
                window.clear_placeholder_background();
            }
        }

        self.last_frame_duration = start.elapsed();
        log::debug!(
            "paint_impl elapsed={:?}, fps={}",
            self.last_frame_duration,
            self.fps
        );
        // Note: when `webgpu_render_thread` is active, `call_draw` above
        // only hands the built frame off to the render thread and returns
        // immediately -- actual GPU submission happens asynchronously on
        // that thread. So `last_frame_duration`/`fps`/this histogram no
        // longer include GPU submission time in that mode; they measure
        // frame-build time only, not full frame-build-and-submit time.
        // (Submission time is tracked separately, on the render thread, as
        // `gui.render_thread.submit`.)
        metrics::histogram!("gui.paint.impl").record(self.last_frame_duration);
        metrics::histogram!("gui.paint.impl.rate").record(1.);

        // If self.has_animation is some, then the last render detected
        // image attachments with multiple frames, so we also need to
        // invalidate the viewport when the next frame is due
        if self.focused.is_some() {
            if let Some(next_due) = *self.has_animation.borrow() {
                let prior = self.scheduled_animation.borrow_mut().take();
                match prior {
                    Some(prior) if prior <= next_due => {
                        // Already due before that time
                    }
                    _ => {
                        self.scheduled_animation.borrow_mut().replace(next_due);
                        let window = self.window.clone().unwrap();
                        promise::spawn::spawn(async move {
                            Timer::at(next_due).await;
                            let win = window.clone();
                            window.notify(TermWindowNotif::Apply(Box::new(move |tw| {
                                tw.scheduled_animation.borrow_mut().take();
                                if tw.tab_bar.next_progress_frame_due().is_some() {
                                    // Deliberately NOT the coalesced entry
                                    // point: this timer already fires at
                                    // INDETERMINATE_SPINNER_INTERVAL, which is
                                    // the same 100ms as
                                    // TITLE_UPDATE_MIN_INTERVAL, so gating it
                                    // would drop spinner frames rather than
                                    // save any work.
                                    tw.update_title_post_status();
                                }
                                win.invalidate();
                            })));
                        })
                        .detach();
                    }
                }
            }
        }
    }

    pub fn paint_modal(&mut self) -> anyhow::Result<()> {
        if let Some(modal) = self.get_modal() {
            for computed in modal.computed_element(self)?.iter() {
                let mut ui_items = computed.ui_items();

                let gl_state = self.render_state.as_ref().unwrap();
                self.render_element(computed, gl_state, None)?;

                self.ui_items_scratch.append(&mut ui_items);
            }
        }

        Ok(())
    }

    /// Paint the window's background (image/color fill), gated on the
    /// transparency/background-image config. This is part of the window
    /// "chrome" -- it is not specific to any one pane's content.
    ///
    /// `panes` is the set of panes to be rendered for the active tab, used
    /// here only to pick a suitable background color/viewport when there is
    /// no background image.
    fn paint_window_background(
        &mut self,
        layers: &mut TripleLayerQuadAllocator,
        panes: &[PositionedPane],
    ) -> anyhow::Result<()> {
        let window_is_transparent =
            !self.window_background.is_empty() || self.config.window_background_opacity != 1.0;

        let mut paint_terminal_background = false;

        // Render the full window background
        match (self.window_background.is_empty(), self.allow_images) {
            (false, AllowImage::Yes | AllowImage::Scale(_)) => {
                let bg_color = self.palette().background.to_linear();

                let top = panes
                    .iter()
                    .find(|p| p.is_active)
                    .map(|p| match self.get_viewport(p.pane.pane_id()) {
                        Some(top) => top,
                        None => p.pane.get_dimensions().physical_top,
                    })
                    .unwrap_or(0);

                let loaded_any = self
                    .render_backgrounds(bg_color, top)
                    .context("render_backgrounds")?;

                if !loaded_any {
                    // Either there was a problem loading the background(s)
                    // or they haven't finished loading yet.
                    // Use the regular terminal background until that changes.
                    paint_terminal_background = true;
                }
            }
            _ if window_is_transparent => {
                // Avoid doubling up the background color: the panes
                // will render out through the padding so there
                // should be no gaps that need filling in
            }
            _ => {
                paint_terminal_background = true;
            }
        }

        if paint_terminal_background {
            // Regular window background color
            let background = if panes.len() == 1 {
                // If we're the only pane, use the pane's palette
                // to draw the padding background
                panes[0].pane.palette().background
            } else {
                self.palette().background
            }
            .to_linear()
            .mul_alpha(self.config.window_background_opacity);

            self.filled_rectangle(
                layers,
                0,
                euclid::rect(
                    0.,
                    0.,
                    self.dimensions.pixel_width as f32,
                    self.dimensions.pixel_height as f32,
                ),
                background,
            )
            .context("filled_rectangle for window background")?;
        }

        Ok(())
    }

    /// Paint the active tab's pane content: every visible pane of the
    /// active tab, plus the divider lines between split panes within that
    /// same tab.
    ///
    /// Task #251: this whole call is bounded by a single wall-clock
    /// deadline derived from `tab_frame_build_budget_ms` (disabled when
    /// that config value is 0). The deadline is computed once here, up
    /// front, and threaded down into every pane's `paint_pane` call rather
    /// than each pane getting its own fresh budget -- otherwise a split
    /// layout with N panes could take up to N times the configured budget
    /// in total, which would defeat the point. The actual abort/skip
    /// happens at row granularity inside `paint_pane`/`render_lines`
    /// (checked between whole rows, never mid-shape); this per-pane loop
    /// boundary only matters for split layouts; for the common
    /// single-pane tab it does nothing extra, which is exactly why the
    /// row-level check exists.
    fn paint_tab_content(
        &mut self,
        layers: &mut TripleLayerQuadAllocator,
        panes: &[PositionedPane],
    ) -> anyhow::Result<()> {
        let focused = self.focused.is_some();

        let budget_ms = self.config.tab_frame_build_budget_ms;
        let frame_deadline = if budget_ms > 0 {
            Some(Instant::now() + Duration::from_millis(budget_ms))
        } else {
            None
        };

        for pos in panes {
            if pos.is_active {
                self.update_text_cursor(pos);
                if focused {
                    pos.pane.advise_focus();
                    mux::Mux::get().record_focus_for_current_identity(pos.pane.pane_id());
                }
            }
            self.paint_pane(pos, layers, frame_deadline)
                .context("paint_pane")?;
        }

        if let Some(pane) = self.get_active_pane_or_overlay() {
            let splits = self.get_splits();
            for split in &splits {
                self.paint_split(layers, split, &pane)
                    .context("paint_split")?;
            }
        }

        Ok(())
    }

    /// Paint everything that isn't the active tab's pane content: the tab
    /// bar and the window borders. (The window background is painted
    /// separately, before tab content, and the modal is painted separately,
    /// after this closure returns -- see `paint_pass`.)
    fn paint_window_chrome(&mut self, layers: &mut TripleLayerQuadAllocator) -> anyhow::Result<()> {
        if self.show_tab_bar {
            self.paint_tab_bar(layers).context("paint_tab_bar")?;
        }

        self.paint_window_borders(layers)
            .context("paint_window_borders")?;

        Ok(())
    }

    pub fn paint_pass(&mut self) -> anyhow::Result<()> {
        {
            let gl_state = self.render_state.as_ref().unwrap();
            for layer in gl_state.layers.borrow().iter() {
                layer.clear_quad_allocation();
            }
        }

        // Clear out UI item positions; we'll rebuild these as we render
        self.ui_items_scratch.clear();

        let panes = self.get_panes_to_render();

        let start = Instant::now();
        let gl_state = self.render_state.as_ref().unwrap();
        let layer = gl_state
            .layer_for_zindex(0)
            .context("layer_for_zindex(0)")?;
        log::trace!("quad map elapsed {:?}", start.elapsed());
        metrics::histogram!("quad.map").record(start.elapsed());

        layer.with_quad_allocator(|layers| -> anyhow::Result<()> {
            self.paint_window_background(layers, &panes)?;
            self.paint_tab_content(layers, &panes)?;
            self.paint_window_chrome(layers)?;
            Ok(())
        })?;

        self.paint_modal().context("paint_modal")?;

        // Publish the freshly-built UI items so that mouse hit-testing
        // (which may run on a different thread once rendering is moved
        // off the GUI thread) never has to synchronize with the render
        // pass via a lock.
        self.ui_items.store(std::sync::Arc::new(std::mem::take(
            &mut self.ui_items_scratch,
        )));

        Ok(())
    }
}

#[cfg(test)]
mod atlas_retry_tests {
    use super::atlas_retry_size;

    #[test]
    fn grows_small_glyph_atlases_before_evicting() {
        assert_eq!(atlas_retry_size(0, 128, 256), 256);
        assert_eq!(atlas_retry_size(0, 1024, 2048), 2048);
    }

    #[test]
    fn caps_opportunistic_growth_but_preserves_large_frame_retry() {
        assert_eq!(atlas_retry_size(0, 2048, 4096), 2048);
        assert_eq!(atlas_retry_size(0, 1024, 4096), 1024);
        assert_eq!(atlas_retry_size(1, 2048, 4096), 4096);
    }
}
