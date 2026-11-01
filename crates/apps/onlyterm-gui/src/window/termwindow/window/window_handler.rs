use super::*;
use crate::frontend::front_end;
use anyhow::Context;
use onlyterm_mux::Mux;
use std::time::Instant;

impl TermWindow {
    pub(super) fn load_os_parameters(&mut self) {
        if let Some(ref window) = self.window {
            self.os_parameters = match window.get_os_parameters(&self.config, self.window_state) {
                Ok(os_parameters) => os_parameters,
                Err(err) => {
                    log::warn!("Error while getting OS parameters: {:#}", err);
                    None
                }
            };
        }
    }

    // OnlyTerm: never prompt on window close - close-confirmation overlays
    // are removed entirely, not just defaulted off via config.
    pub(super) fn close_requested(&mut self, window: &Window) {
        let mux = Mux::get();
        mux.kill_window(self.mux_window_id);
        window.close();
        front_end().forget_known_window(window);
    }

    pub(super) fn focus_changed(&mut self, focused: bool, window: &Window) {
        log::trace!("Setting focus to {:?}", focused);
        self.focused = if focused { Some(Instant::now()) } else { None };
        let old_quad_generation = self.quad_generation;
        self.quad_generation += 1;
        log::info!(
            "focus_changed: focused={} quad_generation {} -> {}",
            focused,
            old_quad_generation,
            self.quad_generation
        );
        self.load_os_parameters();
        if self.focused.is_none() {
            self.last_mouse_click = None;
            let outcome = self.pass_through.focus_lost();
            self.pending_pass_through = None;
            if let Some(armed) = outcome.armed_edge {
                log::debug!(
                    "diag: pass-through {} (focus lost)",
                    if armed { "armed" } else { "disarmed" }
                );
                self.invalidate_fancy_tab_bar();
            }
            self.current_mouse_buttons.clear();
            self.current_mouse_capture = None;
            self.is_click_to_focus_window = false;
            self.suppress_move_after_focus_click = None;
            for state in self.pane_state.borrow_mut().values_mut() {
                state.mouse_terminal_coords.take();
            }
        }
        self.prev_cursor.bump();
        window.invalidate();
        if let Some(pane) = self.get_active_pane_or_overlay() {
            pane.focus_changed(focused);
        }
        self.update_title();
    }

    pub(super) fn refresh_renderer_info(&mut self) {
        let hosted_info = self
            .render_thread
            .as_ref()
            .and_then(|backend| backend.renderer_info())
            .map(|info| {
                format!(
                    "WebGPU: {}",
                    onlyterm_gpu_render::wire_adapter_info_to_gpu_info(info)
                )
            });
        let context_info = self
            .render_state
            .as_ref()
            .and_then(|state| state.context.renderer_info());
        if let Some(info) = hosted_info.or(context_info) {
            self.renderer_info.replace(info);
        } else if self
            .render_state
            .as_ref()
            .is_some_and(|state| state.context.webgpu().is_none())
            || self.webgpu.is_none()
        {
            self.renderer_info.replace("WebGPU: n/a".to_string());
        }
    }

    pub(super) fn created(&mut self, ctx: RenderContext) -> anyhow::Result<()> {
        self.render_state = None;
        let render_info = self
            .render_thread
            .as_ref()
            .and_then(|backend| backend.renderer_info())
            .map(|info| {
                format!(
                    "WebGPU: {}",
                    onlyterm_gpu_render::wire_adapter_info_to_gpu_info(info)
                )
            })
            .or_else(|| ctx.renderer_info())
            .unwrap_or_else(|| "WebGPU: n/a".to_string());
        self.renderer_info.replace(render_info.clone());
        let mirror_atlas = self.wants_gpu_atlas_mirroring();
        match RenderState::new(
            ctx,
            &self.fonts,
            &self.render_metrics,
            ATLAS_SIZE,
            mirror_atlas,
        ) {
            Ok(render_state) => {
                log::debug!(
                    "Renderer initialized! {} onlyterm version: {}",
                    render_info,
                    onlyterm_config::onlyterm_version()
                );
                self.render_state.replace(render_state);
                self.atlas_generation = self.atlas_generation.wrapping_add(1);
                self.invalidate_atlas_dependent_caches();

                // A working renderer is now installed, but -- unlike before
                // task #425 -- the Windows `WM_ERASEBKGND` placeholder
                // (task #330; see `WindowOps::clear_placeholder_
                // background`'s doc comment) is deliberately NOT cleared
                // here. `RenderState::new` succeeding only means the GPU
                // pipeline/atlas exist; on the WebGpu path (the default) the
                // per-window render thread is spawned strictly *after* this
                // function returns (see `new_window`), and even the
                // synchronous fallback path still needs a `WM_PAINT` message
                // the OS message loop hasn't dispatched yet to actually
                // build and submit the first real frame. Clearing here (the
                // pre-#425 behavior) tore down the only thing painting the
                // client area before any real content was ever queued for
                // presentation, leaving a gap where the window showed
                // undefined swapchain contents -- typically a black flash --
                // between the placeholder disappearing and the first real
                // frame landing. `paint_impl` does the actual clear (gated
                // on `placeholder_cleared`) on the synchronous no-render-
                // thread path; on the render-thread path (the Windows
                // default), `renderthread.rs`'s `submit_one_frame` does it
                // instead, right after its own first successful
                // `submit_frame` -- task #407 found that clearing as soon as
                // a frame was merely handed off/enqueued to the render
                // thread, rather than actually presented, still left the
                // same undefined-swapchain-contents gap on that path. See
                // `WindowOps::clear_placeholder_background`'s doc comment
                // for both call sites.
                //
                // `window.invalidate()` still happens unconditionally here:
                // `created` is the single funnel every renderer (re)build
                // path goes through (initial `new_window` creation and
                // `finish_renderer_rebuild`), and a forced repaint is
                // exactly as necessary as before for both -- only the
                // placeholder teardown moved. The window
                // may already be visible (task #331/early show) with
                // nothing queued to trigger a repaint yet, so this ensures
                // the first (or, for a rebuild, next) real frame appears
                // immediately instead of waiting for some unrelated event
                // (focus change, resize, blink tick, ...).
                if let Some(window) = self.window.as_ref() {
                    window.invalidate();
                }
            }
            Err(err) => {
                log::error!("failed to create RenderState: {}", err);
                return Err(err)
                    .context(format!("failed to create RenderState for {}", render_info));
            }
        }
        Ok(())
    }
}
