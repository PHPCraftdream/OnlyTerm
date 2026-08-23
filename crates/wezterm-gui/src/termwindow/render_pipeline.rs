use super::prevcursor::PrevCursorPos;
use super::*;
use crate::colorease::ColorEase;
use crate::frontend::front_end;
use crate::inputmap::InputMap;
use crate::overlay::{CopyOverlay, QuickSelectOverlay};
use crate::resize_increment_calculator::ResizeIncrementCalculator;
use crate::tabbar::TabBarState;
use crate::termwindow::background::load_background_image;
use crate::termwindow::keyevent::KeyTableState;
use crate::termwindow::render::paint::AllowImage;
use crate::utilsprites::RenderMetrics;
use anyhow::{anyhow, Context};
use config::{
    configuration, AudibleBell, Dimension, DimensionContext, FrontEndSelection, GeometryOrigin,
};
use mux::pane::{Pane, PaneId};
use mux::renderable::RenderableDimensions;
use mux::window::WindowId as MuxWindowId;
use mux::{Mux, MuxNotification};
use smol::Timer;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, LinkedList};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use wezterm_font::FontConfiguration;
use wezterm_gpu_render::{rebuild_backoff_for_attempt, WebGpuState};
use wezterm_term::{Alert, StableRowIndex, TerminalSize};

