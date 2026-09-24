use super::super::placeholder::{PLACEHOLDER_SPINNER_INTERVAL_MS, PLACEHOLDER_SPINNER_TIMER_ID};
use super::super::*;
use super::mouse::{mouse_coords, screen_to_client};
use super::paint::wm_paint;
use crate::{
    Appearance, Dimensions, Rect, ScreenPoint, WindowDecorations, WindowEvent, WindowState,
};
use onlyterm_config::SystemBackdrop;
use shared_library::shared_library;
use std::ptr::null_mut;
use winapi::shared::winerror::S_OK;
use winapi::um::uxtheme::{
    CloseThemeData, GetThemeFont, GetThemeSysFont, OpenThemeData, SetWindowTheme,
};
use winapi::um::wingdi::LOGFONTW;

impl WindowInner {
    fn get_effective_dpi(&self) -> usize {
        // SAFETY: `self.hwnd.0` is a live window handle.
        let actual_dpi = unsafe { GetDpiForWindow(self.hwnd.0) } as f64;

        if self.config.dpi_by_screen.is_empty() {
            return self.config.dpi.unwrap_or(actual_dpi) as usize;
        }

        // SAFETY: `mi` is zeroed then sized correctly before use; `MonitorFromWindow`
        // and `GetMonitorInfoW` receive a valid `hwnd`/`MONITORINFO` pointer and only
        // write into `mi`.
        unsafe {
            let mut mi: MONITORINFOEXW = std::mem::zeroed();
            mi.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
            let mon = MonitorFromWindow(self.hwnd.0, MONITOR_DEFAULTTONEAREST);
            GetMonitorInfoW(mon, &mut mi as *mut MONITORINFOEXW as *mut MONITORINFO);

            if let Ok(info) = crate::os::windows::connection::ScreenInfoHelper::new() {
                let name = info.monitor_name(&mi);
                if let Some(dpi) = self.config.dpi_by_screen.get(&name).copied() {
                    return dpi as usize;
                }
            }

            actual_dpi as usize
        }
    }

    /// Check if we need to generate a resize callback.
    /// Calls resize if needed.
    /// Returns true if we did.
    fn check_and_call_resize_if_needed(&mut self) -> bool {
        let mut rect = RECT {
            left: 0,
            bottom: 0,
            right: 0,
            top: 0,
        };
        // SAFETY: `rect` is a live stack `RECT` and `self.hwnd.0` is a valid
        // window handle; `GetClientRect` only writes the client rect into it.
        unsafe {
            GetClientRect(self.hwnd.0, &mut rect);
            caption::content_rect(self.hwnd.0, &mut rect);
        }
        let pixel_width = rect_width(&rect) as usize;
        let pixel_height = rect_height(&rect) as usize;

        // Keep the WebGpu child window (see `create_webgpu_child_window`)
        // sized/positioned to exactly cover the parent's client area. This
        // runs on every resize/move/DPI-change notification (this function
        // is reached from both `wm_size` and `wm_windowposchanged`, which is
        // also how DPI-driven geometry changes are observed since there is
        // no separate `WM_DPICHANGED` handler), so the child never lags
        // behind, even during live interactive resizing.
        if !self.webgpu_child_hwnd.0.is_null() {
            // SAFETY: `webgpu_child_hwnd.0` is a valid child window handle
            // owned by `self.hwnd.0`; `rect.left`/`rect.top` are always 0
            // (client-relative origin) so the child exactly overlays the
            // parent's client area. `SWP_NOACTIVATE|SWP_NOZORDER` avoid
            // disturbing focus/z-order, which are otherwise unrelated to a
            // pure resize/reposition.
            unsafe {
                SetWindowPos(
                    self.webgpu_child_hwnd.0,
                    null_mut(),
                    rect.left,
                    rect.top,
                    rect_width(&rect),
                    rect_height(&rect),
                    SWP_NOACTIVATE | SWP_NOZORDER,
                );
            }
        }

        let current_dims = Dimensions {
            pixel_width,
            pixel_height,
            dpi: self.get_effective_dpi(),
        };
        self.sync_caption();

        let same = self
            .last_size
            .as_ref()
            .map(|&dims| dims == current_dims)
            .unwrap_or(false);
        self.last_size.replace(current_dims);

        // If a placeholder fade is in progress and the client area's actual
        // pixel size (or DPI) really changed, the overlay's bounds are stale
        // the instant the WebGpu child underneath it resizes. A
        // shifted/mismatched rectangle would look worse than an instant cut,
        // so finish the fade now rather than trying to reposition the
        // overlay. This runs from both `wm_size` and `wm_windowposchanged`
        // (task #399), and `WM_WINDOWPOSCHANGED` also fires for pure moves,
        // z-order changes and activation with no size change at all -- `same`
        // (computed from `GetClientRect`, above) is what distinguishes a
        // real resize from those, so a plain move no longer cuts the fade
        // short.
        if !same && self.placeholder_fade.is_some() {
            self.finish_placeholder_fade();
        }

        if !same {
            self.set_ime_window_position(Rect::default());

            self.events.dispatch(WindowEvent::Resized {
                dimensions: current_dims,
                window_state: get_window_state(self.hwnd.0),
                live_resizing: self.in_size_move,
            });
        }

        !same
    }

