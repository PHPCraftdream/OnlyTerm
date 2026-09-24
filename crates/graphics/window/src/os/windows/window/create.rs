use super::input::keyboard_layout::KeyboardLayoutInfo;
use super::msg::frame::{
    apply_theme, decorations_to_style, no_native_title_bar, schedule_apply_decoration,
};
use super::msg::paint::paint_parent_placeholder_spinner;
use super::msg::wndproc::wnd_proc;
use super::placeholder::PlaceholderSpinner;
use super::*;
use crate::connection::ConnectionOps;
use crate::{RequestedWindowGeometry, ResolvedGeometry, WindowEvent, WindowEventSender};
use anyhow::bail;
use onlyterm_config::ConfigHandle;
use onlyterm_font::FontConfiguration;
use std::any::Any;
use std::cell::RefCell;
use std::io::Error as IoError;
use std::ptr::{null, null_mut};
use std::rc::Rc;
use winapi::um::libloaderapi::GetModuleHandleW;
use winapi::um::shellapi::DragAcceptFiles;

impl Window {
    fn create_window(
        config: ConfigHandle,
        class_name: &str,
        name: &str,
        geometry: ResolvedGeometry,
        lparam: *const RefCell<WindowInner>,
    ) -> anyhow::Result<HWND> {
        let class_name = wide_string(class_name);
        // SAFETY: null module name returns the current process's exe handle,
        // which is always valid and non-null on Windows.
        let h_inst = unsafe { GetModuleHandleW(null()) };
        let class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW | CS_OWNDC,
            lpfnWndProc: Some(wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: h_inst,
            // FIXME: this resource is specific to the onlyterm build and this should
            // really be made generic for other sorts of windows.
            // The ID is defined in assets/windows/resource.rc
            // SAFETY: `h_inst` is a valid module handle and `MAKEINTRESOURCEW(0x101)`
            // is a valid resource-id token for the bundled icon.
            hIcon: unsafe { LoadIconW(h_inst, MAKEINTRESOURCEW(0x101)) },
            hCursor: null_mut(),
            hbrBackground: null_mut(),
            lpszMenuName: null(),
            lpszClassName: class_name.as_ptr(),
        };

        // SAFETY: `class` is a fully-initialized `WNDCLASSW` with valid string
        // pointers and a registered `wnd_proc`; the failure case (return 0) is
        // handled below, including the benign CLASS_ALREADY_EXISTS case.
        if unsafe { RegisterClassW(&class) } == 0 {
            let err = IoError::last_os_error();
            match err.raw_os_error() {
                Some(code)
                    if code == winapi::shared::winerror::ERROR_CLASS_ALREADY_EXISTS as i32 => {}
                _ => return Err(err.into()),
            }
        }

        let decorations = config.window_decorations;
        let style = decorations_to_style(decorations);
        let frame_dpi = get_primary_monitor_dpi();
        let (width, height) =
            adjust_client_to_window_dimensions(style, geometry.width, geometry.height, frame_dpi);

        let (x, y) = match (geometry.x, geometry.y) {
            (Some(x), Some(y)) => (x, y),
            _ => {
                if (style & WS_POPUP) == 0 {
                    (CW_USEDEFAULT, CW_USEDEFAULT)
                } else {
                    // WS_POPUP windows need to specify the initial position.
                    // We pick the middle of the primary monitor

                    // SAFETY: `mi` is zeroed then sized before use; the monitor
                    // handle and info pointer are valid and only written to.
                    unsafe {
                        let mut mi: MONITORINFO = std::mem::zeroed();
                        mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
                        GetMonitorInfoW(
                            MonitorFromWindow(std::ptr::null_mut(), MONITOR_DEFAULTTOPRIMARY),
                            &mut mi,
                        );

                        let mon_width = mi.rcMonitor.right - mi.rcMonitor.left;
                        let mon_height = mi.rcMonitor.bottom - mi.rcMonitor.top;

                        (
                            mi.rcMonitor.left + (mon_width - width) / 2,
                            mi.rcMonitor.top + (mon_height - height) / 2,
                        )
                    }
                }
            }
        };

        let name = wide_string(name);
        // SAFETY: `class_name`/`name` are live null-terminated UTF-16 buffers,
        // all handle/pointer args are null (no parent/menu/instance), and
        // `lparam` is an `Rc` raw pointer produced by `rc_to_pointer` that is
        // recovered as the window's `WM_CREATE`/`WM_NCCREATE` lparam. A null
        // result is reported as an error below.
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class_name.as_ptr(),
                name.as_ptr(),
                style,
                x,
                y,
                width,
                height,
                null_mut(),
                null_mut(),
                null_mut(),
                std::mem::transmute::<*const RefCell<WindowInner>, LPVOID>(lparam),
            )
        };

        if hwnd.is_null() {
            let err = IoError::last_os_error();
            bail!("CreateWindowExW: {}", err);
        }

        // We have to re-apply the styles otherwise they don't
        // completely stick
        schedule_apply_decoration(hwnd, decorations);

        Ok(hwnd)
    }

    /// Create the child `WS_CHILD` window that the WebGpu swapchain surface
    /// targets, parented to `parent` and sized to exactly cover its current
    /// client area.
    ///
    /// This exists because DXGI only permits one swapchain per HWND: putting
    /// the surface on a dedicated child HWND (rather than directly on the
    /// application's own top-level HWND) means a future in-place renderer
    /// rebuild (task #253, not yet implemented) can tear down and recreate
    /// the child HWND/surface without fighting the top-level window's own
    /// swapchain lifetime. Today this child window is purely structural: it
    /// is kept perfectly in sync with the parent's client area and made
    /// input-transparent (see `child_wnd_proc`'s `WM_NCHITTEST` handling), so
    /// behavior is externally identical to rendering directly on the
    /// top-level HWND.
    fn create_webgpu_child_window(parent: HWND) -> anyhow::Result<HWND> {
        let class_name = wide_string("OnlyTermWebGpuChild");
        // SAFETY: null module name returns the current process's exe handle,
        // which is always valid and non-null on Windows.
        let h_inst = unsafe { GetModuleHandleW(null()) };
        let class = WNDCLASSW {
            // Deliberately NOT `CS_HREDRAW | CS_VREDRAW`. Those styles force
            // a full invalidate-with-erase of the window on every width or
            // height change, which is what you want for a window that draws
            // itself in `WM_PAINT`. This one draws nothing in `WM_PAINT` at
            // all -- its pixels come from the swapchain that DXGI presents
            // to it. The forced erase is therefore pure downside: once the
            // startup placeholder has been retired, `WM_ERASEBKGND` here
            // paints nothing (see `child_wnd_proc`) and the null background
            // brush means `DefWindowProc` doesn't either, so every resize
            // blanked this window to undefined pixels until the swapchain
            // happened to present again -- which the compositor showed as a
            // see-through flash of whatever was behind the window. Without
            // these styles a resize leaves the previously presented frame on
            // screen until the next one lands, which is both correct and
            // what every other swapchain-backed window does.
            style: 0,
            lpfnWndProc: Some(child_wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: h_inst,
            hIcon: null_mut(),
            hCursor: null_mut(),
            hbrBackground: null_mut(),
            lpszMenuName: null(),
            lpszClassName: class_name.as_ptr(),
        };

        // SAFETY: `class` is a fully-initialized `WNDCLASSW` with valid string
        // pointers and a registered `child_wnd_proc`; the failure case (return
        // 0) is handled below, including the benign CLASS_ALREADY_EXISTS case
        // (multiple top-level windows in the same process share this class).
        if unsafe { RegisterClassW(&class) } == 0 {
            let err = IoError::last_os_error();
            match err.raw_os_error() {
                Some(code)
                    if code == winapi::shared::winerror::ERROR_CLASS_ALREADY_EXISTS as i32 => {}
                _ => return Err(err.into()),
            }
        }

        let mut rect = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        // SAFETY: `rect` is a live stack `RECT` and `parent` is a valid,
        // just-created window handle; `GetClientRect` only writes into `rect`.
        unsafe {
            GetClientRect(parent, &mut rect);
            caption::content_rect(parent, &mut rect);
        }

        let name = wide_string("OnlyTermWebGpuChild");
        // SAFETY: `class_name`/`name` are live null-terminated UTF-16 buffers.
        // `parent` is the valid, just-created top-level HWND, so passing it
        // makes this a `WS_CHILD` window owned by it (destroyed automatically
        // when `parent` is destroyed). No menu/custom instance/create-params
        // are needed since this window has no `WindowInner` of its own. A
        // null result is reported as an error below.
        //
        // Created WITHOUT `WS_VISIBLE` on purpose: this window has no pixels
        // of its own until the swapchain presents its first frame, and while
        // it is visible it covers the parent's entire client area -- so
        // showing it before then composites undefined pixels over the
        // parent's startup placeholder, which reads as a see-through flash
        // of whatever is behind the window. Callers show it once there is
        // actually something to show: `clear_placeholder_background` (the
        // first-frame-presented signal) on the startup path, and
        // `recreate_webgpu_child_window` immediately on the renderer-rebuild
        // path, where a renderer was already up and the placeholder is long
        // gone.
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class_name.as_ptr(),
                name.as_ptr(),
                WS_CHILD,
                rect.left,
                rect.top,
                rect_width(&rect),
                rect_height(&rect),
                parent,
                null_mut(),
                h_inst,
                null_mut(),
            )
        };

        if hwnd.is_null() {
            let err = IoError::last_os_error();
            bail!("CreateWindowExW (webgpu child): {}", err);
        }

        Ok(hwnd)
    }

    pub async fn new_window<F>(
        class_name: &str,
        name: &str,
        geometry: RequestedWindowGeometry,
        config: Option<&ConfigHandle>,
        _font_config: Rc<FontConfiguration>,
        event_handler: F,
    ) -> anyhow::Result<Window>
    where
        F: 'static + FnMut(WindowEvent, &Window),
    {
        let events = WindowEventSender::new(event_handler);

        let config = match config {
            Some(c) => c.clone(),
            None => onlyterm_config::configuration(),
        };
        let appearance = get_appearance();

        // Create the placeholder spinner's GDI objects up front, colored
        // from the *effective* palette (color scheme + explicit overrides
        // already resolved into `config.resolved_palette`, exactly like
        // `TermConfig::color_palette` does for the terminal model itself --
        // see `crates/configuration/config/src/terminal.rs`), not hardcoded colors. This
        // is the same background/foreground the terminal will actually
        // paint once the renderer comes up, so the spinner shown to the
        // user before the first GPU frame is drawn in colors that already
        // match the real thing. Hardcoding e.g. a white background here
        // would be invisible for this fork's light-theme default but would
        // flash white on every dark theme -- precisely the defect this
        // placeholder exists to prevent.
        //
        // SAFETY: `PlaceholderSpinner::new` only reads plain config values
        // and creates GDI objects, which is always safe.
        let placeholder_spinner = Some(unsafe { PlaceholderSpinner::new(&config) });

        let inner = Rc::new(RefCell::new(WindowInner {
            hwnd: HWindow(null_mut()),
            webgpu_child_hwnd: HWindow(null_mut()),
            caption: caption::Caption::default(),
            retired_webgpu_children: Vec::new(),
            appearance,
            events,
            vscroll_remainder: 0,
            hscroll_remainder: 0,
            keyboard_info: KeyboardLayoutInfo::new(),
            last_size: None,
            in_size_move: false,
            dead_pending: None,
            saved_placement: None,
            track_mouse_leave: false,
            window_drag_position: None,
            maximize_button_position: None,
            config: config.clone(),
            paint_throttled: false,
            invalidated: true,
            placeholder_spinner,
            renderer_ready: false,
            shell_ready: false,
            placeholder_fade: None,
            extra_ref_reclaimed_by_ncdestroy: std::cell::Cell::new(false),
        }));

        // Careful: `raw` owns a ref to inner, but there is no Drop impl
        let raw = rc_to_pointer(&inner);

        let conn = Connection::get().expect("Connection::init was not called");

        let geometry = conn.resolve_geometry(geometry);

        let centered_caption = config.show_process_tree_stats_in_title
            && !no_native_title_bar(config.window_decorations);
        let hwnd = match Self::create_window(config, class_name, name, geometry, raw) {
            Ok(hwnd) => HWindow(hwnd),
            Err(err) => {
                // `CreateWindowExW` always delivers `WM_NCCREATE` before it
                // can fail. If it failed *after* `WM_NCCREATE` ran (e.g.
                // GDI/USER handle exhaustion), Windows also synchronously
                // sends `WM_NCDESTROY` to unwind the partially-created
                // window, and `wm_ncdestroy` has already reclaimed (and
                // will drop) this same extra ref via `take_rc_from_pointer`
                // -- see `WindowInner::extra_ref_reclaimed_by_ncdestroy`.
                // Reclaiming it again here would double-drop the same
                // strong reference (task #402). Only reclaim it ourselves
                // when `wm_ncdestroy` never ran, i.e. `CreateWindowExW`
                // failed before `WM_NCCREATE` (bad class/parent handle,
                // etc.), in which case `GWLP_USERDATA` was never populated
                // and this is still the sole owner of that extra ref.
                if should_new_window_drop_extra_ref(
                    inner.borrow().extra_ref_reclaimed_by_ncdestroy.get(),
                ) {
                    // SAFETY: `raw` was produced by `rc_to_pointer` above (a
                    // valid `Rc` raw pointer with one extra strong ref) and
                    // `should_new_window_drop_extra_ref` confirmed
                    // `wm_ncdestroy` has not already reclaimed it, so this
                    // is the first and only reclaim of that ref.
                    drop(unsafe { Rc::from_raw(raw) });
                }
                return Err(err);
            }
        };

        let webgpu_child_hwnd = match Self::create_webgpu_child_window(hwnd.0) {
            Ok(child) => HWindow(child),
            Err(err) => {
                log::error!(
                    "Failed to create WebGpu child window ({:#}); WebGpu surface \
                     creation will fall back to the top-level window",
                    err
                );
                HWindow(null_mut())
            }
        };

        let window_handle = Window(hwnd);
        {
            let mut inner_mut = inner.borrow_mut();
            inner_mut.webgpu_child_hwnd = webgpu_child_hwnd;
            inner_mut.caption.set_text(name, None);
            inner_mut.events.assign_window(window_handle.clone());
        }

        // SAFETY: parent and its child have been created; change the frame before showing it.
        unsafe {
            caption::configure(hwnd.0, centered_caption && !webgpu_child_hwnd.0.is_null());
            SetWindowPos(
                hwnd.0,
                null_mut(),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
            );
        }

        apply_theme(hwnd.0);
        // NOT `enable_blur_behind(hwnd.0)` here: see the call in
        // `clear_placeholder_background` for why it's deferred.

        // Make window capable of accepting drag and drop
        // SAFETY: `hwnd.0` is a valid, just-created window handle.
        unsafe {
            DragAcceptFiles(hwnd.0, winapi::shared::minwindef::TRUE);
        }

        conn.windows.borrow_mut().insert(hwnd, Rc::clone(&inner));

        Ok(window_handle)
    }

    /// Returns the raw HWND of the `WS_CHILD` window that the WebGpu
    /// swapchain surface should target (see `create_webgpu_child_window`),
    /// or `None` if it doesn't exist (e.g. its creation failed and we fell
    /// back to targeting the top-level window directly).
    ///
    /// Callable synchronously because this is only ever used from the GUI's
    /// main/connection thread during window/surface setup, matching
    /// `HasWindowHandle for Window`'s synchronous `Connection::get_window`
    /// access pattern just above.
    pub fn webgpu_child_hwnd(&self) -> Option<isize> {
        let conn = Connection::get()?;
        let handle = conn.get_window(self.0)?;
        let inner = handle.borrow();
        if inner.webgpu_child_hwnd.0.is_null() {
            None
        } else {
            Some(inner.webgpu_child_hwnd.0 as isize)
        }
    }

    /// Retire the existing WebGpu child window (if any) and create a fresh
    /// one in its place, parented to this window and sized to its current
    /// client area. Used by task #253's in-place renderer rebuild: when a
    /// window's render thread is found stuck inside a GPU submit call, we
    /// tear down and recreate the whole WebGpu stack (instance/adapter/
    /// device/surface) rather than the whole top-level OS window, and this
    /// child HWND is the reason that's possible at all -- DXGI only allows
    /// one swapchain per HWND, so recreating a surface on the *same* HWND
    /// that already has a (possibly wedged) swapchain on it doesn't work,
    /// but retiring (see below) and creating a fresh plain child window is
    /// safe and fast.
    ///
    /// `old_webgpu_state` is a type-erased `Weak` handle (this crate cannot
    /// name `onlyterm_gui::termwindow::webgpu::WebGpuState` directly, hence
    /// `dyn Any`) to the `WebGpuState` whose surface targets the *old* child
    /// HWND, downgraded from the caller's `Arc` right before the caller
    /// drops its own strong reference (see `begin_renderer_rebuild`). We do
    /// not immediately `DestroyWindow` the old child here (task #283): if
    /// the just-shut-down render thread is wedged inside
    /// `submit_frame`/`present()` -- the whole reason a rebuild was
    /// triggered -- it can still hold the other strong `Arc<WebGpuState>`
    /// (`RenderThreadSeed::webgpu`), keeping the live surface/DXGI
    /// swapchain targeting this HWND alive. Destroying the HWND out from
    /// under a still-possibly-live swapchain is undefined, driver-dependent
    /// behavior. Instead the old child is hidden and stashed (see
    /// `retired_webgpu_children`) until `sweep_retired_webgpu_children`
    /// observes `old_webgpu_state.strong_count() == 0` (i.e. the render
    /// thread has actually returned and dropped its `Arc`), at which point
    /// it's safe to actually destroy.
    ///
    /// `async` and deferred via `onlyterm_promise::spawn::spawn`: the caller
    /// (`TermWindow`'s render-thread hang supervisor) always reaches this
    /// synchronously from inside `notify()`'s `WindowEvent::Notification`
    /// dispatch, which is itself invoked from `Connection::with_window_inner`
    /// while it still holds this exact window's `WindowInner` `RefCell`
    /// mutably borrowed (see `notify`). Borrowing it again *synchronously*
    /// here would panic with "already mutably borrowed" -- this bit the
    /// first version of this method in manual testing. Deferring the actual
    /// `get_window`/`borrow` to a freshly spawned task lets that outer borrow
    /// finish and drop first.
    pub async fn recreate_webgpu_child_window(
        &self,
        old_webgpu_state: std::sync::Weak<dyn Any + Send + Sync>,
    ) -> anyhow::Result<()> {
        let window = self.0;
        onlyterm_promise::spawn::spawn(async move {
            let conn = Connection::get().ok_or_else(|| anyhow::anyhow!("no Connection"))?;
            let handle = conn
                .get_window(window)
                .ok_or_else(|| anyhow::anyhow!("window handle invalid!?"))?;

            let parent = handle.borrow().hwnd.0;
            let old_child = handle.borrow().webgpu_child_hwnd.0;
            if !old_child.is_null() {
                // SAFETY: `old_child` is a live `WS_CHILD` window handle
                // created by an earlier `create_webgpu_child_window` call
                // (or a previous `recreate_webgpu_child_window` call).
                // `ShowWindow(SW_HIDE)` on the window's own owning/
                // connection thread merely hides it -- unlike `DestroyWindow`,
                // this is safe even if the old surface's swapchain might
                // still be alive/in-use on another thread.
                unsafe {
                    ShowWindow(old_child, SW_HIDE);
                }
                {
                    let mut inner = handle.borrow_mut();
                    inner
                        .retired_webgpu_children
                        .push((HWindow(old_child), old_webgpu_state));
                    // Null out the field immediately, before attempting to
                    // create the replacement, so a `?`-triggered early
                    // return below (or `webgpu_child_hwnd()` observed from
                    // any other thread in the meantime) correctly reports
                    // "no child window" rather than pointing at a
                    // now-retired (albeit not yet destroyed) HWND that's no
                    // longer the one any new surface should target.
                    inner.webgpu_child_hwnd = HWindow(null_mut());
                }
            }

            let new_child = Self::create_webgpu_child_window(parent)?;
            let mut inner = handle.borrow_mut();
            inner.webgpu_child_hwnd = HWindow(new_child);
            // `create_webgpu_child_window` returns a hidden window so the
            // startup path can defer showing it until its first frame
            // lands. That deferral doesn't apply here: a renderer was
            // already up (that's what is being rebuilt), so the startup
            // placeholder is long gone and there is nothing else left to
            // paint this area -- leaving the child hidden until the
            // rebuilt renderer's own first frame would blank the window
            // for the whole rebuild instead of holding the last image.
            // Show it immediately, matching the pre-deferral behavior on
            // this path exactly.
            if inner.renderer_ready {
                // SAFETY: `new_child` is the valid `WS_CHILD` handle just
                // returned above, owned by `parent`. `SW_SHOWNA` shows it
                // without activating.
                unsafe {
                    ShowWindow(new_child, SW_SHOWNA);
                }
            }
            // A new WebGpu child is created at the top of the parent's child
            // z-order, which would land above any active fade overlay. Rather
            // than re-assert the overlay's z-order against the newcomer,
            // finish the fade instantly (same principle as the resize path
            // in `check_and_call_resize_if_needed`).
            if inner.placeholder_fade.is_some() {
                inner.finish_placeholder_fade();
            }
            Ok(())
        })
        .await
    }

    /// Destroy any retired WebGpu child HWNDs (see
    /// `retired_webgpu_children`) whose paired `Weak<WebGpuState>` has hit
    /// zero strong references, i.e. whose old render thread has actually
    /// returned and dropped the `Arc` that kept its surface/swapchain
    /// alive. Called periodically from `TermWindow::check_render_thread_hang_tick`'s
    /// existing ~2s timer (see that function's doc comment) while a render
    /// thread exists, and once more from `close` so a full window close
    /// clears the list eagerly rather than leaving it to `hwnd`'s own
    /// `WS_CHILD` teardown (still a correct backstop either way -- see
    /// `retired_webgpu_children`'s doc comment).
    ///
    /// Safe to call with no retired windows (no-op) and safe to call
    /// repeatedly (each entry is only ever destroyed once, then removed).
    ///
    /// Deferred via `onlyterm_promise::spawn::spawn`, exactly like
    /// `recreate_webgpu_child_window` above and for the identical reason
    /// (task #291): both call sites (`check_render_thread_hang_tick`,
    /// `finish_renderer_rebuild`) reach this synchronously from inside
    /// `notify()`'s `WindowEvent::Notification` dispatch, which is itself
    /// invoked from `Connection::with_window_inner` while it still holds
    /// this exact window's `WindowInner` `RefCell` mutably borrowed (see
    /// `notify`). Borrowing it again *synchronously* here -- as this method
    /// used to do -- panics with "already mutably borrowed" on literally
    /// the first `check_render_thread_hang_tick` timer fire after a window
    /// opens with WebGpu + the render-thread hang supervisor enabled
    /// (defaults), since that outer borrow is still on the stack. Spawning
    /// a fresh task lets that outer borrow finish and drop first, then
    /// performs the sweep a moment later on the main thread -- still well
    /// within the ~2s hang-check cadence the caller's doc comment relies on
    /// for prompt HWND reclamation (spawned tasks run essentially
    /// immediately once the current dispatch unwinds, not on any
    /// significant delay), and still a plain fire-and-forget no-op if there
    /// happen to be no retired children.
    pub fn sweep_retired_webgpu_children(&self) {
        let window = self.0;
        onlyterm_promise::spawn::spawn(async move {
            let Some(conn) = Connection::get() else {
                return;
            };
            let Some(handle) = conn.get_window(window) else {
                return;
            };
            handle.borrow_mut().sweep_retired_webgpu_children();
        })
        .detach();
    }
}

