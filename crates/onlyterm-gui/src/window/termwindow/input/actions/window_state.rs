use super::*;

impl TermWindow {
    pub(in crate::termwindow) fn palette(&mut self) -> &ColorPalette {
        if self.palette.is_none() {
            self.palette
                .replace(config::TermConfig::new().color_palette());
        }
        self.palette.as_ref().unwrap()
    }

    pub fn config_was_reloaded(&mut self) {
        log::debug!(
            "config was reloaded, overrides: {:?}",
            self.config_overrides
        );
        self.key_table_state.clear_stack();
        self.connection_name = Connection::get().unwrap().name();
        let config = match config::overridden_config(&self.config_overrides) {
            Ok(config) => config,
            Err(err) => {
                log::error!(
                    "Failed to apply config overrides to window: {:#}: {:?}",
                    err,
                    self.config_overrides
                );
                configuration()
            }
        };
        self.config = config.clone();
        self.palette.take();

        let mux = Mux::get();
        let window = match mux.get_window(self.mux_window_id) {
            Some(window) => window,
            _ => return,
        };
        if window.len() == 1 {
            self.show_tab_bar = config.enable_tab_bar && !config.hide_tab_bar_if_only_one_tab;
        } else {
            self.show_tab_bar = config.enable_tab_bar;
        }
        *self.cursor_blink_state.borrow_mut() = ColorEase::new(
            config.cursor_blink_rate,
            config.cursor_blink_ease_in,
            config.cursor_blink_rate,
            config.cursor_blink_ease_out,
            None,
        );
        *self.blink_state.borrow_mut() = ColorEase::new(
            config.text_blink_rate,
            config.text_blink_ease_in,
            config.text_blink_rate,
            config.text_blink_ease_out,
            None,
        );
        *self.rapid_blink_state.borrow_mut() = ColorEase::new(
            config.text_blink_rate_rapid,
            config.text_blink_rapid_ease_in,
            config.text_blink_rate_rapid,
            config.text_blink_rapid_ease_out,
            None,
        );

        self.show_scroll_bar = config.enable_scroll_bar;
        self.shape_generation += 1;
        {
            let mut shape_cache = self.shape_cache.borrow_mut();
            shape_cache.update_config(&config);
            shape_cache.clear();
        }
        // Task #439: clear shape_hash_cache on config reload
        self.shape_hash_cache.borrow_mut().clear();
        self.line_quad_cache.borrow_mut().update_config(&config);
        self.line_to_ele_shape_cache
            .borrow_mut()
            .update_config(&config);
        self.fancy_tab_bar.take();
        self.invalidate_fancy_tab_bar();
        self.invalidate_modal();
        self.input_map = InputMap::new(&config);
        self.leader_is_down = None;
        // Re-read the pass-through flag so toggling it applies live; disabling
        // also drops any armed mode or partial tap, like focus loss does.
        let outcome = self
            .pass_through
            .set_enabled(config.pass_through_next_key_on_double_ctrl);
        self.pending_pass_through = None;
        if let Some(armed) = outcome.armed_edge {
            log::debug!(
                "diag: pass-through {} (config reload)",
                if armed { "armed" } else { "disarmed" }
            );
        }
        if let Some(rs) = self.render_state.as_mut() {
            rs.config_changed()
        }
        let dimensions = self.dimensions;

        if let Err(err) = self.fonts.config_changed(&config) {
            log::error!("Failed to load font configuration: {:#}", err);
        }

        if let Some(window) = mux.get_window(self.mux_window_id) {
            let term_config: Arc<dyn TerminalConfiguration> =
                Arc::new(TermConfig::with_config(config.clone()));
            for tab in window.iter() {
                for pane in tab.iter_panes_ignoring_zoom() {
                    pane.pane.set_config(Arc::clone(&term_config));
                }
            }
            for state in self.pane_state.borrow().values() {
                if let Some(overlay) = &state.overlay {
                    overlay.pane.set_config(Arc::clone(&term_config));
                }
            }
            for state in self.tab_state.borrow().values() {
                if let Some(overlay) = &state.overlay {
                    overlay.pane.set_config(Arc::clone(&term_config));
                }
            }
        }

        if let Some(window) = self.window.clone() {
            self.load_os_parameters();
            self.apply_scale_change(&dimensions, self.fonts.get_font_scale());
            self.apply_dimensions(&dimensions, None, &window);
            window.config_did_change(&config);
            window.invalidate();
        }

        // Do this after we've potentially adjusted scaling based on config/padding
        // and window size
        self.window_background = reload_background_image(
            &config,
            &self.window_background,
            &self.dimensions,
            &self.render_metrics,
        );

        self.invalidate_modal();
    }

