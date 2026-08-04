use super::*;
use crate::frontend::front_end;
use anyhow::Context;
use mux::Mux;
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
        self.quad_generation += 1;
        self.load_os_parameters();

        if self.focused.is_none() {
            self.last_mouse_click = None;
            self.current_mouse_buttons.clear();
            self.current_mouse_capture = None;
            self.is_click_to_focus_window = false;
            self.suppress_move_after_focus_click = None;

            for state in self.pane_state.borrow_mut().values_mut() {
                state.mouse_terminal_coords.take();
            }
        }

        // Reset the cursor blink phase
        self.prev_cursor.bump();

        // force cursor to be repainted
        window.invalidate();

        if let Some(pane) = self.get_active_pane_or_overlay() {
            pane.focus_changed(focused);
        }

        self.update_title();
    }

    pub(super) fn created(&mut self, ctx: RenderContext) -> anyhow::Result<()> {
        self.render_state = None;

        let render_info = ctx.renderer_info();
        self.opengl_info.replace(render_info.clone());

        match RenderState::new(ctx, &self.fonts, &self.render_metrics, ATLAS_SIZE) {
            Ok(render_state) => {
                log::debug!(
                    "OpenGL initialized! {} wezterm version: {}",
                    render_info,
                    config::wezterm_version(),
                );
                self.render_state.replace(render_state);

                // A working renderer is now installed and will paint every
                // subsequent frame, so the Windows `WM_ERASEBKGND`
                // placeholder (task #330; see `WindowOps::
                // clear_placeholder_background`'s doc comment) is no longer
                // needed. `created` is the single funnel every renderer
                // (re)build path goes through -- initial `new_window`
                // creation, `finish_opengl_fallback`, and
                // `finish_renderer_rebuild` all call it -- so clearing here
                // covers all three without duplicating the call at each
                // site. No-op on non-Windows platforms and idempotent if a
                // later rebuild calls `created` again.
                if let Some(window) = self.window.as_ref() {
                    window.clear_placeholder_background();
                    // The window may already be visible (task #331/early
                    // show) with nothing queued to trigger a repaint yet.
                    // Force one now so the first real frame appears
                    // immediately instead of waiting for some unrelated
                    // event (focus change, resize, blink tick, ...) to
                    // eventually invalidate the window.
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
