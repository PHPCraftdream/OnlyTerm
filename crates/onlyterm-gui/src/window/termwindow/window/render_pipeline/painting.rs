use super::*;

impl TermWindow {
    pub(super) fn do_paint_webgpu(&mut self) -> anyhow::Result<bool> {
        let dims = self.dimensions;
        self.resize_webgpu_surface(dims);
        match self.do_paint_webgpu_impl() {
            Ok(ok) => Ok(ok),
            Err(err) => {
                // Note: with a render thread active, `do_paint_webgpu_impl`
                // (via `paint_impl` -> `call_draw` -> `call_draw_webgpu`,
                // see 221.5) never actually returns a `SurfaceError` --
                // frames are handed off to `send_frame` and this always
                // returns `Ok(())`. So this retry branch is effectively
                // dead code in render-thread mode; it remains the
                // correct/only recovery path when the render thread is
                // inactive (flag off, non-Windows, or spawn failed), so
                // it's left in place rather than removed.
                if let Some(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) =
                    err.downcast_ref::<wgpu::SurfaceError>()
                {
                    let dims = self.dimensions;
                    self.resize_webgpu_surface(dims);
                    return self.do_paint_webgpu_impl();
                }
                Err(err)
            }
        }
    }

    fn do_paint_webgpu_impl(&mut self) -> anyhow::Result<bool> {
        self.paint_impl(&mut RenderFrame::WebGpu);
        Ok(true)
    }

