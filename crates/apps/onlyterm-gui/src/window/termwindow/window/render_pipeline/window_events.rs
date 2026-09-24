use super::*;

impl TermWindow {
    pub(super) fn dispatch_window_event(
        &mut self,
        event: WindowEvent,
        window: &Window,
    ) -> anyhow::Result<bool> {
        log::debug!("{event:?}");
        match event {
            WindowEvent::Destroyed => {
                // Ensure that we cancel any overlays we had running, so
                // that the mux can empty out, otherwise the mux keeps
                // the TermWindow alive via the frontend even though
                // the window is gone and we'll linger forever.
                // <https://github.com/wezterm/wezterm/issues/3522>
                self.clear_all_overlays();
                // Drop render resources while the window surface is still
                // alive, before the OS invalidates the GPU drawable
                // (e.g. NSView dealloc on macOS). render_state's Drop deletes
                // the wgpu buffers/textures/glyph atlas via the device it was
                // built from, so it must go before the render thread (which
                // owns the device/surface) is torn down.
                self.render_state.take();
                // Mark the outgoing WebGpuState stale before dropping our
                // reference to it, exactly as the in-place rebuild path
                // above does (see its "Mark the outgoing device stale"
                // comment): the render thread's `RenderThreadSeed` holds its
                // own separate `Arc<WebGpuState>` and can keep it alive past
                // this point (e.g. while stuck in a hung driver call), so
                // without this, `is_current` would stay `true` and a late
                // device-lost event could try to notify this now-destroyed
                // window.
                if let Some(webgpu) = self.webgpu.take() {
                    webgpu.mark_stale();
                }
                // Detach, don't join: the whole point of the render
                // thread is that a stuck GPU driver call can't freeze the
                // GUI thread, so blocking window-close on that same thread
                // via .join() would defeat the purpose. Sending Shutdown
                // (and, failing that, dropping the handle's Sender, which
                // disconnects the channel) is enough to let the thread's
                // recv() loop end on its own, whenever the driver call it
                // may currently be stuck in eventually returns.
                if let Some(rt) = self.render_thread.take() {
                    rt.shutdown();
                }
                Ok(false)
            }
            WindowEvent::CloseRequested => {
                self.close_requested(window);
                Ok(true)
            }
            WindowEvent::AppearanceChanged(appearance) => {
                log::debug!("Appearance is now {:?}", appearance);
                // This is a bit fugly; we get per-window notifications
                // for appearance changes which successfully updates the
                // per-window config, but we need to explicitly tell the
                // global config to reload, otherwise things that acces
                // the config via onlyterm_config::configuration() will see the
                // prior version of the config.
                // What's fugly about this is that we'll reload the
                // global config here once per window, which could
                // be nasty for folks with a lot of windows.
                // <https://github.com/wezterm/wezterm/issues/2295>
                onlyterm_config::reload();
                self.config_was_reloaded();
                Ok(true)
            }
            WindowEvent::PerformKeyAssignment(action) => {
                if let Some(pane) = self.get_active_pane_or_overlay() {
                    self.perform_key_assignment(&pane, &action)?;
                    window.invalidate();
                }
                Ok(true)
            }
            WindowEvent::FocusChanged(focused) => {
                self.focus_changed(focused, window);
                Ok(true)
            }
            WindowEvent::MouseEvent(event) => {
                self.mouse_event_impl(event, window);
                Ok(true)
            }
            WindowEvent::MouseLeave => {
                self.mouse_leave_impl(window);
                Ok(true)
            }
            WindowEvent::Resized {
                dimensions,
                window_state,
                live_resizing,
            } => {
                self.resize(dimensions, window_state, window, live_resizing);
                Ok(true)
            }
            WindowEvent::SetInnerSizeCompleted => {
                self.resizes_pending -= 1;
                if self.is_repaint_pending {
                    self.is_repaint_pending = false;
                    if self.webgpu.is_some() {
                        self.do_paint_webgpu()?;
                    }
                }
                self.apply_pending_scale_changes();
                Ok(true)
            }
            WindowEvent::AdviseModifiersLedStatus(modifiers, leds) => {
                self.current_modifier_and_leds = (modifiers, leds);
                self.update_title();
                window.invalidate();
                Ok(true)
            }
            WindowEvent::RawKeyEvent(event) => {
                self.raw_key_event_impl(event, window);
                Ok(true)
            }
            WindowEvent::KeyEvent(event) => {
                self.key_event_impl(event, window);
                Ok(true)
            }
            WindowEvent::AdviseDeadKeyStatus(status) => {
                if self.config.debug_key_events {
                    log::info!("DeadKeyStatus now: {:?}", status);
                } else {
                    log::trace!("DeadKeyStatus now: {:?}", status);
                }
                self.dead_key_status = status;
                self.update_title();
                // Ensure that we repaint so that any composing
                // text is updated
                window.invalidate();
                Ok(true)
            }
            WindowEvent::NeedRepaint => {
                // Early-show (task #331) means the window can be visible --
                // and generating `NeedRepaint` events from focus/resize/OS
                // paint requests -- before the renderer is attached.
                // `self.webgpu` is still `None` from when it's constructed
                // (see the `myself` initializer above `new_window`'s
                // `Window::new_window` call) until `created()` fills it in.
                // This is intentionally *not* treated as an error:
                // `do_paint_webgpu` is simply unreachable while
                // `self.webgpu` is `None` (the `else` branch below runs
                // instead). The client area itself is covered by the
                // `WM_ERASEBKGND` placeholder brush (task #330) for the
                // brief window before `created()` runs, so a dropped
                // repaint here is harmless -- `created()` also forces one
                // more `window.invalidate()` once the renderer is actually in
                // place, so nothing is lost, only delayed.
                if self.resizes_pending > 0 {
                    self.is_repaint_pending = true;
                    Ok(true)
                } else if self.webgpu.is_some() {
                    self.do_paint_webgpu()
                } else {
                    Ok(false)
                }
            }
            WindowEvent::Notification(item) => {
                if let Ok(notif) = item.downcast::<TermWindowNotif>() {
                    self.dispatch_notif(*notif, window)
                        .context("dispatch_notif")?;
                }
                Ok(true)
            }
            WindowEvent::DroppedString(text) => {
                let pane = match self.get_active_pane_or_overlay() {
                    Some(pane) => pane,
                    None => return Ok(true),
                };
                pane.send_paste(text.as_str())?;
                Ok(true)
            }
            WindowEvent::DroppedUrl(urls) => {
                let pane = match self.get_active_pane_or_overlay() {
                    Some(pane) => pane,
                    None => return Ok(true),
                };
                let urls = urls
                    .iter()
                    .map(|url| self.config.quote_dropped_files.escape(url.as_ref()))
                    .collect::<Vec<_>>()
                    .join(" ")
                    + " ";
                pane.send_paste(urls.as_str())?;
                Ok(true)
            }
            WindowEvent::DroppedFile(paths) => {
                let pane = match self.get_active_pane_or_overlay() {
                    Some(pane) => pane,
                    None => return Ok(true),
                };
                let paths = paths
                    .iter()
                    .map(|path| {
                        self.config
                            .quote_dropped_files
                            .escape(&path.to_string_lossy())
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
                    + " ";
                pane.send_paste(&paths)?;
                Ok(true)
            }
            WindowEvent::DraggedFile(_) => Ok(true),
        }
    }
}
