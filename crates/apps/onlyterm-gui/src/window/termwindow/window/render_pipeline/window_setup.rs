use super::*;

impl TermWindow {
    /// Builds and shows the OS window plus renderer for `mux_window_id`.
    ///
    /// Every window -- the process's very first one and any opened later at
    /// runtime (eg. via `KeyAssignment::SpawnWindow`) -- funnels through this
    /// same function; there is no separate "startup" code path. A
    /// fatal-looking failure partway through construction (currently: WebGpu
    /// adapter/device init) decides whether it is safe to tear down the
    /// whole process, or whether it must instead fail only this one window
    /// and leave any other already-running windows untouched, by checking
    /// `front_end().has_any_known_window()` *at the point of failure* --
    /// see the WebGpu init failure handling below (task #428).
    pub async fn new_window(mux_window_id: MuxWindowId) -> anyhow::Result<()> {
        let config = configuration();
        let dpi = config.dpi.unwrap_or_else(::window::default_dpi) as usize;
        // Startup-latency diagnostics: see the "startup:" checkpoints in
        // main.rs. This is the window's own (second) `FontConfiguration`;
        // `main.rs::cell_pixel_dims` builds a throwaway first one just to
        // size the initial window -- these two checkpoint pairs are what
        // showed that repeating font enumeration here is essentially free
        // (font lookups are cached), not a second real cost.
        log::info!("startup: new_window font enumeration starting");
        let fontconfig = Rc::new(FontConfiguration::new(Some(config.clone()), dpi)?);
        log::info!("startup: new_window font enumeration done");

        let mux = Mux::get();
        let size = match mux.get_active_tab_for_window(mux_window_id) {
            Some(tab) => tab.get_size(),
            None => {
                log::debug!("new_window has no tabs... yet?");
                Default::default()
            }
        };
        let physical_rows = size.rows as usize;
        let physical_cols = size.cols as usize;

        let render_metrics = RenderMetrics::new(&fontconfig)?;
        log::trace!("using render_metrics {:#?}", render_metrics);

        // Initially we have only a single tab, so take that into account
        // for the tab bar state.
        let show_tab_bar = config.enable_tab_bar && !config.hide_tab_bar_if_only_one_tab;
        let tab_bar_height = if show_tab_bar {
            Self::tab_bar_pixel_height_impl(&config, &fontconfig, &render_metrics)? as usize
        } else {
            0
        };

        let terminal_size = TerminalSize {
            rows: physical_rows,
            cols: physical_cols,
            pixel_width: (render_metrics.cell_size.width as usize * physical_cols),
            pixel_height: (render_metrics.cell_size.height as usize * physical_rows),
            dpi: dpi as u32,
        };

        if terminal_size != size {
            // DPI is different from the default assumed DPI when the mux
            // created the pty. We need to inform the kernel of the revised
            // pixel geometry now
            log::trace!(
                "Initial geometry was {:?} but dpi-adjusted geometry \
                        is {:?}; update the kernel pixel geometry for the ptys!",
                size,
                terminal_size,
            );
            if let Some(window) = mux.get_window(mux_window_id) {
                for tab in window.iter() {
                    tab.resize(terminal_size);
                }
            };
        }

        let h_context = DimensionContext {
            dpi: dpi as f32,
            pixel_max: terminal_size.pixel_width as f32,
            pixel_cell: render_metrics.cell_size.width as f32,
        };
        let padding_left = config.window_padding.left.evaluate_as_pixels(h_context) as usize;
        let padding_right = resize::effective_right_padding(&config, h_context) as usize;
        let v_context = DimensionContext {
            dpi: dpi as f32,
            pixel_max: terminal_size.pixel_height as f32,
            pixel_cell: render_metrics.cell_size.height as f32,
        };
        let padding_top = config.window_padding.top.evaluate_as_pixels(v_context) as usize;
        let padding_bottom = config.window_padding.bottom.evaluate_as_pixels(v_context) as usize;

        let mut dimensions = Dimensions {
            pixel_width: (terminal_size.pixel_width + padding_left + padding_right) as usize,
            pixel_height: ((terminal_size.rows * render_metrics.cell_size.height as usize)
                + padding_top
                + padding_bottom) as usize
                + tab_bar_height,
            dpi,
        };

        let border = Self::get_os_border_impl(&None, &config, &dimensions, &render_metrics);

        dimensions.pixel_height += (border.top + border.bottom).get() as usize;
        dimensions.pixel_width += (border.left + border.right).get() as usize;

        let window_background = load_background_image(&config, &dimensions, &render_metrics);

        log::trace!(
            "TermWindow::new_window called with mux_window_id {} {:?} {:?}",
            mux_window_id,
            terminal_size,
            dimensions
        );

        let render_state = None;

        let connection_name = Connection::get().unwrap().name();

        let myself = Self {
            created: Instant::now(),
            shell_output_seen: false,
            placeholder_cleared: false,
            connection_name,
            last_fps_check_time: Instant::now(),
            num_frames: 0,
            last_frame_duration: Duration::ZERO,
            fps: 0.,
            config_subscription: None,
            os_parameters: None,
            webgpu: None,
            render_thread: None,
            render_thread_hang_handled: Cell::new(false),
            hang_check_scheduled: Cell::new(false),
            rebuild_attempts: RefCell::new(Vec::new()),
            process_usage_scheduled: Cell::new(false),
            last_process_usage_sample: RefCell::new(None),
            process_usage_suffix: RefCell::new(None),
            window: None,
            window_background,
            config: config.clone(),
            config_overrides: onlyterm_dynamic::Value::default(),
            palette: None,
            focused: None,
            mux_window_id,
            mux_window_id_for_subscriptions: Arc::new(Mutex::new(mux_window_id)),
            mux_subscription_dead: Arc::new(AtomicBool::new(false)),
            fonts: Rc::clone(&fontconfig),
            render_metrics,
            dimensions,
            window_state: WindowState::default(),
            resizes_pending: 0,
            is_repaint_pending: false,
            pending_scale_changes: LinkedList::new(),
            terminal_size,
            render_state,
            input_map: InputMap::new(&config),
            leader_is_down: None,
            pass_through: crate::termwindow::keyevent::pass_through::PassThrough::new(
                config.pass_through_next_key_on_double_ctrl,
            ),
            pending_pass_through: None,
            dead_key_status: DeadKeyStatus::None,
            show_tab_bar,
            show_scroll_bar: config.enable_scroll_bar,
            tab_bar: TabBarState::default(),
            fancy_tab_bar: None,
            right_status: String::new(),
            left_status: String::new(),
            last_mouse_coords: (0, -1),
            suppress_move_after_focus_click: None,
            window_drag_position: None,
            current_mouse_event: None,
            current_modifier_and_leds: Default::default(),
            prev_cursor: PrevCursorPos::new(),
            last_scroll_info: RenderableDimensions::default(),
            tab_state: RefCell::new(HashMap::new()),
            pane_state: RefCell::new(HashMap::new()),
            current_mouse_buttons: vec![],
            current_mouse_capture: None,
            last_mouse_click: None,
            current_highlight: None,
            quad_generation: 0,
            shape_generation: 0,
            fallback_generations: RefCell::new(HashMap::new()),
            fallback_epoch: std::cell::Cell::new(0),
            shape_cache: RefCell::new(LfuCache::new(
                "shape_cache.hit.rate",
                "shape_cache.miss.rate",
                |config| config.shape_cache_size,
                &config,
            )),
            // Task #439: Shape hash cache keyed by (pane_id, stable_row)
            shape_hash_cache: RefCell::new(LfuCache::new(
                "shape_hash_cache.hit.rate",
                "shape_hash_cache.miss.rate",
                |config| config.line_state_cache_size,
                &config,
            )),
            line_quad_cache: RefCell::new(LfuCache::new(
                "line_quad_cache.hit.rate",
                "line_quad_cache.miss.rate",
                |config| config.line_quad_cache_size,
                &config,
            )),
            retained_rows: RefCell::new(std::collections::HashMap::new()),
            line_to_ele_shape_cache: RefCell::new(LfuCache::new(
                "line_to_ele_shape_cache.hit.rate",
                "line_to_ele_shape_cache.miss.rate",
                |config| config.line_to_ele_shape_cache_size,
                &config,
            )),
            title_update_coalescer: Default::default(),
            cursor_blink_state: RefCell::new(ColorEase::new(
                config.cursor_blink_rate,
                config.cursor_blink_ease_in,
                config.cursor_blink_rate,
                config.cursor_blink_ease_out,
                None,
            )),
            blink_state: RefCell::new(ColorEase::new(
                config.text_blink_rate,
                config.text_blink_ease_in,
                config.text_blink_rate,
                config.text_blink_ease_out,
                None,
            )),
            rapid_blink_state: RefCell::new(ColorEase::new(
                config.text_blink_rate_rapid,
                config.text_blink_rapid_ease_in,
                config.text_blink_rate_rapid,
                config.text_blink_rapid_ease_out,
                None,
            )),
            event_states: HashMap::new(),
            current_event: None,
            has_animation: RefCell::new(None),
            scheduled_animation: RefCell::new(None),
            scheduled_budget_repaint: RefCell::new(None),
            allow_images: AllowImage::Yes,
            semantic_zones: HashMap::new(),
            ui_items_scratch: vec![],
            ui_items: arc_swap::ArcSwap::new(std::sync::Arc::new(Vec::new())),
            dragging: None,
            last_ui_item: None,
            is_click_to_focus_window: false,
            key_table_state: KeyTableState::default(),
            modal: RefCell::new(None),
            renderer_info: None,
            last_frame_signature: None,
            last_wire_atlas_generation: std::cell::Cell::new(None),
            atlas_generation: 0,
        };

        let tw = Rc::new(RefCell::new(myself));
        let tw_event = Rc::clone(&tw);

        let mut x = None;
        let mut y = None;
        let mut origin = GeometryOrigin::default();

        if let Some(position) = mux
            .get_window(mux_window_id)
            .and_then(|window| window.get_initial_position().clone())
            .or_else(|| POSITION.lock().unwrap().take())
        {
            x.replace(position.x);
            y.replace(position.y);
            origin = position.origin;
        }

        let geometry = RequestedWindowGeometry {
            width: Dimension::Pixels(dimensions.pixel_width as f32),
            height: Dimension::Pixels(dimensions.pixel_height as f32),
            x,
            y,
            origin,
        };
        log::trace!("{:?}", geometry);

        let window = Window::new_window(
            &get_window_class(),
            "OnlyTerm",
            geometry,
            Some(&config),
            Rc::clone(&fontconfig),
            move |event, window| {
                let mut tw = tw_event.borrow_mut();
                if let Err(err) = tw.dispatch_window_event(event, window) {
                    log::error!("dispatch_window_event: {:#}", err);
                }
            },
        )
        .await?;
        tw.borrow_mut().window.replace(window.clone());

        Self::apply_icon(&window)?;

        // Show the window now, before WebGpu adapter/device/pipeline
        // initialization, instead of waiting for `RenderState` to be ready
        // further down. On Windows with the WebGpu/DX12 default this
        // initialization alone can take several seconds; the user should not
        // stare at nothing that whole time. The client area is safe to show
        // unpainted because the Windows window class fills it with the
        // terminal's background color via `WM_ERASEBKGND` until the first
        // real frame actually lands and that placeholder is cleared (task
        // #330; moved out of `created()` below by task #425, then further
        // hardened by task #407 to wait for an actual `present()` rather
        // than just a frame being handed off/enqueued -- see
        // `WindowOps::clear_placeholder_background`'s doc comment for why
        // clearing it as soon as `created()` merely installs a
        // `RenderState` left a gap where nothing painted the window).
        // `NeedRepaint` before the
        // renderer exists is already a no-op (`do_paint_webgpu` is
        // unreachable until `self.webgpu` is set in `created()`), and pane
        // input keeps flowing to the pty regardless of whether a renderer is
        // attached yet, so there is nothing unsafe about a
        // visible-but-not-yet-rendering window.
        window.show();
        if config.start_maximized {
            window.maximize();
        }

        let config_subscription = onlyterm_config::subscribe_to_config_reload({
            let window = window.clone();
            move || {
                window.notify(TermWindowNotif::Apply(Box::new(|tw| {
                    tw.config_was_reloaded()
                })));
                true
            }
        });

        // WebGpu is the only renderer OnlyTerm has left (the OpenGL/Mesa
        // fallback was removed in task #414). `FrontEndSelection` has only
        // one variant (see onlyterm_config::frontend), so this is always taken; the
        // `match` isn't a real branch point any more, just documentation of
        // that invariant, kept so a future new `FrontEndSelection` variant
        // doesn't silently fall through here unhandled.
        debug_assert_eq!(config.front_end, FrontEndSelection::WebGpu);

        // Every window's GPU rendering is hosted in its own `--gpu-tab-host`
        // child process (`HostProcessBackend`, task #651) -- unconditional,
        // no config lever, per the decision that a proven, self-healing
        // respawn (killing the child never takes the parent down, and a
        // dead child's last frame keeps showing on screen while a
        // replacement spawns) makes an opt-in toggle pointless. Falling back
        // to the in-process `RenderThreadHandle` only happens if
        // `HostProcessBackend::spawn` itself fails outright (no
        // DirectComposition, spawn failure) -- exactly like falling back to
        // it used to be the *only* option.
        let host_process_backend =
            onlyterm_gpu_render::HostProcessBackend::spawn(&window, dimensions);
        if host_process_backend.is_none() {
            log::warn!(
                "HostProcessBackend::spawn failed; falling back to in-process GPU rendering \
                 for this window (see preceding log lines for the specific failure)"
            );
        }

        let webgpu = if host_process_backend.is_some() {
            WebGpuState::new_device_only(&config, gpu_recovery_notifier(&window)).await
        } else {
            WebGpuState::new(&window, dimensions, &config, gpu_recovery_notifier(&window)).await
        };
        let webgpu = match webgpu {
            Ok(state) => Arc::new(state),
            Err(err) => {
                // WebGpu adapter/device creation can fail in RDP sessions, on
                // old/software-only GPUs, in VMs without GPU passthrough, or
                // due to a driver mismatch (eg. opening a new window on a
                // second monitor driven by a different, weaker GPU adapter,
                // or a transient driver hiccup). There is no other renderer
                // left to fall back to (task #414 removed the OpenGL/Mesa
                // fallback that used to catch this).
                let message = format!(
                    "Failed to initialize the WebGpu renderer: {err:#}\n\n\
                     This can happen in a VM without GPU passthrough, over \
                     RDP, or due to a graphics driver mismatch. OnlyTerm has \
                     no other rendering backend to fall back to, so it \
                     cannot open a window without a working WebGpu \
                     adapter/device."
                );
                // Whether it is safe to tear down the whole process over
                // this failure, versus just this one window, is decided
                // right here, right now, rather than from a flag captured
                // before this `await`ed WebGpu init even started: WebGpu
                // adapter/device creation can take up to several seconds on
                // Windows/DX12 (see the comment on `window.show()` above),
                // and `reconcile_workspace` can have several independent
                // spawn loops racing each other (eg. session restore with
                // multiple saved windows, possibly racing a concurrent
                // `SpawnWindow`). A flag snapshotted ahead of time could go
                // stale mid-`await` and let two windows both believe they
                // are the process's only one. `known_windows` only gains an
                // entry once `record_known_window` runs, at the very end of
                // a *successful* `new_window`, well after this point -- so
                // if it is empty right now, this window is genuinely the
                // only live one, regardless of how many other
                // `new_window` calls are concurrently in flight but haven't
                // finished (or have also failed) yet.
                if !front_end().has_any_known_window() {
                    // This is the only window the process has; the process
                    // cannot usefully continue running without it, so fail
                    // clean the same way any other fatal startup error does:
                    // log the real cause, show the user a toast notification
                    // explaining what went wrong, and exit. See
                    // `crate::terminate_with_error_message` (used
                    // identically by `crate::terminate_with_error` for other
                    // fatal startup failures in `main.rs`) -- reusing that
                    // path here rather than inventing a second one means the
                    // user sees the exact same failure UX regardless of
                    // which fatal startup error they hit.
                    crate::terminate_with_error_message(&message);
                } else {
                    // At least one other window is already up and running
                    // with its own panes/shells/child processes attached.
                    // Killing the whole process here (as task #414
                    // originally did, unconditionally) would take all of
                    // that down over a failure scoped to just this one new
                    // window. Instead, notify the user with the same
                    // message a fatal startup failure would have shown, and
                    // return `Err` so the caller
                    // (`GuiFrontEnd::reconcile_workspace`'s spawn loop, in
                    // `frontend.rs`) can clean up only this mux window --
                    // logging, `mux.kill_window`, and unregistering it --
                    // exactly like it already does for any other
                    // `new_window` failure, without touching the rest of the
                    // process.
                    //
                    // `window.show()` above already made the OS window
                    // (with its "Loading..." placeholder) visible before
                    // WebGpu init even started, and this window was never
                    // registered in `known_windows` (that only happens on
                    // the success path, in `record_known_window` below) --
                    // so nothing else will ever `close()` it for us. Windows
                    // does not tear down the HWND on its own just because
                    // this `TermWindow`/`Window` value is about to be
                    // dropped (there is no `Drop` impl wired up to
                    // `DestroyWindow` anywhere in `crates/graphics/window/src`), so
                    // without an explicit `close()` here the OS window would
                    // leak on screen forever as an empty, permanently
                    // "Loading..." window with no `TermWindow` state behind
                    // it able to ever finish it.
                    window.close();
                    onlyterm_toast_notification::persistent_toast_notification(
                        "OnlyTerm: failed to open window",
                        &message,
                    );
                    return Err(anyhow!(message));
                }
            }
        };

        {
            let mut myself = tw.borrow_mut();
            myself.config_subscription.replace(config_subscription);
            if config.use_resize_increments {
                window.set_resize_increments(
                    ResizeIncrementCalculator {
                        x: myself.render_metrics.cell_size.width as u16,
                        y: myself.render_metrics.cell_size.height as u16,
                        padding_left,
                        padding_top,
                        padding_right,
                        padding_bottom,
                        border,
                        tab_bar_height,
                    }
                    .into(),
                );
            }

            myself.webgpu.replace(Arc::clone(&webgpu));

            // The render backend has to be installed *before* `created()`,
            // which is what builds this window's `RenderState` and with it
            // its glyph atlas: `RenderState::new` asks
            // `wants_gpu_atlas_mirroring()` (i.e. this backend) whether
            // atlas writes must be recorded, and has to switch recording on
            // before `UtilSprites::new` writes the first sprites into that
            // atlas. Installing the backend afterwards left the answer stuck
            // at "no" for the whole life of the window, so a
            // `HostProcessBackend` child was fed a permanently blank mirror
            // of the atlas and drew every glyph with alpha 0 -- a window
            // with correct background colors and no text whatsoever.
            let mut installed_render_thread = false;
            if let Some(backend) = host_process_backend {
                myself.render_thread =
                    Some(Box::new(backend) as Box<dyn onlyterm_gpu_render::RenderBackend>);
                installed_render_thread = true;
            } else if config.webgpu_render_thread {
                let (tx, rx) = std::sync::mpsc::channel();
                let in_flight = Arc::new(std::sync::atomic::AtomicBool::new(false));
                let repaint_pending = Arc::new(std::sync::atomic::AtomicBool::new(false));
                let window_destroyed = Arc::new(std::sync::atomic::AtomicBool::new(false));
                let submit_started_at = Arc::new(parking_lot::Mutex::new(None));
                let seed = crate::renderthread::RenderThreadSeed {
                    window: window.clone(),
                    webgpu: Arc::clone(&webgpu),
                    rx,
                    in_flight,
                    repaint_pending,
                    window_destroyed,
                    submit_started_at,
                    on_renderer_error: Box::new(|win, reason| {
                        let recovery_window = win.clone();
                        win.notify(crate::termwindow::TermWindowNotif::Apply(Box::new(
                            move |tw| {
                                tw.handle_render_error_recovery(&recovery_window, &reason);
                            },
                        )));
                    }),
                };
                myself.render_thread =
                    crate::renderthread::RenderThreadHandle::spawn(seed, tx, myself.mux_window_id)
                        .map(|handle| {
                            Box::new(handle) as Box<dyn onlyterm_gpu_render::RenderBackend>
                        });
                installed_render_thread = myself.render_thread.is_some();
            }

            myself.created(RenderContext(Arc::clone(&webgpu)))?;

            if installed_render_thread {
                myself.schedule_render_thread_hang_check(&window);
            }
            myself.schedule_process_usage_tick(&window);
            myself.load_os_parameters();
            myself.subscribe_to_pane_updates();

            // If the startup chooser is armed, install the New Tab Options
            // modal now that the window is fully constructed. This happens
            // at most once: the first window to finish construction takes the
            // chooser, every later window gets None.
            if let Some(pending) = crate::startup_chooser::take() {
                let modal = crate::termwindow::newtab_options::NewTabOptions::new_with_on_cancel(
                    crate::termwindow::newtab_options::OnCancel::QuitApplication,
                    pending.activity,
                    pending.cwd,
                );
                myself.set_modal(std::rc::Rc::new(modal));
            }
        }

        crate::update::start_update_checker();
        front_end().record_known_window(window, mux_window_id);

        Ok(())
    }
}