    pub(in crate::os::windows::window) fn apply_decoration(&mut self) {
        let hwnd = self.hwnd.0;
        schedule_apply_decoration(hwnd, self.config.window_decorations);
    }
}

pub(in crate::os::windows::window) fn schedule_apply_decoration(
    hwnd: HWND,
    decorations: WindowDecorations,
) {
    onlyterm_promise::spawn::spawn(async move {
        apply_decoration_immediate(hwnd, decorations);
    })
    .detach();
}

fn apply_decoration_immediate(hwnd: HWND, decorations: WindowDecorations) {
    match rc_from_hwnd(hwnd) {
        Some(inner) => {
            let inner = inner.borrow();
            // SAFETY: this live window is owned by the GUI thread; property is scalar.
            unsafe {
                caption::configure(
                    hwnd,
                    inner.config.show_process_tree_stats_in_title
                        && !no_native_title_bar(decorations)
                        && !inner.caption.failed
                        && !inner.webgpu_child_hwnd.0.is_null(),
                );
            }
            if inner.saved_placement.is_some() {
                // We are full screen; ignore it for now
                return;
            }
        }
        None => return,
    };

    // SAFETY: `hwnd` is a valid window handle; the style flags are plain
    // integers and `SetWindowPos` receives valid no-op position/size flags
    // (NOMOVE|NOSIZE) with a null hwndInsertAfter.
    unsafe {
        let orig_style = GetWindowLongW(hwnd, GWL_STYLE);
        let style = decorations_to_style(decorations);
        let new_style = (orig_style & !(WS_OVERLAPPEDWINDOW as i32)) | style as i32;
        SetWindowLongW(hwnd, GWL_STYLE, new_style);
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            0,
            0,
            0,
            0,
            SWP_NOACTIVATE
                | SWP_NOMOVE
                | SWP_NOSIZE
                | SWP_NOZORDER
                | SWP_NOOWNERZORDER
                | SWP_FRAMECHANGED,
        );
        apply_theme(hwnd);
    }
}

pub(in crate::os::windows::window) fn decorations_to_style(decorations: WindowDecorations) -> u32 {
    if decorations == WindowDecorations::RESIZE {
        WS_OVERLAPPEDWINDOW
    } else if decorations == WindowDecorations::TITLE {
        WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_MAXIMIZEBOX
    } else if decorations == WindowDecorations::NONE {
        WS_POPUP
    } else {
        WS_OVERLAPPEDWINDOW
    }
}

/// Returns the theme log font used for the window caption.
///
/// # Safety
/// `hwnd` must be a valid window handle and `hdc` a valid DC (or the calls
/// simply fail and return `None`).
unsafe fn get_title_log_font(hwnd: HWND, hdc: HDC) -> Option<LOGFONTW> {
    let mut log_font = LOGFONTW::default();
    let theme = OpenThemeData(hwnd, wide_string("HEADER").as_ptr());
    if !theme.is_null() {
        let res = GetThemeFont(
            theme,
            hdc,
            extra_constants::HP_HEADERITEM,
            extra_constants::HIS_NORMAL,
            extra_constants::TMT_CAPTIONFONT,
            &mut log_font,
        );
        if res == S_OK {
            CloseThemeData(theme);
            return Some(log_font);
        }
    }

    let res = GetThemeSysFont(theme, extra_constants::TMT_CAPTIONFONT, &mut log_font);
    if !theme.is_null() {
        CloseThemeData(theme);
    }

    if res == S_OK {
        Some(log_font)
    } else {
        None
    }
}