impl WindowInner {
    /// Destroy any retired WebGpu child HWNDs whose paired `Weak` has hit
    /// zero strong references. Shared body for `Window::sweep_retired_webgpu_children`
    /// (reached via `Connection::get_window`) and `close` below (which
    /// already holds `&mut self` directly, so it can call this without an
    /// extra `Connection` round-trip). See `retired_webgpu_children`'s doc
    /// comment for the full rationale.
    pub(super) fn sweep_retired_webgpu_children(&mut self) {
        self.retired_webgpu_children.retain(|(hwnd, weak)| {
            if weak.strong_count() > 0 {
                // Still (possibly) referenced by a not-yet-returned render
                // thread; leave it hidden and retired for the next sweep.
                return true;
            }
            // SAFETY: `hwnd.0` is a retired `WS_CHILD` window handle that
            // hasn't been destroyed yet (this closure only runs once per
            // entry before `retain` drops it), and `weak`'s zero strong
            // count proves the `WebGpuState`/surface that used to target it
            // has been fully dropped, so nothing can still be
            // presenting/configuring against this HWND.
            unsafe {
                DestroyWindow(hwnd.0);
            }
            false
        });
    }
}

/// Window procedure for the small `WS_CHILD` window that hosts the WebGpu
/// swapchain surface (see `Window::create_webgpu_child_window`).
///
/// This window has no `WindowInner`/`GWLP_USERDATA` of its own -- it is pure
/// plumbing that exists only so DXGI has a dedicated HWND to attach a
/// swapchain to. Two messages matter. `WM_NCHITTEST`: returning
/// `HTTRANSPARENT` makes Windows route all mouse input (clicks, drags,
/// hover, wheel) through to whatever is beneath this window in Z-order --
/// i.e. the parent top-level window -- exactly as if this child window
/// didn't exist from an input-routing perspective. Keyboard input is
/// unaffected by hit-testing and already reaches the parent, since this
/// child window is never focused (nothing ever calls `SetFocus` on it).
/// `WM_ERASEBKGND`: paints the parent's placeholder background during the
/// window between the window being shown and the swapchain's first frame,
/// see the handler below.
///
/// # Safety
/// This is the `WNDCLASSW::lpfnWndProc` callback: Win32 supplies a valid
/// `hwnd` and the raw message arguments.
unsafe extern "system" fn child_wnd_proc(
    hwnd: HWND,
    msg: UINT,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match std::panic::catch_unwind(|| {
        if msg == WM_NCHITTEST {
            return HTTRANSPARENT as LRESULT;
        }
        if msg == WM_ERASEBKGND {
            // This child window is created up front, together with the
            // top-level window -- not when WebGpu finishes initializing --
            // and it is `WS_VISIBLE` from the start, covering the parent's
            // entire client area. So the parent's own placeholder paint is
            // painted *underneath* it and never visible, while this window
            // paints nothing at all until the swapchain presents its first
            // frame seconds later. That gap is what the user sees as a
            // blank white rectangle. Paint the parent's spinner here
            // instead; once the renderer is up, `clear_placeholder_
            // background` drops it and this goes back to being a no-op,
            // leaving the swapchain in sole control of these pixels.
            let hdc = wparam as HDC;
            let mut rect = RECT {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            };
            // SAFETY: `hwnd` is this valid child window handle; `rect`
            // is a live stack `RECT` that `GetClientRect` only writes
            // into.
            GetClientRect(hwnd, &mut rect);
            // SAFETY: `hwnd` is this valid child window handle; `hdc` is
            // the device context Win32 passed in `wparam` of a real
            // `WM_ERASEBKGND` and is live for `rect`, this child's full
            // client area.
            if paint_parent_placeholder_spinner(hwnd, hdc, &rect) {
                return 1;
            }
        }
        // SAFETY: `hwnd`/`msg`/`wparam`/`lparam` are the values Win32 just
        // supplied to this wndproc; `DefWindowProcW` is always valid to call
        // with them.
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }) {
        Ok(result) => result,
        Err(e) => {
            log::error!("caught {:?}", e);
            std::process::exit(1)
        }
    }
}