    pub(in crate::termwindow) fn invalidate_modal(&mut self) {
        if let Some(modal) = self.get_modal() {
            modal.reconfigure(self);
            if let Some(window) = self.window.as_ref() {
                window.invalidate();
            }
        }
    }

    pub fn cancel_modal(&self) {
        self.modal.borrow_mut().take();
        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
    }

    pub fn set_modal(&self, modal: Rc<dyn Modal>) {
        self.modal.borrow_mut().replace(modal);
        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
    }

    pub(in crate::termwindow) fn get_modal(&self) -> Option<Rc<dyn Modal>> {
        self.modal.borrow().as_ref().map(Rc::clone)
    }

    pub(super) fn update_scrollbar(&mut self) {
        if !self.show_scroll_bar {
            return;
        }

        let tab = match self.get_active_pane_or_overlay() {
            Some(tab) => tab,
            None => return,
        };

        let render_dims = tab.get_dimensions();
        if render_dims == self.last_scroll_info {
            return;
        }

        self.last_scroll_info = render_dims;

        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
    }

    /// Called by various bits of code to update the title bar.
    pub(in crate::termwindow) fn update_title(&mut self) {
        self.update_title_impl();
    }

    /// Same as `update_title`, but for callers that a running program can
    /// trigger arbitrarily often (mux alerts, user-var events). See
    /// TITLE_UPDATE_MIN_INTERVAL for why only these are rate-limited.
    pub(in crate::termwindow) fn update_title_coalesced(&mut self) {
        self.request_title_update();
    }

    pub(in crate::termwindow) fn window_contains_pane(&mut self, pane_id: PaneId) -> bool {
        let mux = Mux::get();

        let (_domain, window_id, _tab_id) = match mux.resolve_pane_id(pane_id) {
            Some(tuple) => tuple,
            None => return false,
        };

        window_id == self.mux_window_id
    }

    pub(in crate::termwindow) fn emit_user_var_event(
        &mut self,
        pane_id: PaneId,
        _name: String,
        _value: String,
    ) {
        if !self.window_contains_pane(pane_id) {
            return;
        }
        // Shell integration sets user vars from the prompt hook, so a
        // command loop can raise this as fast as it prints.
        self.update_title_coalesced();
    }

    /// Called by window:set_right_status after the status has
    /// been updated; let's update the bar
    pub fn update_title_post_status(&mut self) {
        self.update_title_impl();
    }

    /// Same as `update_title_post_status`, for the mux notifications a
    /// busy pane can raise continuously (WindowInvalidated, PaneFocused,
    /// TabReflowed, TabTitleChanged) and for the progress-bar animation
    /// timer.
    pub(in crate::termwindow) fn update_title_post_status_coalesced(&mut self) {
        self.request_title_update();
    }

    /// Rate-limited entry point for tab-bar/window-title rebuilds, used by
    /// the storm-prone paths only.
    fn request_title_update(&mut self) {
        let now = Instant::now();
        if self
            .title_update_coalescer
            .should_run_now(now, TITLE_UPDATE_MIN_INTERVAL)
        {
            self.update_title_impl();
            return;
        }
        if !self.title_update_coalescer.needs_trailing_arm() {
            // A trailing rebuild is already armed and will observe this
            // request's state when it fires.
            return;
        }
        let deadline = self
            .title_update_coalescer
            .trailing_deadline(TITLE_UPDATE_MIN_INTERVAL);
        match self.window.as_ref() {
            Some(window) => {
                let window = window.clone();
                promise::spawn::spawn(async move {
                    Timer::at(deadline).await;
                    window.notify(TermWindowNotif::Apply(Box::new(|tw| {
                        tw.title_update_coalescer.trailing_fired(Instant::now());
                        tw.update_title_impl();
                    })));
                })
                .detach();
            }
            None => {
                // No window yet (or during teardown): no timer plumbing
                // to defer with, so apply inline rather than drop the
                // update -- the pre-coalescing behavior.
                self.title_update_coalescer.trailing_fired(now);
                self.update_title_impl();
            }
        }
    }