/// # Safety
/// `hwnd` must be a valid window handle.
unsafe fn update_title_font(hwnd: HWND) {
    let hdc = GetDC(hwnd);
    if hdc.is_null() {
        return;
    }

    let mut font = TITLE_FONT.lock().expect("locking title_font");
    if let Some(lf) = get_title_log_font(hwnd, hdc) {
        *font = onlyterm_font::locator::gdi::parse_log_font(&lf, hdc).ok();
    }

    ReleaseDC(hwnd, hdc);
}

/// Set up bidirectional pointers:
/// hwnd.USERDATA -> WindowInner
/// WindowInner.hwnd -> hwnd
///
/// # Safety
/// `hwnd` must be the window being created and `lparam` the `CREATESTRUCTW`
/// from a real `WM_NCCREATE` message whose `lpCreateParams` is an `Rc` raw
/// pointer produced by `rc_to_pointer`.
pub(super) unsafe fn wm_nccreate(
    hwnd: HWND,
    _msg: UINT,
    _wparam: WPARAM,
    lparam: LPARAM,
) -> Option<LRESULT> {
    // SAFETY: `lparam` points at the `CREATESTRUCTW` Win32 passes to WM_NCCREATE.
    let create: &CREATESTRUCTW = &*(lparam as *const CREATESTRUCTW);
    let inner = rc_from_pointer(create.lpCreateParams);
    // SAFETY: valid hwnd; storing the `Rc` raw pointer in GWLP_USERDATA for later
    // recovery (balanced by `wm_ncdestroy`).
    SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as _);
    let mut inner_mut = inner.borrow_mut();
    inner_mut.hwnd = HWindow(hwnd);

    // Start the placeholder spinner's redraw tick now that `hwnd` is valid.
    // This is the earliest point a `SetTimer` call on this window is legal.
    // The timer just invalidates the window on every tick (see `WM_TIMER`
    // in `do_wnd_proc`) to advance the animation; it is killed in
    // `clear_placeholder_background` as soon as a working renderer is
    // installed, so it never outlives the few seconds the spinner is shown
    // for.
    if let Some(spinner) = inner_mut.placeholder_spinner.as_mut() {
        // SAFETY: `hwnd` is the just-created, valid window handle;
        // `PLACEHOLDER_SPINNER_TIMER_ID` is a plain nonzero id scoped to
        // this window.
        SetTimer(
            hwnd,
            PLACEHOLDER_SPINNER_TIMER_ID,
            PLACEHOLDER_SPINNER_INTERVAL_MS,
            None,
        );
        spinner.timer_running = true;
    }

    None
}

/// Called when the window is being destroyed.
/// Goal is to release the WindowInner reference that was stashed
/// in the window by wm_nccreate.
///
/// # Safety
/// `hwnd` must be a valid window handle whose `GWLP_USERDATA` was set by
/// `wm_nccreate` (or is null).
pub(super) unsafe fn wm_ncdestroy(
    hwnd: HWND,
    _msg: UINT,
    _wparam: WPARAM,
    _lparam: LPARAM,
) -> Option<LRESULT> {
    let raw = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as LPVOID;
    if !raw.is_null() {
        let inner = take_rc_from_pointer(raw);
        let mut inner = inner.borrow_mut();
        // Record that this call is the one reclaiming the extra strong ref
        // `new_window` stashed for `wm_nccreate`/`GWLP_USERDATA` (see
        // `WindowInner::extra_ref_reclaimed_by_ncdestroy`'s doc comment).
        // This must happen even on the ordinary successful-creation teardown
        // path (not just the `CreateWindowExW`-failure unwind case), because
        // `new_window`'s error branch can't otherwise distinguish "window
        // was destroyed after being fully created" from "still pending" --
        // it simply never runs in the successful case, so setting this
        // unconditionally here is harmless either way.
        inner.extra_ref_reclaimed_by_ncdestroy.set(true);
        inner.caption = caption::Caption::default();
        // SAFETY: clear this window's scalar property before its HWND is destroyed.
        caption::configure(hwnd, false);
        inner.events.dispatch(WindowEvent::Destroyed);
        inner.hwnd = HWindow(null_mut());
        // Backstop in case this window is closed before a renderer ever
        // came up (so `TermWindow::created` never ran and never called
        // `clear_placeholder_background` itself): make sure the spinner's
        // GDI objects are always deleted rather than leaked. No-op if it
        // was already cleared.
        inner.clear_placeholder_background();
        // By `WM_NCDESTROY` time the parent's children (including any fade
        // overlay) have already been destroyed by Windows itself (WM_DESTROY
        // parent -> WM_DESTROY children -> WM_NCDESTROY children ->
        // WM_NCDESTROY parent), so the overlay HWND is already dead. Just
        // drop our `PlaceholderFade` bookkeeping without calling
        // `ShowWindow`/`DestroyWindow` on a stale handle -- the normal
        // teardown (`finish_placeholder_fade`) is left to the timer / resize
        // / rebuild paths, where the overlay is guaranteed still live. No-op
        // if there was no fade in progress.
        inner.placeholder_fade.take();
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
    }

    None
}