    pub(super) fn dispatch_notif(
        &mut self,
        notif: TermWindowNotif,
        window: &Window,
    ) -> anyhow::Result<()> {
        match notif {
            TermWindowNotif::InvalidateShapeCache(chars) => {
                self.bump_fallback_epoch();
                if chars.is_empty() {
                    self.shape_generation += 1;
                    self.shape_cache.borrow_mut().clear();
                    self.line_to_ele_shape_cache.borrow_mut().clear();
                    self.shape_hash_cache.borrow_mut().clear();
                } else {
                    let mut generations = self.fallback_generations.borrow_mut();
                    for ch in chars {
                        let generation = generations.entry(ch).or_insert(0);
                        *generation = generation.wrapping_add(1);
                    }
                }
                self.invalidate_modal();
                self.invalidate_fancy_tab_bar();
                window.invalidate();
            }
            TermWindowNotif::PerformAssignment {
                pane_id,
                assignment,
                tx,
            } => {
                let mux = Mux::get();
                let result = || -> anyhow::Result<()> {
                    // The CopyMode overlay doesn't exist in the mux, but aliases
                    // itself with the overlaid pane's pane_id.
                    // So we do a bit of fancy footwork here to resolve the overlay
                    // and use that if it has the same pane_id, but otherwise fall
                    // back to what we get from the mux.
                    // <https://github.com/wezterm/wezterm/issues/3209>
                    let active_pane = self
                        .get_active_pane_or_overlay()
                        .ok_or_else(|| anyhow!("there is no active pane!?"))?;
                    let pane = if active_pane.pane_id() == pane_id {
                        active_pane
                    } else {
                        mux.get_pane(pane_id)
                            .ok_or_else(|| anyhow!("pane id {} is not valid", pane_id))?
                    };
                    self.perform_key_assignment(&pane, &assignment)
                        .context("perform_key_assignment")?;
                    Ok(())
                }();
                window.invalidate();
                if let Some(tx) = tx {
                    tx.try_send(result).ok();
                }
            }
            TermWindowNotif::CancelOverlayForPane(pane_id) => {
                self.cancel_overlay_for_pane(pane_id);
            }
            TermWindowNotif::CancelOverlayForTab { tab_id, pane_id } => {
                self.cancel_overlay_for_tab(tab_id, pane_id);
            }
            TermWindowNotif::MuxNotification(n) => match n {
                MuxNotification::Alert {
                    alert: Alert::SetUserVar { name, value },
                    pane_id,
                } => {
                    self.emit_user_var_event(pane_id, name, value);
                }
                MuxNotification::WindowTitleChanged { .. }
                | MuxNotification::Alert {
                    alert:
                        Alert::OutputSinceFocusLost
                        | Alert::CurrentWorkingDirectoryChanged
                        | Alert::WindowTitleChanged(_)
                        | Alert::TabTitleChanged(_)
                        | Alert::IconTitleChanged(_)
                        | Alert::Progress(_),
                    ..
                } => {
                    // The hot one: a full-screen TUI re-sets its title,
                    // cwd and progress on every repaint, and each of those
                    // used to force a full rebuild through the contended
                    // terminal lock. Rate-limited; see
                    // TITLE_UPDATE_MIN_INTERVAL.
                    self.update_title_coalesced();
                }
                MuxNotification::Alert {
                    alert: Alert::PaletteChanged,
                    pane_id,
                } => {
                    // Shape cache includes color information, so
                    // ensure that we invalidate that as part of
                    // this overall invalidation for the palette
                    self.dispatch_notif(TermWindowNotif::InvalidateShapeCache(Vec::new()), window)?;
                    self.mux_pane_output_event(pane_id);
                }
                MuxNotification::Alert {
                    alert: Alert::Bell,
                    pane_id,
                } => {
                    if !self.window_contains_pane(pane_id) {
                        return Ok(());
                    }

                    match self.config.audible_bell {
                        AudibleBell::SystemBeep => {
                            Connection::get().expect("on main thread").beep();
                        }
                        AudibleBell::Disabled => {}
                    }

                    log::trace!("Ding! (this is the bell) in pane {}", pane_id);

                    let mut per_pane = self.pane_state(pane_id);
                    per_pane.bell_start.replace(Instant::now());
                    window.invalidate();
                }
                MuxNotification::Alert {
                    alert: Alert::ToastNotification { .. },
                    ..
                } => {}
                MuxNotification::TabAddedToWindow {
                    window_id: _,
                    tab_id,
                } => {
                    let mux = Mux::get();
                    let mut size = self.terminal_size;
                    if let Some(tab) = mux.get_tab(tab_id) {
                        // If we attached to a remote domain and loaded in
                        // a tab async, we need to fixup its size, either
                        // by resizing it or resizes ourselves.
                        // The strategy here is to adjust both by taking
                        // the maximal size in both horizontal and vertical
                        // dimensions and applying that. In practice that
                        // means that a new local client will resize larger
                        // to adjust to the size of an existing client.
                        let tab_size = tab.get_size();
                        size.rows = size.rows.max(tab_size.rows);
                        size.cols = size.cols.max(tab_size.cols);

                        if size.rows != self.terminal_size.rows
                            || size.cols != self.terminal_size.cols
                            || size.pixel_width != self.terminal_size.pixel_width
                            || size.pixel_height != self.terminal_size.pixel_height
                        {
                            self.set_window_size(size, window)?;
                        } else if tab_size.dpi == 0 {
                            log::debug!("fixup dpi in newly added tab");
                            tab.resize(self.terminal_size);
                        }
                    }
                }
                MuxNotification::PaneOutput(pane_id) => {
                    self.mux_pane_output_event(pane_id);
                }
                MuxNotification::WindowInvalidated(_) => {
                    window.invalidate();
                    self.update_title_post_status_coalesced();
                }
                MuxNotification::WindowRemoved(_window_id) => {
                    // Handled by frontend
                }
                MuxNotification::AssignClipboard { .. } => {
                    // Handled by frontend
                }
                MuxNotification::SaveToDownloads { .. } => {
                    // Handled by frontend
                }
                MuxNotification::PaneFocused(_) => {
                    // Also handled by clientpane
                    self.update_title_post_status_coalesced();
                }
                MuxNotification::TabReflowed(_) => {
                    // Also handled by onlyterm-client
                    self.update_title_post_status_coalesced();
                }
                MuxNotification::TabTitleChanged { .. } => {
                    self.update_title_post_status_coalesced();
                }
                MuxNotification::PaneRemoved(pane_id) => {
                    // The pane is gone from the mux. This notification is broadcast to
                    // every TermWindow in the process, so it also arrives for panes of
                    // other OS windows; dropping an id this window never rendered is a
                    // no-op. Nothing will ever paint this pane again -- drop everything
                    // we cached for it (see forget_pane_caches).
                    let mut pane_state = self.pane_state.borrow_mut();
                    let mut retained_rows = self.retained_rows.borrow_mut();
                    forget_pane_caches(
                        &mut pane_state,
                        &mut self.semantic_zones,
                        &mut retained_rows,
                        pane_id,
                    );
                }
                MuxNotification::PaneAdded(_)
                | MuxNotification::WorkspaceRenamed { .. }
                | MuxNotification::WindowWorkspaceChanged(_)
                | MuxNotification::ActiveWorkspaceChanged(_)
                | MuxNotification::Empty
                | MuxNotification::WindowCreated(_) => {}
            },
            TermWindowNotif::Apply(func) => {
                func(self);
            }
            TermWindowNotif::SwitchToMuxWindow(mux_window_id) => {
                self.mux_window_id = mux_window_id;
                *self.mux_window_id_for_subscriptions.lock().unwrap() = mux_window_id;

                self.clear_all_overlays();
                self.current_highlight.take();
                self.invalidate_fancy_tab_bar();
                self.invalidate_modal();

                let mux = Mux::get();
                if let Some(window) = mux.get_window(self.mux_window_id) {
                    for tab in window.iter() {
                        tab.resize(self.terminal_size);
                    }
                };
                self.update_title();
                window.invalidate();
            }
        }

        Ok(())
    }
}