    fn update_title_impl(&mut self) {
        let mux = Mux::get();
        let window = match mux.get_window(self.mux_window_id) {
            Some(window) => window,
            _ => return,
        };
        // Both lists used to call pos_pane_to_pane_info on the same active pane,
        // paying its three bounded terminal.lock() acquisitions twice per update.
        // The active tab's entry is now computed once and shared.
        let panes = self.get_pane_information();
        let active_pane = panes.iter().find(|p| p.is_active).cloned();
        let tabs = self.get_tab_information(active_pane.clone());
        let active_tab = tabs.iter().find(|t| t.is_active).cloned();

        let border = self.get_os_border();
        let tab_bar_height = self.tab_bar_pixel_height().unwrap_or(0.);
        let tab_bar_y = if self.config.tab_bar_at_bottom {
            ((self.dimensions.pixel_height as f32) - (tab_bar_height + border.bottom.get() as f32))
                .max(0.)
        } else {
            border.top.get() as f32
        };

        let tab_bar_height = self.tab_bar_pixel_height().unwrap_or(0.);

        let hovering_in_tab_bar = match &self.current_mouse_event {
            Some(event) => {
                let mouse_y = event.coords.y as f32;
                mouse_y >= tab_bar_y && mouse_y < tab_bar_y + tab_bar_height
            }
            None => false,
        };

        let new_tab_bar = TabBarState::new(
            self.dimensions.pixel_width / self.render_metrics.cell_size.width as usize,
            if hovering_in_tab_bar {
                Some(self.last_mouse_coords.0)
            } else {
                None
            },
            &tabs,
            &panes,
            self.config.resolved_palette.tab_bar.as_ref(),
            &self.config,
            &self.left_status,
            &self.right_status,
            self.render_metrics.cell_size.width as f32,
            self.os_parameters.as_ref(),
        );
        if new_tab_bar != self.tab_bar {
            self.tab_bar = new_tab_bar;
            self.invalidate_fancy_tab_bar();
            self.invalidate_modal();
            if let Some(window) = self.window.as_ref() {
                window.invalidate();
            }
        }

        let num_tabs = window.len();
        if num_tabs == 0 {
            return;
        }
        drop(window);

        let title = if let (Some(pos), Some(tab)) = (active_pane, active_tab) {
            let pane_title = crate::tabbar::automatic_title(
                pos.current_working_dir.as_deref(),
                &pos.title,
                self.config.allow_process_title_updates,
            );
            if num_tabs == 1 {
                format!("{}{}", if pos.is_zoomed { "[Z] " } else { "" }, pane_title)
            } else {
                format!(
                    "{}[{}/{}] {}",
                    if pos.is_zoomed { "[Z] " } else { "" },
                    tab.tab_index + 1,
                    num_tabs,
                    pane_title
                )
            }
        } else {
            "".to_string()
        };
        if let Some(window) = self.window.as_ref() {
            let usage = self.process_usage_suffix.borrow();
            let status = usage
                .as_ref()
                .filter(|_| self.config.show_process_tree_stats_in_title)
                .map(|usage| (usage.full.as_str(), usage.compact.as_str()));
            window.set_title_and_status(&title, status);
            drop(usage);

            let show_tab_bar = if num_tabs == 1 {
                self.config.enable_tab_bar && !self.config.hide_tab_bar_if_only_one_tab
            } else {
                self.config.enable_tab_bar
            };

            // If the number of tabs changed and caused the tab bar to
            // hide/show, then we'll need to resize things.  It is simplest
            // to piggy back on the config reloading code for that, so that
            // is what we're doing.
            if show_tab_bar != self.show_tab_bar {
                self.config_was_reloaded();
            }
        }
    }

    pub(in crate::termwindow) fn update_text_cursor(&mut self, pos: &PositionedPane) {
        if let Some(win) = self.window.as_ref() {
            let cursor = pos.pane.get_cursor_position();
            let top = pos.pane.get_dimensions().physical_top;
            let tab_bar_height = if self.show_tab_bar && !self.config.tab_bar_at_bottom {
                self.tab_bar_pixel_height().unwrap()
            } else {
                0.0
            };
            let (padding_left, padding_top) = self.padding_left_top();

            let r = Rect::new(
                Point::new(
                    (((cursor.x + pos.left) as isize).max(0) * self.render_metrics.cell_size.width)
                        .add(padding_left as isize),
                    ((cursor.y + pos.top as isize - top).max(0)
                        * self.render_metrics.cell_size.height)
                        .add(tab_bar_height as isize)
                        .add(padding_top as isize),
                ),
                self.render_metrics.cell_size,
            );
            win.set_text_cursor_position(r);
        }
    }
}