pub(in crate::os::windows::window) fn no_native_title_bar(decorations: WindowDecorations) -> bool {
    decorations == WindowDecorations::RESIZE
        || decorations.contains(WindowDecorations::INTEGRATED_BUTTONS)
}

/// # Safety
/// `hwnd` must be a valid window handle and `wparam`/`lparam` the values from a
/// real `WM_NCCALCSIZE` message (when `wparam==1`, `lparam` points at a valid
/// `NCCALCSIZE_PARAMS`).
pub(super) unsafe fn wm_nccalcsize(
    hwnd: HWND,
    _msg: UINT,
    wparam: WPARAM,
    lparam: LPARAM,
) -> Option<LRESULT> {
    // SAFETY: parameters are the original WM_NCCALCSIZE arguments.
    if let Some(result) = unsafe { caption::nc_calc(hwnd, _msg, wparam, lparam) } {
        return Some(result);
    }
    let inner = rc_from_hwnd(hwnd)?;
    let inner = match inner.try_borrow() {
        Ok(inner) => inner,
        Err(_) => {
            // We've been called recursively and the upper levels
            // own the borrow. Just take the default action
            return None;
        }
    };

    let no_native_title_bar = no_native_title_bar(inner.config.window_decorations);

    if !(wparam == 1 && no_native_title_bar) {
        return None;
    }

    if inner.saved_placement.is_none() {
        let dpi = inner.get_effective_dpi() as u32;
        let frame_x = GetSystemMetricsForDpi(SM_CXFRAME, dpi);
        let frame_y = GetSystemMetricsForDpi(SM_CYFRAME, dpi);
        let padding = GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi);

        let params = (lparam as *mut NCCALCSIZE_PARAMS).as_mut().unwrap();

        let requested_client_rect = &mut params.rgrc[0];

        requested_client_rect.right -= frame_x + padding;
        requested_client_rect.left += frame_x + padding;

        let is_maximized = get_window_state(hwnd) == WindowState::MAXIMIZED;

        // Handle bugged top window border on Windows 10
        if *IS_WIN10 {
            if is_maximized {
                requested_client_rect.top += frame_y + padding;
                requested_client_rect.bottom -= frame_y + padding - 2;
            } else {
                requested_client_rect.top += 1;
                requested_client_rect.bottom -= frame_y - padding;
            }
        } else {
            requested_client_rect.bottom -= frame_y + padding;

            if is_maximized {
                requested_client_rect.top += frame_y + padding;
            }
        }
    }

    Some(0)
}