/// Builds the recovery callback handed to the GPU-render crate when a window
/// registers for device-lost/error notifications: routes the reason string
/// into the GUI-thread `TermWindowNotif::Apply` that runs
/// `handle_render_error_recovery` -- exactly what the webgpu context's
/// subscriber machinery used to do inline, before that code moved out of
/// this crate.
fn gpu_recovery_notifier(window: &Window) -> wezterm_gpu_render::GpuRecoveryNotifier {
    let win = window.clone();
    Box::new(move |reason: &str| {
        let reason = reason.to_string();
        let win2 = win.clone();
        win.notify(TermWindowNotif::Apply(Box::new(move |tw| {
            tw.handle_render_error_recovery(&win2, &reason);
        })));
    })
}

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
            window: None,
            window_background,
            config: config.clone(),
            config_overrides: wezterm_dynamic::Value::default(),
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
            last_status_call: Instant::now(),
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

        let config_subscription = config::subscribe_to_config_reload({
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
        // one variant (see config::frontend), so this is always taken; the
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
            wezterm_gpu_render::HostProcessBackend::spawn(&window, dimensions);
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
                    // `DestroyWindow` anywhere in `crates/window/src`), so
                    // without an explicit `close()` here the OS window would
                    // leak on screen forever as an empty, permanently
                    // "Loading..." window with no `TermWindow` state behind
                    // it able to ever finish it.
                    window.close();
                    wezterm_toast_notification::persistent_toast_notification(
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
                    Some(Box::new(backend) as Box<dyn wezterm_gpu_render::RenderBackend>);
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
                            Box::new(handle) as Box<dyn wezterm_gpu_render::RenderBackend>
                        });
                installed_render_thread = myself.render_thread.is_some();
            }

            myself.created(RenderContext(Arc::clone(&webgpu)))?;

            if installed_render_thread {
                myself.schedule_render_thread_hang_check(&window);
            }
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

    /// Schedules the next tick of this window's render-thread hang
    /// supervisor. Self-rearming: each tick either closes the window (if its
    /// render thread is hung) or calls this again to schedule the next tick,
    /// exactly like `scheduled_animation`'s `Timer::at` + `notify` pattern in
    /// `paint_impl` reschedules itself.
    ///
    /// Only ever called (initially from `new_window`, then from
    /// `finish_renderer_rebuild` after a successful rebuild, and recursively
    /// from `check_render_thread_hang_tick`) while running on the GUI thread
    /// -- `promise::spawn::spawn` is GUI-thread-only (it uses `spawn_local`
    /// under the hood), which holds here since all call sites already run
    /// on the GUI thread.
    ///
    /// Guarded by `hang_check_scheduled` (task #287): if a chain is already
    /// pending for this window, this is a no-op rather than arming a second,
    /// concurrent chain. See that field's doc comment for the race this
    /// closes. The guard is set here, at the point a new timer is actually
    /// armed, and cleared at the very top of `check_render_thread_hang_tick`
    /// -- i.e. it tracks "is a tick currently in flight for this window",
    /// not "has a chain ever been started".
    fn schedule_render_thread_hang_check(&self, window: &Window) {
        if self.hang_check_scheduled.get() {
            // A chain is already pending (its timer tick hasn't fired yet);
            // do not start a second, concurrent chain. See this call's
            // doc comment and `hang_check_scheduled`'s own doc comment.
            return;
        }
        self.hang_check_scheduled.set(true);

        // Poll at a fraction of the hang threshold, the same style as
        // `window::os::windows::watchdog`'s `poll_interval = (threshold /
        // 4).max(Duration::from_millis(50))`. This check is cheaper than the
        // GUI watchdog's (just a `Mutex<Option<Instant>>` read, no syscalls),
        // so a smaller minimum is fine, but we still don't want a
        // misconfigured (very low) threshold to turn into a busy-poll.
        let threshold =
            Duration::from_millis(config::configuration().render_thread_hang_threshold_ms);
        let poll_interval = (threshold / 2).max(Duration::from_millis(500));
        let next_check = Instant::now() + poll_interval;

        let window = window.clone();
        promise::spawn::spawn(async move {
            Timer::at(next_check).await;
            let win = window.clone();
            window.notify(TermWindowNotif::Apply(Box::new(move |tw| {
                tw.check_render_thread_hang_tick(&win);
            })));
        })
        .detach();
    }

    /// Circuit breaker thresholds for the in-place renderer rebuild
    /// performed by `check_render_thread_hang_tick`. If rebuilding the
    /// renderer doesn't actually fix things -- the GPU/driver/adapter is
    /// fundamentally broken rather than having suffered a one-off transient
    /// stall -- the render thread will simply hang again almost
    /// immediately after each rebuild. `3` rebuilds within `30` seconds is
    /// enough slack for a couple of unlucky-but-unrelated stalls (e.g. two
    /// independent brief driver hiccups minutes apart would never trip
    /// this), while still catching an immediate re-hang loop quickly: three
    /// full rebuild-and-rehang cycles within half a minute is well outside
    /// what a real transient stall looks like.
    const MAX_REBUILDS_PER_WINDOW: usize = 3;
    const REBUILD_WINDOW: Duration = Duration::from_secs(30);

    /// One tick of the render-thread hang supervisor: if this window's
    /// render thread appears hung, rebuild the renderer in place (new
    /// WebGpu device/surface, new render thread) so the window and all its
    /// tabs/panes survive -- unless the circuit breaker has tripped, in
    /// which case fall back to the old destructive close. Otherwise re-arms
    /// for another tick. See `schedule_render_thread_hang_check` for the
    /// scheduling half.
    fn check_render_thread_hang_tick(&mut self, window: &Window) {
        // Clear the "a chain is pending" guard before any other logic in
        // this tick runs (task #287): this tick *is* that pending chain
        // firing, so from this point on `schedule_render_thread_hang_check`
        // must be willing to arm a fresh timer again -- whether that
        // happens below (the `!hung` re-arm path) or later, from
        // `finish_renderer_rebuild` once an in-place rebuild triggered by
        // this same tick completes. Clearing it late (or conditionally)
        // would reopen the race this flag exists to close: a rebuild
        // finishing and calling `schedule_render_thread_hang_check` while
        // this flag was still `true` would be wrongly suppressed, leaving
        // this window with no supervisor at all.
        self.hang_check_scheduled.set(false);

        // Sweep any WebGpu child HWNDs retired by an earlier
        // `begin_renderer_rebuild` (task #283): destroys the ones whose
        // paired `Weak<WebGpuState>` has since hit zero strong references
        // (i.e. the old render thread has actually returned), leaving any
        // others -- still possibly referenced by a render thread that
        // hasn't unwedged yet -- in place for the next tick. Runs
        // unconditionally, before the early-return guards below, so this
        // ~2s cadence is what actually reclaims a retired HWND promptly
        // instead of leaving it until the window closes; see
        // `Window::sweep_retired_webgpu_children`'s doc comment for the
        // full rationale, including why leaving one unswept is never a
        // leak (the top-level window's own `WS_CHILD` teardown is a
        // backstop).
        #[cfg(windows)]
        window.sweep_retired_webgpu_children();

        if self.render_thread_hang_handled.get() {
            // Already rebuilding/closing this window for a hang detected on
            // an earlier tick; a tick that fires after that (a race between
            // the scheduled timer and the rebuild/close actually completing)
            // must be a no-op, not a double-rebuild or double-close.
            return;
        }
        // Two distinct failures, both fatal to this window's rendering and
        // both recovered the same way. Hung: the thread is alive but stuck
        // inside one GPU call. Died: it unwound out of `render_thread_loop`
        // altogether -- a panic in a wgpu call is the observed cause -- and
        // `std::thread` swallowed the panic, so the process lives on with
        // nobody left to turn `Frame` messages into pixels. The second one
        // reads as "not hung" by design (see `render_thread_has_died`), so
        // supervising only `render_thread_is_hung` let a dead render thread
        // freeze a window silently and indefinitely: the message loop keeps
        // pumping, Windows keeps reporting the window as responding, and
        // nothing ever repaints.
        let (hung, died) = match self.render_thread.as_ref() {
            Some(rt) => (rt.render_thread_is_hung(), rt.render_thread_has_died()),
            None => {
                // Render thread is gone (e.g. window already tearing down);
                // nothing left to supervise.
                return;
            }
        };
        if !hung && !died {
            self.schedule_render_thread_hang_check(window);
            return;
        }

        let reason = if died {
            "this window's render thread has exited unexpectedly (a panic inside a GPU \
             call unwinds it, and nothing is left to paint this window)"
        } else {
            "this window's render thread appears stuck inside a GPU submit/reconfigure \
             call (not the whole app -- just this window's GPU driver call)"
        };
        self.attempt_renderer_rebuild_or_close(
            window,
            reason,
            "this window's render thread has hung and been rebuilt",
            "gui.render_thread.window_renderer_rebuilt",
        );
    }

    /// Re-entry point (via `TermWindowNotif::Apply`) for render-error
    /// recovery signals raised from the render thread that aren't a plain
    /// hang: a `wgpu::SurfaceError` variant other than `Lost`/`Outdated`
    /// (see `renderthread::submit_one_frame`'s `other` branch), or a genuine
    /// wgpu device-lost event (see `WebGpuState::new`'s
    /// `set_device_lost_callback` registration). Both signals mean this
    /// window's GPU device/surface is in a broken state that a fresh
    /// `submit_frame` call won't recover from on its own, so this reuses
    /// exactly the same in-place rebuild (and circuit breaker) that
    /// `check_render_thread_hang_tick` uses for a stuck render thread --
    /// from the GUI thread's point of view, "the renderer needs rebuilding"
    /// is the same recovery action regardless of which symptom (hang vs.
    /// repeated surface error vs. device-lost) triggered it.
    ///
    /// Unlike `check_render_thread_hang_tick`, this does not check
    /// `render_thread_is_hung()` (the render thread may not be hung at all
    /// -- `submit_frame` returned promptly with an error, or the device-lost
    /// callback fired inline during a call that itself returned) and does
    /// not self-reschedule (it's not a polling loop; each call corresponds
    /// to one observed error event). It still honors
    /// `render_thread_hang_handled` as a one-shot-per-episode guard, exactly
    /// like the hang path, so a burst of repeated `SurfaceError::Other`
    /// values across several frames (or an error arriving while a
    /// hang-triggered rebuild is already in flight) collapses into a single
    /// rebuild attempt rather than one per event.
    pub(crate) fn handle_render_error_recovery(&mut self, window: &Window, reason: &str) {
        if self.render_thread_hang_handled.get() {
            // A rebuild (or close) for an earlier episode -- hang or error --
            // is already in flight; this event is redundant.
            return;
        }
        self.attempt_renderer_rebuild_or_close(
            window,
            reason,
            "this window's renderer has failed and been rebuilt",
            "gui.render_thread.window_renderer_rebuilt_after_error",
        );
    }

    /// Shared circuit-breaker bookkeeping + rebuild-or-close decision, used
    /// by both the render-thread hang supervisor
    /// (`check_render_thread_hang_tick`) and the render-error recovery entry
    /// point (`handle_render_error_recovery`). Callers are responsible for
    /// their own "should we even consider recovering right now" checks
    /// (hang detection, one-shot guard) before calling this; this function
    /// always sets the one-shot guard, records an attempt, and either
    /// rebuilds or closes.
    ///
    /// `log_reason` describes what was observed (used in the "rebuilding..."
    /// log line); `circuit_breaker_log_reason` is the shorter phrase used in
    /// the circuit-breaker-tripped log line; `rebuilt_metric` is the counter
    /// incremented when a rebuild is actually attempted, so the two call
    /// sites (hang vs. error recovery) remain distinguishable in metrics
    /// even though they now share this implementation.
    fn attempt_renderer_rebuild_or_close(
        &mut self,
        window: &Window,
        log_reason: &str,
        circuit_breaker_log_reason: &str,
        rebuilt_metric: &'static str,
    ) {
        // Set the one-shot guard immediately: everything below this point
        // (the circuit breaker check, the async rebuild, the fallback close)
        // must not race with another recovery trigger for this same
        // episode. It gets reset to `false` once a rebuild actually
        // succeeds (see `finish_renderer_rebuild`), so a *later*, separate
        // failure can also be recovered from -- this is "one-shot per
        // episode", not "one-shot ever".
        self.render_thread_hang_handled.set(true);

        let now = Instant::now();
        {
            let mut attempts = self.rebuild_attempts.borrow_mut();
            attempts.retain(|t| now.duration_since(*t) < Self::REBUILD_WINDOW);
            attempts.push(now);
        }
        let attempts_in_window = self.rebuild_attempts.borrow().len();

        if attempts_in_window > Self::MAX_REBUILDS_PER_WINDOW {
            // The GPU/driver/adapter looks fundamentally broken, not just
            // transiently stuck: rebuilding WebGpu in place has already
            // failed to produce a working renderer `MAX_REBUILDS_PER_WINDOW`
            // times within `REBUILD_WINDOW`. There is no other renderer left
            // to fall back to (task #414 removed the OpenGL/Mesa fallback
            // that used to catch this), so the only thing left to do is
            // close this window cleanly rather than let it sit there
            // silently broken or spin forever retrying.
            log::error!(
                "{} {} times in the last {:?}; giving up on rebuilding WebGpu (the \
                 GPU/driver/adapter looks fundamentally broken, not just transiently \
                 stuck) and closing this window -- OnlyTerm has no other rendering \
                 backend to fall back to",
                circuit_breaker_log_reason,
                attempts_in_window,
                Self::REBUILD_WINDOW,
            );
            metrics::counter!("gui.render_thread.rebuild_circuit_breaker_tripped").increment(1);
            self.close_window_for_unrecoverable_render_hang(window);
            return;
        }

        log::error!(
            "{}; rebuilding this window's renderer in place (attempt {} of {} allowed \
             within {:?}) so its tabs/panes survive",
            log_reason,
            attempts_in_window,
            Self::MAX_REBUILDS_PER_WINDOW,
            Self::REBUILD_WINDOW,
        );
        metrics::counter!(rebuilt_metric).increment(1);

        match rebuild_backoff_for_attempt(attempts_in_window) {
            None => self.begin_renderer_rebuild(window),
            Some(delay) => {
                // Wait before touching the driver again. The reason this
                // matters is not politeness: the first attempt usually fails
                // because the system is momentarily out of memory, and
                // creating a device/surface is exactly what a driver in that
                // state handles worst. A crash dump from this machine caught
                // the failure mode -- a NULL dereference inside
                // `igd10iumd64.dll` while DXGI was building the D3D11 child
                // device for a flip-model swapchain -- with the two rebuild
                // attempts 107ms apart. The circuit breaker counts attempts
                // but never spaced them, so a transient shortage was met with
                // three immediate re-entries into the code that was failing
                // because of it.
                //
                // Deferred through the same timer/notify path
                // `schedule_render_thread_hang_check` uses, rather than a
                // sleep: this runs on the GUI thread, and blocking it here
                // would freeze every other window in the process.
                log::warn!(
                    "waiting {:?} before rebuild attempt {} so a transient GPU memory \
                     shortage has a chance to clear before we ask the driver again",
                    delay,
                    attempts_in_window,
                );
                let deadline = Instant::now() + delay;
                let window = window.clone();
                promise::spawn::spawn(async move {
                    Timer::at(deadline).await;
                    let win = window.clone();
                    window.notify(TermWindowNotif::Apply(Box::new(move |tw| {
                        tw.begin_renderer_rebuild(&win);
                    })));
                })
                .detach();
            }
        }
    }

    /// The destructive fallback: kill this window's panes (and their child
    /// processes) before destroying the OS window, otherwise the
    /// shells/programs running in them are orphaned with no controlling
    /// terminal left. This is the same sequence `close_requested` uses; it's
    /// the true last resort, reached only once the in-place WebGpu rebuild's
    /// circuit breaker trips (task #414 removed the OpenGL fallback that used
    /// to be tried before resorting to this).
    fn close_window_for_unrecoverable_render_hang(&mut self, window: &Window) {
        let mux = Mux::get();
        mux.kill_window(self.mux_window_id);
        window.close();
        front_end().forget_known_window(window);
        metrics::counter!("gui.render_thread.window_closed_for_hang").increment(1);
    }

    /// Kick off the async half of the in-place renderer rebuild (abandoning
    /// the old render thread and dropping the old GPU resources are cheap
    /// and synchronous, so they happen here; `WebGpuState::new` is `async`,
    /// so the rest is done in a spawned task, mirroring the established
    /// pattern in `schedule_render_thread_hang_check` for bridging sync
    /// code -> async GUI-thread-only work -> re-entry via
    /// `TermWindowNotif::Apply`).
    fn begin_renderer_rebuild(&mut self, window: &Window) {
        // Grab the outgoing render thread's teardown sentinel (task #292)
        // *before* taking/shutting it down below: `RenderThreadHandle::
        // teardown_sentinel` only exists on the handle itself, so this has
        // to happen while `self.render_thread` still holds it. See that
        // method's doc comment for why this -- not a `Weak` obtained by
        // downgrading `self.webgpu` -- is the correct signal for
        // `recreate_webgpu_child_window`/`sweep_retired_webgpu_children` to
        // poll: a `Weak<WebGpuState>`'s strong count can read zero while
        // `WebGpuState::drop` (and the `wgpu::Surface`/DXGI swapchain
        // teardown inside it) is still running on the render thread, since
        // `Arc::drop` decrements the strong count before running the
        // value's own `drop_in_place`. The sentinel instead only reports
        // zero strong references once the render thread has fully returned
        // from `render_thread_loop`, strictly after every `Arc<WebGpuState>`
        // clone it held has already been dropped.
        //
        // Only actually consumed on Windows (`recreate_webgpu_child_window`
        // below), since that's the only platform with a dedicated WebGpu
        // child HWND to retire in the first place; the `#[allow]` avoids an
        // unused-variable warning on other platforms where it's computed
        // but never read.
        #[allow(unused_variables)]
        let old_webgpu_weak: std::sync::Weak<dyn std::any::Any + Send + Sync> = self
            .render_thread
            .as_ref()
            .map(|rt| rt.teardown_sentinel())
            .unwrap_or_else(|| {
                std::sync::Weak::<()>::new() as std::sync::Weak<dyn std::any::Any + Send + Sync>
            });

        // Step 1: abandon the old render thread. Detach, don't join --
        // exactly like the `Destroyed` handler: a stuck GPU driver call
        // can't freeze the GUI thread, so blocking here via `.join()` would
        // defeat the whole purpose of having a separate render thread.
        // Sending `Shutdown` (which also sets `window_destroyed` on the
        // shared flag) is enough to let the thread's `recv()` loop end on
        // its own, whenever the driver call it may currently be stuck in
        // eventually returns.
        if let Some(rt) = self.render_thread.take() {
            rt.shutdown();
        }

        // Step 2: retire the old GPU resources in the same order the
        // `Destroyed` handler documents: render_state first, then the
        // device+surface (webgpu) -- but neither is dropped synchronously
        // here any more.
        //
        // We are in this function because this window's device/render
        // thread was already judged unreliable: a hang, a device-lost
        // event, or -- the case that motivated this -- a GPU driver being
        // reinstalled out from under the process. Running `RenderState`'s
        // ordinary `Drop` right now would call straight back into that same
        // suspect driver to release its buffers/textures/glyph atlas,
        // synchronously, on the GUI thread. Observed live: with the driver
        // genuinely unavailable mid-reinstall, that call never returned --
        // it froze the GUI message loop for the rest of the process's life,
        // until the user killed it by hand. Windows' own AppHang mechanism
        // only *reports* a stuck message loop; it does not recover one.
        //
        // `RenderState` holds `Rc`/`RefCell` internally (its glyph
        // cache/layers are shared with the rest of the GUI-thread-only
        // rendering code), so it is `!Send` and cannot be hosted on a
        // background thread for a clean deferred drop. The device it was
        // built against is being discarded here regardless -- a fresh one
        // is what the rest of this function goes on to build -- so once the
        // OS/driver finishes tearing down (or replacing) that device, its
        // own device-removed cleanup reclaims whatever GPU memory this
        // `RenderState` still referenced, whether or not wgpu's `Drop` ever
        // ran for it. `mem::forget` trades a resource release we can no
        // longer safely attempt for a GUI thread that can no longer hang on
        // it.
        if let Some(render_state) = self.render_state.take() {
            std::mem::forget(render_state);
        }
        // Mark the outgoing device stale (task #267) before dropping it: its
        // `set_device_lost_callback` closure keeps living (wgpu gives no way
        // to unregister it) for as long as the underlying `wgpu::Device`
        // handle does, so a *late* device-lost event from this now-abandoned
        // device must be able to tell it's stale and no-op, instead of
        // charging a spurious rebuild attempt against the freshly-rebuilt,
        // perfectly healthy device that replaces it below.
        if let Some(webgpu) = self.webgpu.take() {
            webgpu.mark_stale();
            // Unlike `RenderState` above, `Arc<WebGpuState>` holds no
            // `Rc`/`RefCell` and is `Send`, so its eventual drop can be
            // deferred to a background thread instead of risking the same
            // suspect-driver call on the GUI thread. This is usually a
            // no-op: per `old_webgpu_weak`'s comment above, the
            // just-shutdown render thread typically still holds its own
            // clone, so this is rarely the last strong reference and this
            // thread's only job is decrementing a refcount. It matters in
            // the edge case where that render thread had already exited
            // (e.g. a rebuild-after-failed-rebuild), where this would
            // otherwise be the last reference and run the real teardown.
            std::thread::Builder::new()
                .name("webgpu-drop".to_string())
                .spawn(move || drop(webgpu))
                .ok();
        }

        let window_for_async = window.clone();
        let dimensions = self.dimensions;
        let config = self.config.clone();

        promise::spawn::spawn(async move {
            // Step 3: retire the old WebGpu child HWND and create a fresh
            // one, *before* rebuilding `WebGpuState` below. This has to
            // happen ahead of the `WebGpuState::new` call, not after it:
            // `WebGpuState::new` picks whichever child HWND
            // `window.webgpu_child_hwnd()` currently returns, so rebuilding
            // the surface against the *old* child HWND (the one whose
            // swapchain may itself be the thing that's wedged) would defeat
            // the entire point of task #252's dedicated child HWND.
            //
            // This can't run synchronously back in `begin_renderer_rebuild`
            // (unlike steps 1-2 above): `Window::recreate_webgpu_child_window`
            // needs to borrow this window's `WindowInner`, but
            // `begin_renderer_rebuild` is always reached synchronously from
            // inside `notify()`'s dispatch, which is itself invoked from
            // `Connection::with_window_inner` while that exact `WindowInner`
            // is already mutably borrowed -- a synchronous re-borrow here
            // panics with "already mutably borrowed" (hit in this task's own
            // manual verification). `recreate_webgpu_child_window` is
            // `async` and internally defers its borrow via
            // `promise::spawn::spawn` for exactly this reason (see its doc
            // comment), so awaiting it here, one spawned task removed from
            // the original `notify()` call, is what actually avoids the
            // re-entrant borrow.
            #[cfg(windows)]
            if let Err(err) = window_for_async
                .recreate_webgpu_child_window(old_webgpu_weak)
                .await
            {
                let win = window_for_async.clone();
                window_for_async.notify(TermWindowNotif::Apply(Box::new(move |tw| {
                    tw.finish_renderer_rebuild(&win, Err(err));
                })));
                return;
            }

            let result = WebGpuState::new(
                &window_for_async,
                dimensions,
                &config,
                gpu_recovery_notifier(&window_for_async),
            )
            .await;
            let win = window_for_async.clone();
            window_for_async.notify(TermWindowNotif::Apply(Box::new(move |tw| {
                tw.finish_renderer_rebuild(&win, result);
            })));
        })
        .detach();
    }

    /// Re-entry point (via `TermWindowNotif::Apply`) once the async half of
    /// the rebuild (`WebGpuState::new`) has resolved. On success, rebuilds
    /// `RenderState` against the new device and spawns a fresh render
    /// thread, mirroring `new_window`'s original setup sequence.
    ///
    /// There are two sequential failure points here -- `WebGpuState::new`
    /// itself failing, and (task #272) `self.created` (i.e. `RenderState::new`:
    /// shader compilation, glyph atlas allocation, etc.) failing even though
    /// `WebGpuState::new` just succeeded -- and both re-enter the same
    /// circuit-breaker-gated path (`attempt_renderer_rebuild_or_close`) that
    /// got us here, rather than closing the window immediately. Rationale
    /// (task #255, extended by #272): either failure is, from the circuit
    /// breaker's point of view, just another WebGpu rebuild attempt that
    /// didn't pan out -- exactly like a rebuild that "succeeds" (returns a
    /// device) but then immediately re-hangs, which the breaker already
    /// tolerates up to `MAX_REBUILDS_PER_WINDOW` times. Treating either of
    /// these failures differently (skip straight to close, bypassing the
    /// retry budget entirely) would be an inconsistency with no real
    /// justification: all three symptoms mean "this WebGpu attempt didn't
    /// produce a working renderer", and the breaker's 3-attempts/30s budget
    /// is exactly the mechanism designed to decide when to stop retrying and
    /// give up. So a failure at either point counts as one attempt against
    /// that same budget, and re-calling `attempt_renderer_rebuild_or_close`
    /// naturally either retries WebGpu again (if attempts remain) or, once
    /// the breaker trips, closes the window (see that function; task #414
    /// removed the OpenGL fallback that used to be tried at that point).
    fn finish_renderer_rebuild(&mut self, window: &Window, result: anyhow::Result<WebGpuState>) {
        // Opportunistic extra sweep (task #283): by the time `WebGpuState::new`
        // (real adapter/device/surface setup work, not instantaneous) has
        // resolved, the old render thread this rebuild abandoned has
        // usually already observed the shutdown signal and returned,
        // dropping its `Arc<WebGpuState>`. Sweeping here means the common
        // case -- a healthy render thread that wasn't really wedged, just
        // slow -- gets its retired HWND destroyed right away instead of
        // waiting for the next ~2s `check_render_thread_hang_tick`. Not
        // load-bearing: if the old thread is still wedged, this is a
        // harmless no-op and the next tick (or eventually window close)
        // will catch it once it's safe.
        #[cfg(windows)]
        window.sweep_retired_webgpu_children();

        let webgpu = match result {
            Ok(state) => Arc::new(state),
            Err(err) => {
                // Same failure modes `WebGpuState::new` can hit at initial
                // window creation: RDP session, no GPU passthrough in a VM,
                // a driver mismatch, etc. Retry through the circuit
                // breaker (see doc comment above) instead of closing
                // immediately; `render_thread_hang_handled` is still `true`
                // from the original `attempt_renderer_rebuild_or_close`
                // call that led here, so this is safe to re-enter directly
                // without re-checking it.
                log::error!(
                    "failed to rebuild WebGpu renderer after a render-thread hang ({:#}); \
                     retrying through the rebuild circuit breaker",
                    err
                );
                metrics::counter!("gui.render_thread.rebuild_failed").increment(1);
                self.attempt_renderer_rebuild_or_close(
                    window,
                    "the previous WebGpu rebuild attempt itself failed to create a device/surface",
                    "the WebGpu rebuild attempt has failed",
                    "gui.render_thread.window_renderer_rebuilt",
                );
                return;
            }
        };

        // The WebGpu child HWND was already retired and a fresh one
        // recreated (task #283 onward: the old HWND is *retired*, not
        // destroyed here -- it's swept later once the outgoing render
        // thread's `WebGpuState` has actually dropped, see
        // `sweep_retired_webgpu_children` above) via the spawned task in
        // `begin_renderer_rebuild` that awaits
        // `recreate_webgpu_child_window`, before `WebGpuState::new` was even
        // called. So the surface/device just resolved above already targets
        // the fresh child HWND. Nothing left to do for the HWND here.
        self.webgpu.replace(Arc::clone(&webgpu));
        // Reset frame signature on renderer rebuild - new renderer/surface,
        // so the previous frame is no longer comparable (task #450)
        self.last_frame_signature = None;
        if let Err(err) = self.created(RenderContext(Arc::clone(&webgpu))) {
            // Same reasoning as the `WebGpuState::new` failure arm above
            // (task #272): the device/surface rebuild itself just
            // succeeded, so this is a `RenderState::new` failure (shader
            // compilation, glyph atlas allocation, etc.) on top of a
            // healthy device -- still just another WebGpu rebuild attempt
            // that didn't pan out, from the circuit breaker's point of
            // view. Retry through it instead of closing immediately, so a
            // transient failure right after a device rebuild gets the same
            // retry/OpenGL-fallback chance as any other rebuild hiccup.
            // `self.created` already reset `self.render_state` to `None` on
            // this failure, and the next `begin_renderer_rebuild` (if the
            // breaker allows another attempt) will mark this `self.webgpu`
            // stale and clear it before creating a fresh one, so no stale
            // partial state is left around for a subsequent attempt to trip
            // over.
            log::error!(
                "failed to rebuild RenderState after a successful WebGpu device/surface \
                 rebuild ({:#}); retrying through the rebuild circuit breaker",
                err
            );
            metrics::counter!("gui.render_thread.rebuild_failed").increment(1);
            self.attempt_renderer_rebuild_or_close(
                window,
                "RenderState build failed after a successful device/surface rebuild",
                "the WebGpu rebuild attempt has failed",
                "gui.render_thread.window_renderer_rebuilt",
            );
            return;
        }

        let config = config::configuration();
        if config.webgpu_render_thread {
            let (tx, rx) = std::sync::mpsc::channel();
            let in_flight = Arc::new(AtomicBool::new(false));
            let repaint_pending = Arc::new(AtomicBool::new(false));
            let window_destroyed = Arc::new(AtomicBool::new(false));
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
            self.render_thread =
                crate::renderthread::RenderThreadHandle::spawn(seed, tx, self.mux_window_id)
                    .map(|handle| Box::new(handle) as Box<dyn wezterm_gpu_render::RenderBackend>);
            if self.render_thread.is_some() {
                self.schedule_render_thread_hang_check(window);
            }
        }

        // The rebuild succeeded and a fresh render thread (if configured)
        // is running: re-arm the one-shot guard so a later, separate hang
        // on this same window can also be recovered from.
        self.render_thread_hang_handled.set(false);

        // The old frame's content is gone (new device, new/blank surface);
        // force a full repaint rather than waiting for the next organic
        // invalidate.
        window.invalidate();

        log::info!(
            "successfully rebuilt this window's WebGpu renderer in place after a \
             render-thread hang; window and all its tabs/panes survived"
        );
    }

    fn dispatch_window_event(
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
                // the config via config::configuration() will see the
                // prior version of the config.
                // What's fugly about this is that we'll reload the
                // global config here once per window, which could
                // be nasty for folks with a lot of windows.
                // <https://github.com/wezterm/wezterm/issues/2295>
                config::reload();
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

    fn do_paint_webgpu(&mut self) -> anyhow::Result<bool> {
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

    fn dispatch_notif(&mut self, notif: TermWindowNotif, window: &Window) -> anyhow::Result<()> {
        match notif {
            TermWindowNotif::InvalidateShapeCache => {
                self.shape_generation += 1;
                self.shape_cache.borrow_mut().clear();
                // Task #439: clear shape_hash_cache on shape cache invalidation
                self.shape_hash_cache.borrow_mut().clear();
                self.invalidate_modal();
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
                    self.dispatch_notif(TermWindowNotif::InvalidateShapeCache, window)?;
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
                    // Also handled by wezterm-client
                    self.update_title_post_status_coalesced();
                }
                MuxNotification::TabTitleChanged { .. } => {
                    self.update_title_post_status_coalesced();
                }
                MuxNotification::PaneAdded(_)
                | MuxNotification::WorkspaceRenamed { .. }
                | MuxNotification::PaneRemoved(_)
                | MuxNotification::WindowWorkspaceChanged(_)
                | MuxNotification::ActiveWorkspaceChanged(_)
                | MuxNotification::Empty
                | MuxNotification::WindowCreated(_) => {}
            },
            TermWindowNotif::EmitStatusUpdate => {
                self.emit_status_event();
            }
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

    pub(super) fn set_inner_size(&mut self, window: &Window, width: usize, height: usize) {
        self.resizes_pending += 1;
        window.set_inner_size(width, height);
    }

    /// Take care to remove our panes from the mux, otherwise
    /// we can leave the mux with no windows but some panes
    /// and it won't believe that we are empty.
    pub(super) fn clear_all_overlays(&mut self) {
        let overlay_panes_to_cancel = self
            .pane_state
            .borrow()
            .values()
            .filter_map(|state| state.overlay.as_ref().map(|overlay| overlay.pane.pane_id()))
            .collect::<Vec<_>>();

        for pane_id in overlay_panes_to_cancel {
            self.cancel_overlay_for_pane(pane_id);
        }

        let tab_overlays_to_cancel = self
            .tab_state
            .borrow()
            .iter()
            .filter_map(|(tab_id, state)| state.overlay.as_ref().map(|_| *tab_id))
            .collect::<Vec<_>>();

        for tab_id in tab_overlays_to_cancel {
            self.cancel_overlay_for_tab(tab_id, None);
        }

        self.pane_state.borrow_mut().clear();
        self.tab_state.borrow_mut().clear();
    }

    fn apply_icon(window: &Window) -> anyhow::Result<()> {
        let image = image::load_from_memory(ICON_DATA)?.into_rgba8();
        let (width, height) = image.dimensions();
        window.set_icon(Image::with_rgba32(
            width as usize,
            height as usize,
            width as usize * 4,
            image.as_raw(),
        ));
        Ok(())
    }

    pub(super) fn schedule_status_update(&self) {
        if let Some(window) = self.window.as_ref() {
            window.notify(TermWindowNotif::EmitStatusUpdate);
        }
    }

    fn is_pane_visible(&mut self, pane_id: PaneId) -> bool {
        let mux = Mux::get();
        let tab = match mux.get_active_tab_for_window(self.mux_window_id) {
            Some(tab) => tab,
            None => return false,
        };

        let tab_id = tab.tab_id();
        if let Some(tab_overlay) = self
            .tab_state(tab_id)
            .overlay
            .as_ref()
            .map(|overlay| overlay.pane.clone())
        {
            return tab_overlay.pane_id() == pane_id;
        }

        tab.contains_pane(pane_id)
    }

    fn mux_pane_output_event(&mut self, pane_id: PaneId) {
        metrics::histogram!("mux.pane_output_event.rate").record(1.);
        // `PaneOutput` notifications are delivered to every `TermWindow` in
        // the process (see `subscribe_to_pane_updates` -- the mux
        // subscription is global, not per-window), so this must check
        // `window_contains_pane` before treating the event as "this
        // window's shell is alive"; otherwise output from an unrelated
        // pane in a different OS window would trigger this window's
        // startup fade (task #385) too.
        if !self.shell_output_seen && self.window_contains_pane(pane_id) {
            // First non-empty pty output for this window, from any of its
            // panes/tabs (not just the currently-visible one -- a window
            // can start on a background tab, and the shell in it is no
            // less "alive" for that): tell the OS window layer so it can
            // let the startup placeholder cross-fade into the real content.
            // See `shell_output_seen`'s doc comment for why this specific
            // event was chosen as the "shell is ready" proxy.
            self.shell_output_seen = true;
            if let Some(ref win) = self.window {
                win.notify_shell_ready();
            }
        }
        if self.is_pane_visible(pane_id) {
            if let Some(ref win) = self.window {
                win.invalidate();
            }
        }
    }

    fn mux_pane_output_event_callback(
        n: MuxNotification,
        window: &Window,
        mux_window_id: MuxWindowId,
        dead: &Arc<AtomicBool>,
    ) -> bool {
        if dead.load(Ordering::Relaxed) {
            // Subscription cancelled asynchronously
            return false;
        }

        match n {
            MuxNotification::Alert {
                pane_id,
                alert:
                    Alert::OutputSinceFocusLost
                    | Alert::CurrentWorkingDirectoryChanged
                    | Alert::WindowTitleChanged(_)
                    | Alert::TabTitleChanged(_)
                    | Alert::IconTitleChanged(_)
                    | Alert::Progress(_)
                    | Alert::SetUserVar { .. }
                    | Alert::Bell,
            }
            | MuxNotification::PaneFocused(pane_id)
            | MuxNotification::PaneRemoved(pane_id)
            | MuxNotification::PaneOutput(pane_id) => {
                // Check window validity and propagate to the window event handler
                // that will do the full pane visibility check.
                let mux = Mux::get();
                if mux.get_window(mux_window_id).is_none() {
                    // If the window is not found, the mux_window_id may be stale during
                    // a workspace switch - skip this notif but keep the subscription.
                    // (next notifs should finish the workspace switch & reconcile the state)
                    return true;
                }
                let _ = pane_id;
            }
            MuxNotification::PaneAdded(_pane_id) => {
                // If some other client spawns a pane inside this window, this
                // gives us an opportunity to attach it to the clipboard.
                let mux = Mux::get();
                return mux.get_window(mux_window_id).is_some();
            }
            MuxNotification::TabAddedToWindow { window_id, .. }
            | MuxNotification::WindowTitleChanged { window_id, .. }
            | MuxNotification::WindowInvalidated(window_id) => {
                if window_id != mux_window_id {
                    return true;
                }
            }
            MuxNotification::WindowRemoved(window_id) => {
                if window_id != mux_window_id {
                    return true;
                }
                // The removed window matches our current mux_window_id.
                // During workspace switches, mux_window_id may be stale.
                // Skip this notification but keep the subscription alive.
                // (next notifs should finish the workspace switch & reconcile the state)
                return true;
            }
            MuxNotification::TabReflowed(tab_id)
            | MuxNotification::TabTitleChanged { tab_id, .. } => {
                let mux = Mux::get();
                if mux.window_containing_tab(tab_id) == Some(mux_window_id) {
                    // fall through
                } else {
                    return true;
                }
            }
            MuxNotification::Alert {
                alert: Alert::ToastNotification { .. },
                ..
            }
            | MuxNotification::AssignClipboard { .. }
            | MuxNotification::SaveToDownloads { .. }
            | MuxNotification::WindowCreated(_)
            | MuxNotification::ActiveWorkspaceChanged(_)
            | MuxNotification::WorkspaceRenamed { .. }
            | MuxNotification::Empty
            | MuxNotification::WindowWorkspaceChanged(_) => return true,
            MuxNotification::Alert {
                alert: Alert::PaletteChanged,
                ..
            } => {
                // fall through
            }
        }

        // For PaneOutput notifications, use notify_inline to avoid spawn #3
        // (Connection::with_window_inner). We're already on the main thread,
        // so we can dispatch directly.
        if matches!(n, MuxNotification::PaneOutput(_)) {
            window.notify_inline(TermWindowNotif::MuxNotification(n));
        } else {
            window.notify(TermWindowNotif::MuxNotification(n));
        }

        true
    }

    fn subscribe_to_pane_updates(&self) {
        let window = self.window.clone().expect("window to be valid on startup");
        let mux_window_id = Arc::clone(&self.mux_window_id_for_subscriptions);
        let mux = Mux::get();
        let dead = Arc::clone(&self.mux_subscription_dead);
        mux.subscribe(move |n| {
            if dead.load(Ordering::Relaxed) {
                // Unsubscribe this handler from the mux
                return false;
            }
            // We're already on the main thread here (Mux::notify only calls
            // subscribers from the main thread, either directly or after
            // spawning). No need to spawn again - this was spawn #2.
            let mux_window_id = *mux_window_id.lock().unwrap();
            Self::mux_pane_output_event_callback(n, &window, mux_window_id, &dead)
        });
    }

    fn emit_status_event(&mut self) {
        // update-right-status/update-status events were dispatched to rhai
        // handlers; with the scripting layer removed they have no consumers.
    }

    /// Named window events (`window-resized`, the generic `EmitEvent(name)`
    /// key assignment, etc.) used to be dispatched here to a rhai handler
    /// registered via `wezterm.on(name, ...)`. With the scripting layer
    /// removed there is no handler registry left to receive them, so this
    /// just immediately marks the event as finished (`again: false`, the
    /// same outcome `do_event` used to report when no rhai config was
    /// loaded) without spawning anything. The `EventState`
    /// queueing/de-duplication in [`Self::emit_window_event`] and
    /// [`Self::finish_window_event`] is kept as-is so a queued re-entrant
    /// call is still drained correctly.
    fn schedule_window_event(&mut self, name: &str, _pane_id: Option<PaneId>) {
        self.finish_window_event(name, false);
    }

    /// Called as part of finishing up a window event dispatch.
    /// If again==false it means that there isn't a handler
    /// to execute against, so we should just mark as done.
    /// Otherwise, if there is a queued item, schedule it now.
    fn finish_window_event(&mut self, name: &str, again: bool) {
        let state = self
            .event_states
            .entry(name.to_string())
            .or_insert(EventState::None);
        if again {
            match state {
                EventState::InProgress => {
                    *state = EventState::None;
                }
                EventState::InProgressWithQueued(pane) => {
                    let pane = *pane;
                    *state = EventState::InProgress;
                    self.schedule_window_event(name, pane);
                }
                EventState::None => {}
            }
        } else {
            *state = EventState::None;
        }
    }

    pub fn emit_window_event(&mut self, name: &str, pane_id: Option<PaneId>) {
        if self.get_active_pane_or_overlay().is_none() || self.window.is_none() {
            return;
        }

        let state = self
            .event_states
            .entry(name.to_string())
            .or_insert(EventState::None);
        match state {
            EventState::InProgress => {
                // Flag that we want to run again when the currently
                // executing event calls finish_window_event().
                *state = EventState::InProgressWithQueued(pane_id);
            }
            EventState::InProgressWithQueued(other_pane) => {
                // We've already got one copy executing and another
                // pending dispatch, so don't queue another.
                if pane_id != *other_pane {
                    log::warn!(
                        "Cannot queue {} event for pane {:?}, as \
                         there is already an event queued for pane {:?} \
                         in the same window",
                        name,
                        pane_id,
                        other_pane
                    );
                }
            }
            EventState::None => {
                // Nothing pending, so schedule a call now
                *state = EventState::InProgress;
                self.schedule_window_event(name, pane_id);
            }
        }
    }

    pub(super) fn check_for_dirty_lines_and_invalidate_selection(&mut self, pane: &Arc<dyn Pane>) {
        let dims = pane.get_dimensions();
        let viewport = self
            .get_viewport(pane.pane_id())
            .unwrap_or(dims.physical_top);
        let visible_range = viewport..viewport + dims.viewport_rows as StableRowIndex;
        let seqno = self.selection(pane.pane_id()).seqno;
        let dirty = pane.get_changed_since(visible_range, seqno);

        if dirty.is_empty() {
            return;
        }
        if pane.downcast_ref::<CopyOverlay>().is_none()
            && pane.downcast_ref::<QuickSelectOverlay>().is_none()
        {
            // If any of the changed lines intersect with the
            // selection, then we need to clear the selection, but not
            // when the search overlay is active; the search overlay
            // marks lines as dirty to force invalidate them for
            // highlighting purpose but also manipulates the selection
            // and we want to allow it to retain the selection it made!

            let clear_selection =
                if let Some(selection_range) = self.selection(pane.pane_id()).range.as_ref() {
                    let selection_rows = selection_range.rows();
                    selection_rows.into_iter().any(|row| dirty.contains(row))
                } else {
                    false
                };

            if clear_selection {
                self.selection(pane.pane_id()).range.take();
                self.selection(pane.pane_id()).origin.take();
                self.selection(pane.pane_id()).seqno = pane.get_current_seqno();
            }
        }
    }
}

#[cfg(test)]
mod render_state_send_bound_tests {
    use std::sync::Arc;
    use wezterm_gpu_render::WebGpuState;

    fn assert_send<T: Send>() {}

    /// `Arc<WebGpuState>` must stay `Send` for the background-thread drop in
    /// `begin_renderer_rebuild`'s Step 2 to compile at all -- if this stops
    /// compiling, that drop path needs to change to something else that
    /// still keeps the suspect-driver call off the GUI thread, not just
    /// have this assertion deleted.
    ///
    /// The other half of Step 2's reasoning -- `RenderState` must stay
    /// `!Send`, which is why it is `mem::forget`-ten there instead of
    /// following this same background-thread path -- has no equivalent
    /// test: asserting "does not implement Send" would need to fail to
    /// *compile*, which a `#[test]` cannot express without a compile-fail
    /// harness this crate doesn't otherwise depend on. That half of the
    /// invariant is stated in `begin_renderer_rebuild`'s own comments
    /// instead; if `RenderState` ever becomes `Send` (e.g. its internal
    /// `Rc`/`RefCell` replaced with `Arc`/`Mutex`), its `mem::forget` should
    /// become a background drop like this one.
    #[test]
    fn webgpu_state_arc_is_send() {
        assert_send::<Arc<WebGpuState>>();
    }
}