/// # Safety
/// `hwnd` must be a valid window handle and the args the values from a real
/// `WM_NCHITTEST` message.
pub(super) unsafe fn wm_nchittest(
    hwnd: HWND,
    msg: UINT,
    wparam: WPARAM,
    lparam: LPARAM,
) -> Option<LRESULT> {
    let inner = rc_from_hwnd(hwnd)?;
    let inner = match inner.try_borrow() {
        Ok(inner) => inner,
        Err(_) => {
            // We've been called recursively and the upper levels
            // own the borrow. Just take the default action
            return None;
        }
    };

    let no_native_title_bar = no_native_title_bar(inner.config.window_decorations);
    if !no_native_title_bar {
        return None;
    }

    // Let the default procedure handle resizing areas
    let result = DefWindowProcW(hwnd, msg, wparam, lparam);

    if matches!(
        result,
        HTNOWHERE
            | HTRIGHT
            | HTLEFT
            | HTTOPLEFT
            | HTTOP
            | HTTOPRIGHT
            | HTBOTTOMRIGHT
            | HTBOTTOM
            | HTBOTTOMLEFT
    ) {
        return Some(result);
    }

    // The adjustment in NCCALCSIZE messes with the detection
    // of the top hit area so manually fixing that.
    let dpi = inner.get_effective_dpi() as u32;
    let frame_x = GetSystemMetricsForDpi(SM_CXFRAME, dpi) as isize;
    let frame_y = GetSystemMetricsForDpi(SM_CYFRAME, dpi) as isize;
    let padding = GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi) as isize;

    let coords = mouse_coords(lparam);
    let screen_point = ScreenPoint::new(coords.x, coords.y);
    let cursor_point = screen_to_client(hwnd, screen_point);
    let is_maximized = get_window_state(hwnd) == WindowState::MAXIMIZED;

    // check if mouse is in any of the resize areas (HTTOP, HTBOTTOM, etc)

    let mut client_rect = RECT::default();
    let client_rect_is_valid =
        GetClientRect(hwnd, &mut client_rect) == winapi::shared::minwindef::TRUE;

    // Since we are eating the bottom window frame to deal with a Windows 10 bug,
    // we detect resizing in the window client area as a workaround
    if !is_maximized
        && *IS_WIN10
        && client_rect_is_valid
        && cursor_point.y >= (client_rect.bottom as isize) - (frame_y + padding)
    {
        if cursor_point.x <= (frame_x + padding) {
            return Some(HTBOTTOMLEFT);
        } else if cursor_point.x >= (client_rect.right as isize) - (frame_x + padding) {
            return Some(HTBOTTOMRIGHT);
        } else {
            return Some(HTBOTTOM);
        }
    }

    if !is_maximized && cursor_point.y >= 0 && cursor_point.y < frame_y {
        if cursor_point.x <= (frame_x + padding) {
            return Some(HTTOPLEFT);
        } else if cursor_point.x >= (client_rect.right as isize) - (frame_x + padding) {
            return Some(HTTOPRIGHT);
        } else {
            return Some(HTTOP);
        }
    }

    if let Some(coords) = inner.window_drag_position {
        if coords == screen_point && inner.saved_placement.is_none() {
            return Some(HTCAPTION);
        }
    }

    let use_snap_layouts = !*IS_WIN10;
    if use_snap_layouts {
        if let Some(max) = inner.maximize_button_position {
            if max.contains(screen_point) {
                return Some(HTMAXBUTTON);
            }
        }
    }

    Some(HTCLIENT)
}

pub(in crate::os::windows::window) fn get_window_state(hwnd: HWND) -> WindowState {
    let mut placement = WINDOWPLACEMENT {
        length: std::mem::size_of::<WINDOWPLACEMENT>() as _,
        ..Default::default()
    };

    let placement =
        // SAFETY: `hwnd` is valid and `placement` is a fully-initialized
        // `WINDOWPLACEMENT` that the call only writes to.
        if unsafe { GetWindowPlacement(hwnd, &mut placement) } == winapi::shared::minwindef::TRUE {
            placement.showCmd as i32
        } else {
            0
        };

    match placement {
        SW_SHOWMAXIMIZED => WindowState::MAXIMIZED,
        SW_SHOWMINIMIZED => WindowState::HIDDEN,
        _ => {
            // SAFETY: `hwnd` is valid; `rect`/`mi` are zeroed then sized, and the
            // calls only write into them.
            unsafe {
                let mut rect = std::mem::zeroed();
                GetWindowRect(hwnd, &mut rect);

                let mut mi: MONITORINFO = std::mem::zeroed();
                mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
                GetMonitorInfoW(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST), &mut mi);

                if mi.rcMonitor.left == rect.left
                    && mi.rcMonitor.top == rect.top
                    && mi.rcMonitor.right == rect.right
                    && mi.rcMonitor.bottom == rect.bottom
                {
                    WindowState::FULL_SCREEN
                } else {
                    WindowState::default()
                }
            }
        }
    }
}

/// "Blur behind" is the old vista term for a cool blurring
/// effect that the DWM could enable.  Subsequent windows
/// versions have removed the blurring.  We use this call
/// to tell DWM that we set proper alpha channel info as
/// a result of rendering our window content.
pub(in crate::os::windows::window) fn enable_blur_behind(hwnd: HWND) {
    use winapi::shared::minwindef::*;
    use winapi::um::dwmapi::*;
    use winapi::um::wingdi::*;

    // SAFETY: `hwnd` is valid; the GDI region/handle args are valid and the
    // `DWM_BLURBEHIND` struct is fully initialized.
    unsafe {
        let region = CreateRectRgn(0, 0, -1, -1);

        let bb = DWM_BLURBEHIND {
            dwFlags: DWM_BB_ENABLE | DWM_BB_BLURREGION,
            fEnable: TRUE,
            hRgnBlur: region,
            fTransitionOnMaximized: FALSE,
        };

        DwmEnableBlurBehindWindow(hwnd, &bb);

        DeleteObject(region as _);
    }
}

pub(in crate::os::windows::window) fn apply_theme(hwnd: HWND) -> Option<LRESULT> {
    // Check for OS app theme, and set window attributes accordingly.
    // Note that the MS terminal app uses the logic found here for this stuff:
    // https://github.com/microsoft/terminal/blob/9b92986b49bed8cc41fde4d6ef080921c41e6d9e/src/interactivity/win32/windowtheme.cpp#L62
    use winapi::um::dwmapi::{DwmExtendFrameIntoClientArea, DwmSetWindowAttribute};
    use winapi::um::uxtheme::MARGINS;

    // Name mirrors the Win32 `WINDOWCOMPOSITIONATTRIB` type; keeping it identical
    // to the documented API is preferable to the acronym-style rename clippy wants.
    #[allow(non_snake_case, clippy::upper_case_acronyms)]
    type WINDOWCOMPOSITIONATTRIB = u32;
    const WCA_USEDARKMODECOLORS: WINDOWCOMPOSITIONATTRIB = 26;

    // Name mirrors the Win32 `WINDOWCOMPOSITIONATTRIBDATA` struct used by
    // `SetWindowCompositionAttribute`; kept identical to the documented API.
    #[allow(non_snake_case, clippy::upper_case_acronyms)]
    #[repr(C)]
    pub struct WINDOWCOMPOSITIONATTRIBDATA {
        Attrib: WINDOWCOMPOSITIONATTRIB,
        pvData: PVOID,
        cbData: winapi::shared::basetsd::SIZE_T,
    }

    shared_library!(User32,
        pub fn SetWindowCompositionAttribute(hwnd: HWND, attrib: *mut WINDOWCOMPOSITIONATTRIBDATA) -> BOOL,
    );

    const DWMWA_USE_IMMERSIVE_DARK_MODE: DWORD = 20;
    const DWMWA_MICA_EFFECT: DWORD = 1029;
    const DWMWA_SYSTEMBACKDROP_TYPE: DWORD = 38;

    #[allow(non_camel_case_types)]
    #[allow(dead_code)]
    #[derive(PartialEq, Eq)]
    #[repr(C)]
    enum ACCENT_STATE {
        ACCENT_DISABLED = 0,
        ACCENT_ENABLE_BLURBEHIND = 3,
        ACCENT_ENABLE_ACRYLICBLURBEHIND = 4,
    }

    #[allow(non_snake_case)]
    #[repr(C)]
    struct ACCENT_POLICY {
        AccentState: u32,
        AccentFlags: u32,
        GradientColour: u32,
        AnimationId: u32,
    }

    #[allow(non_camel_case_types)]
    #[allow(dead_code)]
    #[repr(C)]
    enum DWM_SYSTEMBACKDROP_TYPE {
        DWMSBT_AUTO = 0,
        DWMSBT_NONE = 1,
        DWMSBT_MAINWINDOW = 2,      // Mica
        DWMSBT_TRANSIENTWINDOW = 3, // Acrylic
        DWMSBT_TABBEDWINDOW = 4,    // Tabbed
    }

    // SAFETY: `hwnd` is a valid window handle; every FFI call receives either
    // that handle or a pointer to a fully-initialized stack struct of the
    // correct `repr(C)` layout with matching `cbData`/size.
    unsafe {
        update_title_font(hwnd);

        let appearance = get_appearance();
        let theme_string = if appearance == Appearance::Dark {
            "DarkMode_Explorer"
        } else {
            ""
        };

        SetWindowTheme(
            hwnd as _,
            wide_string(theme_string).as_slice().as_ptr(),
            std::ptr::null_mut(),
        );

        let mut enabled: BOOL = if appearance == Appearance::Dark { 1 } else { 0 };
        DwmSetWindowAttribute(
            hwnd as _,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &enabled as *const _ as *const _,
            std::mem::size_of_val(&enabled) as u32,
        );

        if let Ok(user) = User32::open(std::path::Path::new("user32.dll")) {
            (user.SetWindowCompositionAttribute)(
                hwnd,
                &mut WINDOWCOMPOSITIONATTRIBDATA {
                    Attrib: WCA_USEDARKMODECOLORS,
                    pvData: &mut enabled as *mut _ as _,
                    cbData: std::mem::size_of_val(&enabled) as _,
                },
            );
        };

        if let Some(inner) = rc_from_hwnd(hwnd) {
            let mut inner = inner.borrow_mut();

            // Set Acrylic or Mica system Backdrop
            let pv_attribute = match inner.config.win32_system_backdrop {
                SystemBackdrop::Auto => DWM_SYSTEMBACKDROP_TYPE::DWMSBT_AUTO,
                SystemBackdrop::Disable => DWM_SYSTEMBACKDROP_TYPE::DWMSBT_NONE,
                SystemBackdrop::Acrylic => DWM_SYSTEMBACKDROP_TYPE::DWMSBT_TRANSIENTWINDOW,
                SystemBackdrop::Mica => DWM_SYSTEMBACKDROP_TYPE::DWMSBT_MAINWINDOW,
                SystemBackdrop::Tabbed => DWM_SYSTEMBACKDROP_TYPE::DWMSBT_TABBEDWINDOW,
            };

            let margins = match inner.config.window_decorations {
                WindowDecorations::TITLE => -1,
                _ => 0,
            };

            DwmExtendFrameIntoClientArea(
                hwnd,
                &MARGINS {
                    cxLeftWidth: margins,
                    cxRightWidth: margins,
                    cyTopHeight: if margins < 0 {
                        margins
                    } else {
                        caption::height(hwnd)
                    },
                    cyBottomHeight: margins,
                },
            );

            // Apply Acrylic or Mica Backdrop
            if *IS_WIN11_22H2 {
                DwmSetWindowAttribute(
                    hwnd,
                    DWMWA_SYSTEMBACKDROP_TYPE,
                    &pv_attribute as *const _ as _,
                    std::mem::size_of_val(&pv_attribute) as u32,
                );
            } else {
                let mut colour = inner.config.win32_acrylic_accent_color.to_srgb_u8();
                colour.3 = if colour.3 == 0 { 1 } else { colour.3 }; // acrylic doesn't like to have 0 alpha

                let mut policy = ACCENT_POLICY {
                    AccentState: if inner.config.win32_system_backdrop == SystemBackdrop::Acrylic {
                        ACCENT_STATE::ACCENT_ENABLE_ACRYLICBLURBEHIND as _
                    } else {
                        ACCENT_STATE::ACCENT_DISABLED as _
                    },
                    AccentFlags: if inner.config.win32_system_backdrop == SystemBackdrop::Acrylic {
                        2
                    } else {
                        0
                    },
                    GradientColour: (colour.0 as u32)
                        | (colour.1 as u32) << 8
                        | (colour.2 as u32) << 16
                        | (colour.3 as u32) << 24,
                    AnimationId: 0,
                };

                if let Ok(user) = User32::open(std::path::Path::new("user32.dll")) {
                    (user.SetWindowCompositionAttribute)(
                        hwnd,
                        &mut WINDOWCOMPOSITIONATTRIBDATA {
                            Attrib: 0x13,
                            pvData: &mut policy as *mut _ as _,
                            cbData: std::mem::size_of_val(&policy) as _,
                        },
                    );
                }

                if !*IS_WIN10 && !*IS_WIN11_22H2 {
                    // For build versions less than 22h2 but are still win11
                    let mica_enabled: u32 =
                        if inner.config.win32_system_backdrop == SystemBackdrop::Mica {
                            1
                        } else {
                            0
                        };
                    DwmSetWindowAttribute(
                        hwnd,
                        DWMWA_MICA_EFFECT,
                        &mica_enabled as *const _ as _,
                        std::mem::size_of_val(&mica_enabled) as u32,
                    );
                }
            }

            if appearance != inner.appearance {
                inner.appearance = appearance;
                inner.caption.set_appearance(appearance);
                inner
                    .events
                    .dispatch(WindowEvent::AppearanceChanged(appearance));
            }
        }
    }

    None
}

/// # Safety
/// `hwnd` must be a valid window handle and `msg`/args from a real
/// `WM_ENTER/EXITSIZEMOVE` message.
pub(super) unsafe fn wm_enter_exit_size_move(
    hwnd: HWND,
    msg: UINT,
    _wparam: WPARAM,
    _lparam: LPARAM,
) -> Option<LRESULT> {
    let mut should_size = false;
    if let Some(inner) = rc_from_hwnd(hwnd) {
        // This can be called re-entrantly: Windows may synchronously
        // dispatch a nested message (eg. the IME/TSF subsystem showing
        // a candidate/completion popup) while we're already holding a
        // mutable borrow of `inner` higher up the call stack.
        // Use `try_borrow_mut` and simply skip updating the state for
        // this particular re-entrant invocation rather than panicking;
        // the next, non-reentrant, invocation will observe and set the
        // correct state. See: <https://github.com/wezterm/wezterm/issues/7358>
        if let Ok(mut inner) = inner.try_borrow_mut() {
            inner.in_size_move = msg == WM_ENTERSIZEMOVE;
            should_size = !inner.in_size_move;
        }
    }

    if should_size {
        wm_size(hwnd, 0, 0, 0)?;
    }

    Some(0)
}

/// We handle WM_WINDOWPOSCHANGED and dispatch directly to our wm_size as it
/// is a bit more efficient than letting DefWindowProcW parse this and
/// trigger WM_SIZE.
///
/// # Safety
/// `hwnd` must be a valid window handle.
pub(super) unsafe fn wm_windowposchanged(
    hwnd: HWND,
    _msg: UINT,
    _wparam: WPARAM,
    _lparam: LPARAM,
) -> Option<LRESULT> {
    // let pos = &*(lparam as *const WINDOWPOS);
    wm_size(hwnd, 0, 0, 0)?;
    Some(0)
}

/// # Safety
/// `hwnd` must be a valid window handle.
pub(super) unsafe fn wm_size(
    hwnd: HWND,
    _msg: UINT,
    _wparam: WPARAM,
    _lparam: LPARAM,
) -> Option<LRESULT> {
    let mut should_paint = false;
    let mut should_pump = false;

    if let Some(inner) = rc_from_hwnd(hwnd) {
        let mut inner = inner.borrow_mut();
        should_paint = inner.check_and_call_resize_if_needed();
        should_pump = inner.in_size_move;
    }

    if should_paint {
        wm_paint(hwnd, 0, 0, 0)?;
        if should_pump {
            crate::spawn::SPAWN_QUEUE.run();
        }
    }

    None
}

/// # Safety
/// `hwnd` must be a valid window handle.
pub(super) unsafe fn wm_set_focus(
    hwnd: HWND,
    _msg: UINT,
    _wparam: WPARAM,
    _lparam: LPARAM,
) -> Option<LRESULT> {
    rc_from_hwnd(hwnd)?
        .borrow_mut()
        .events
        .dispatch(WindowEvent::FocusChanged(true));
    None
}

/// # Safety
/// `hwnd` must be a valid window handle.
pub(super) unsafe fn wm_kill_focus(
    hwnd: HWND,
    _msg: UINT,
    _wparam: WPARAM,
    _lparam: LPARAM,
) -> Option<LRESULT> {
    rc_from_hwnd(hwnd)?
        .borrow_mut()
        .events
        .dispatch(WindowEvent::FocusChanged(false));
    None
}
