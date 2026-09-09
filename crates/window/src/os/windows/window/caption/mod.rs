//! A client-painted caption band; Windows retains its caption buttons and frame.
use super::{rc_from_hwnd, wide_string, Appearance, WindowInner};
use std::cell::RefCell;
use std::collections::HashMap;
use std::convert::TryInto;
use std::io;
use std::ptr::{null, null_mut};
use std::rc::Rc;
use winapi::shared::minwindef::*;
use winapi::shared::windef::*;
use winapi::um::dwmapi::{
    DwmDefWindowProc, DwmGetWindowAttribute, DwmIsCompositionEnabled, DWMWA_CAPTION_BUTTON_BOUNDS,
};
use winapi::um::libloaderapi::GetModuleHandleW;
use winapi::um::winuser::*;

mod layout;
mod paint;
mod surface;

thread_local! {
    static PAINT_STATES: RefCell<HashMap<usize, Rc<RefCell<paint::CaptionPaint>>>> = RefCell::new(HashMap::new());
}

// Window properties contain opaque integer flags, never Rust pointers.
const ENABLED: &[u8] = b"OnlyTerm.CenteredCaption\0";
const HEIGHT: &[u8] = b"OnlyTerm.CaptionHeight\0";
const LABEL_WIDTH: &[u8] = b"OnlyTerm.CaptionLabelWidth\0";
const CLIP_CHILDREN_ADDED: &[u8] = b"OnlyTerm.CaptionClipsChildren\0";

/// # Safety
/// hwnd must be live on its owning thread; arguments are the original window message.
pub(super) unsafe fn dwm_message(
    hwnd: HWND,
    msg: UINT,
    wparam: WPARAM,
    lparam: LPARAM,
) -> Option<LRESULT> {
    // SAFETY: property is scalar and result is an initialized out parameter.
    unsafe {
        if GetPropA(hwnd, ENABLED.as_ptr().cast()).is_null() {
            return None;
        }
        let mut result = 0;
        let handled = DwmDefWindowProc(hwnd, msg, wparam, lparam, &mut result) != FALSE;
        if matches!(msg, WM_NCCALCSIZE | WM_NCACTIVATE | WM_NCPAINT) {
            log::debug!(
                "caption DWM message=0x{:x} hit={} handled={} result={}",
                msg,
                wparam,
                handled,
                result
            );
        }
        if matches!(msg, WM_NCMOUSEMOVE | WM_NCMOUSEHOVER | WM_NCMOUSELEAVE) {
            log::trace!(
                "caption mouse message=0x{:x} hit={} dwm={} result={}",
                msg,
                wparam,
                handled,
                result
            );
        }
        handled.then_some(result)
    }
}

/// # Safety
/// hwnd must be a live same-thread top-level HWND.
pub(super) unsafe fn log_geometry(hwnd: HWND) {
    if !log::log_enabled!(log::Level::Debug) {
        return;
    }
    // SAFETY: all structs are initialized out parameters for the live HWND.
    unsafe {
        let mut outer = RECT::default();
        let mut visible = RECT::default();
        let mut client = RECT::default();
        let mut origin = POINT { x: 0, y: 0 };
        let mut enabled: BOOL = FALSE;
        GetWindowRect(hwnd, &mut outer);
        GetClientRect(hwnd, &mut client);
        ClientToScreen(hwnd, &mut origin);
        let hr = DwmGetWindowAttribute(hwnd, 9, &mut visible as *mut _ as _, 16);
        DwmGetWindowAttribute(hwnd, 1, &mut enabled as *mut _ as _, 4);
        log::debug!("caption geometry hwnd={:?} outer={},{},{},{} visible={},{},{},{} hr={} client_origin={},{} client_size={},{} band={} nc_enabled={} style={:x}",
            hwnd,outer.left,outer.top,outer.right,outer.bottom,visible.left,visible.top,visible.right,visible.bottom,
            hr,origin.x,origin.y,client.right,client.bottom,height(hwnd),enabled,GetWindowLongW(hwnd,GWL_STYLE));
    }
}

#[derive(Default)]
pub(super) struct Caption {
    pub(super) failed: bool,
    // Non-owning: a WS_CHILD, destroyed by Windows with its parent.
    child: HWND,
    title: String,
    status: Option<(String, String)>,
    paint: Rc<RefCell<paint::CaptionPaint>>,
    button_reserve: Option<(u32, i32)>,
    geometry: Option<CaptionGeometry>,
}

#[derive(PartialEq, Eq)]
struct CaptionGeometry {
    window_width: i32,
    client_left: i32,
    width: i32,
    height: i32,
    dpi: u32,
    maximized: bool,
}

impl Caption {
    pub(super) fn reset_font(&mut self) {
        self.paint.borrow_mut().font = None;
        self.geometry = None;
        self.invalidate();
    }
    pub(super) fn set_text(&mut self, title: &str, status: Option<(&str, &str)>) -> bool {
        if self.title == title
            && self.status.as_ref().map(|(a, b)| (a.as_str(), b.as_str())) == status
        {
            return false;
        }
        self.title = title.to_owned();
        self.status = status.map(|(a, b)| (a.to_owned(), b.to_owned()));
        let title_wide = paint::caption_text(title);
        let status_wide = status
            .map(|(full, compact)| {
                [full, compact]
                    .iter()
                    .map(|text| format!("[{}]", text).encode_utf16().collect())
                    .collect()
            })
            .unwrap_or_default();
        {
            let mut paint = self.paint.borrow_mut();
            paint.title_wide = title_wide;
            paint.status_wide = status_wide;
        }
        self.invalidate();
        true
    }

    pub(super) fn invalidate(&self) {
        if !self.child.is_null() {
            // SAFETY: child is live until the parent's WM_NCDESTROY resets Caption.
            unsafe {
                InvalidateRect(self.child, null(), FALSE);
            }
        }
    }

    pub(super) fn set_appearance(&self, appearance: Appearance) {
        self.paint.borrow_mut().appearance = appearance;
    }
}

/// # Safety
/// hwnd must be a live OnlyTerm top-level window on its owning thread.
pub(super) unsafe fn configure(hwnd: HWND, enabled: bool) {
    let mut composition = FALSE;
    // SAFETY: writable scalar; DWM owns no caller resources for this query.
    let enabled = enabled
        && unsafe { DwmIsCompositionEnabled(&mut composition) } >= 0
        && composition != FALSE;
    if enabled {
        // SAFETY: same-thread style change; remember whether this caption owns the bit.
        if !unsafe { set_child_clipping(hwnd, true) } {
            log::warn!("Could not protect the caption child from parent painting");
            // SAFETY: roll back caption properties on the same live window.
            unsafe {
                configure(hwnd, false);
            }
            return;
        }
        // SAFETY: static NUL-terminated property name; value 1 is an opaque flag.
        if unsafe { SetPropA(hwnd, ENABLED.as_ptr().cast(), 1usize as _) } == FALSE {
            log::warn!(
                "Could not enable centered caption: {}",
                io::Error::last_os_error()
            );
            // SAFETY: failed enable must not leave caption-owned style changes behind.
            unsafe {
                configure(hwnd, false);
            }
        }
    } else {
        // SAFETY: property has no allocation or handle to release.
        unsafe {
            RemovePropA(hwnd, ENABLED.as_ptr().cast());
            RemovePropA(hwnd, HEIGHT.as_ptr().cast());
            RemovePropA(hwnd, LABEL_WIDTH.as_ptr().cast());
            set_child_clipping(hwnd, false);
        }
    }
}

/// # Safety
/// hwnd must be a live same-thread window whose style is owned by this application.
unsafe fn set_child_clipping(hwnd: HWND, enabled: bool) -> bool {
    // SAFETY: live HWND and scalar style/property values; no Rust pointers are stored.
    unsafe {
        let style = GetWindowLongW(hwnd, GWL_STYLE);
        if enabled && style as u32 & WS_CLIPCHILDREN == 0 {
            if SetPropA(hwnd, CLIP_CHILDREN_ADDED.as_ptr().cast(), 1usize as _) == FALSE {
                return false;
            }
            if SetWindowLongW(hwnd, GWL_STYLE, style | WS_CLIPCHILDREN as i32) == 0 {
                RemovePropA(hwnd, CLIP_CHILDREN_ADDED.as_ptr().cast());
                return false;
            }
        } else if !enabled && !GetPropA(hwnd, CLIP_CHILDREN_ADDED.as_ptr().cast()).is_null() {
            if SetWindowLongW(hwnd, GWL_STYLE, style & !(WS_CLIPCHILDREN as i32)) == 0 {
                return false;
            }
            RemovePropA(hwnd, CLIP_CHILDREN_ADDED.as_ptr().cast());
        }
        true
    }
}

/// # Safety
/// hwnd must be live. This does not borrow WindowInner and tolerates reentrancy.
pub(super) unsafe fn height(hwnd: HWND) -> i32 {
    // SAFETY: valid window and static property name; style and metrics are scalar queries.
    unsafe {
        if GetPropA(hwnd, ENABLED.as_ptr().cast()).is_null()
            || GetWindowLongW(hwnd, GWL_STYLE) as u32 & WS_CAPTION == 0
        {
            return 0;
        }
        (GetPropA(hwnd, HEIGHT.as_ptr().cast()) as isize).clamp(0, i32::MAX as isize) as i32
    }
}

/// # Safety
/// hwnd and rect must refer to a live window and its initialized client RECT.
pub(super) unsafe fn content_rect(hwnd: HWND, rect: &mut RECT) {
    // SAFETY: same live window as the caller's client rectangle.
    rect.top = (rect.top + unsafe { height(hwnd) }).min(rect.bottom);
}

/// # Safety
/// hwnd and dc must be live on the owning thread, in raw parent-client coordinates.
pub(super) unsafe fn clear_frame_background(hwnd: HWND, dc: HDC) {
    // SAFETY: black GDI pixels are transparent premultiplied pixels on the extended DWM frame.
    // The opaque child paints the labels; leave DWM's button area uncovered.
    unsafe {
        let h = height(hwnd);
        if h > 0 && !dc.is_null() {
            let mut rect = RECT::default();
            GetClientRect(hwnd, &mut rect);
            rect.left = (GetPropA(hwnd, LABEL_WIDTH.as_ptr().cast()) as usize as i32)
                .clamp(0, rect.right.max(0));
            rect.bottom = h.min(rect.bottom);
            FillRect(
                dc,
                &rect,
                winapi::um::wingdi::GetStockObject(winapi::um::wingdi::BLACK_BRUSH as i32) as _,
            );
        }
    }
}

/// # Safety
/// hwnd must be a live GUI window; invalidate only its terminal content.
pub(super) unsafe fn invalidate_content(hwnd: HWND) {
    // SAFETY: initialized output rectangle and same-thread HWND; no borrowed Rust state.
    unsafe {
        let mut rect = RECT::default();
        if GetClientRect(hwnd, &mut rect) != FALSE {
            content_rect(hwnd, &mut rect);
            InvalidateRect(hwnd, &rect, FALSE);
        }
    }
}

/// # Safety
/// Arguments must come from hwnd's real WM_NCCALCSIZE message.
pub(super) unsafe fn nc_calc(
    hwnd: HWND,
    _msg: UINT,
    wparam: WPARAM,
    lparam: LPARAM,
) -> Option<LRESULT> {
    // Only override the real resize calculation; FALSE is also used by native frame queries.
    if wparam == 0 {
        return None;
    }
    // SAFETY: live message window; no WindowInner borrow is needed here.
    let enabled = unsafe {
        !GetPropA(hwnd, ENABLED.as_ptr().cast()).is_null()
            && GetWindowLongW(hwnd, GWL_STYLE) as u32 & WS_CAPTION != 0
    };
    if !enabled {
        return None;
    }
    // SAFETY: WM_NCCALCSIZE supplies exactly these aligned, writable structures.
    let rect = unsafe { &mut (*(lparam as *mut NCCALCSIZE_PARAMS)).rgrc[0] };
    // SAFETY: scalar metrics on the live message window; unsupported DWM attribute has a fallback.
    unsafe {
        let dpi = GetDpiForWindow(hwnd).max(96);
        let mut native = RECT::default();
        if AdjustWindowRectExForDpi(
            &mut native,
            GetWindowLongW(hwnd, GWL_STYLE) as u32,
            (!GetMenu(hwnd).is_null()) as BOOL,
            GetWindowLongW(hwnd, GWL_EXSTYLE) as u32,
            dpi,
        ) == FALSE
        {
            configure(hwnd, false);
            return None;
        }
        let (top, band, _) = layout::caption_insets(
            -native.top,
            GetSystemMetricsForDpi(SM_CYCAPTION, dpi),
            IsZoomed(hwnd) != FALSE,
        );
        if SetPropA(hwnd, HEIGHT.as_ptr().cast(), band as usize as _) == FALSE {
            configure(hwnd, false);
            return None;
        }
        rect.left -= native.left;
        rect.right -= native.right;
        rect.bottom -= native.bottom;
        rect.top += top;
        log::debug!(
            "caption NCCALC native_top={} keep_top={} band={} result={},{},{},{}",
            -native.top,
            top,
            band,
            rect.left,
            rect.top,
            rect.right,
            rect.bottom
        );
    }
    Some(0)
}

/// # Safety
/// Arguments must be an unmodified message delivered to this live hwnd.
pub(super) unsafe fn non_client_message(
    hwnd: HWND,
    msg: UINT,
    wparam: WPARAM,
    lparam: LPARAM,
) -> Option<LRESULT> {
    // SAFETY: valid message window and parameters.
    unsafe {
        let h = height(hwnd);
        if h == 0 {
            return None;
        }
        if msg != WM_NCHITTEST {
            return Some(DefWindowProcW(hwnd, msg, wparam, lparam));
        }
        let default = DefWindowProcW(hwnd, msg, wparam, lparam);
        if matches!(
            default,
            HTMINBUTTON
                | HTMAXBUTTON
                | HTCLOSE
                | HTHELP
                | HTSYSMENU
                | HTTOP
                | HTTOPLEFT
                | HTTOPRIGHT
                | HTLEFT
                | HTRIGHT
                | HTBOTTOM
                | HTBOTTOMLEFT
                | HTBOTTOMRIGHT
        ) {
            return Some(default);
        }
        let mut point = POINT {
            x: lparam as i16 as i32,
            y: (lparam >> 16) as i16 as i32,
        };
        ScreenToClient(hwnd, &mut point);
        if IsZoomed(hwnd) == FALSE && GetWindowLongW(hwnd, GWL_STYLE) as u32 & WS_THICKFRAME != 0 {
            let dpi = GetDpiForWindow(hwnd).max(96);
            let edge = GetSystemMetricsForDpi(SM_CYSIZEFRAME, dpi);
            if point.y >= 0 && point.y < edge {
                let mut client = RECT::default();
                GetClientRect(hwnd, &mut client);
                return Some(if point.x < edge {
                    HTTOPLEFT
                } else if point.x >= client.right - edge {
                    HTTOPRIGHT
                } else {
                    HTTOP
                });
            }
        }
        if point.y >= 0 && point.y < h {
            let screen = POINT {
                x: lparam as i16 as i32,
                y: (lparam >> 16) as i16 as i32,
            };
            if let Some(button) = caption_button_hit(hwnd, screen, h) {
                return Some(button);
            }
            let icon_end = GetSystemMetricsForDpi(SM_CXSMICON, GetDpiForWindow(hwnd).max(96)) + 8;
            return Some(if point.x >= 0 && point.x < icon_end {
                HTSYSMENU
            } else {
                HTCAPTION
            });
        }
        Some(default)
    }
}

/// # Safety
/// hwnd is live on this thread, point is in screen coordinates, h is its caption band height.
unsafe fn caption_button_hit(hwnd: HWND, point: POINT, h: i32) -> Option<LRESULT> {
    // SAFETY: documented fixed-size output structure for WM_GETTITLEBARINFOEX.
    let mut info: TITLEBARINFOEX = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<TITLEBARINFOEX>() as DWORD;
    // SAFETY: same-thread synchronous query, no WindowInner borrow is held across reentry.
    unsafe {
        SendMessageW(hwnd, WM_GETTITLEBARINFOEX, 0, &mut info as *mut _ as LPARAM);
        let mut origin = POINT { x: 0, y: 0 };
        let mut client = RECT::default();
        ClientToScreen(hwnd, &mut origin);
        GetClientRect(hwnd, &mut client);
        for (index, hit) in [
            (2, HTMINBUTTON),
            (3, HTMAXBUTTON),
            (4, HTHELP),
            (5, HTCLOSE),
        ] {
            let rect = info.rgrect[index];
            if info.rgstate[index]
                & (STATE_SYSTEM_INVISIBLE | STATE_SYSTEM_OFFSCREEN | STATE_SYSTEM_UNAVAILABLE)
                == 0
                && rect.bottom > rect.top
                && layout::button_contains(
                    rect.left..rect.right,
                    origin.x..origin.x + client.right,
                    origin.y..origin.y + h,
                    (point.x, point.y),
                )
            {
                return Some(hit);
            }
        }
    }
    None
}

impl WindowInner {
    pub(super) fn caption_failed(&mut self) {
        if !self.caption.failed {
            self.caption.failed = true;
            log::warn!("Caption drawing unavailable; restoring the native title bar");
            super::schedule_apply_decoration(self.hwnd.0, self.config.window_decorations);
        }
    }
    pub(super) fn sync_caption(&mut self) {
        let hwnd = self.hwnd.0;
        self.caption.set_appearance(self.appearance);
        // SAFETY: called only while WindowInner owns a live parent HWND.
        let h = unsafe { height(hwnd) };
        // SAFETY: same live HWND; minimized child geometry is not a drawable layout.
        if h == 0 || unsafe { IsIconic(hwnd) } != FALSE {
            self.caption.geometry = None;
            if !self.caption.child.is_null() {
                // SAFETY: child is owned by this live parent; hiding does not destroy it.
                unsafe {
                    ShowWindow(self.caption.child, SW_HIDE);
                }
            }
            return;
        }
        if self.caption.child.is_null() {
            // SAFETY: create a same-thread child of this live parent.
            match unsafe { create_child(hwnd, Rc::clone(&self.caption.paint)) } {
                Ok(child) => self.caption.child = child,
                Err(err) => {
                    log::warn!("Could not create caption: {:#}", err);
                    self.caption_failed();
                    return;
                }
            }
        }
        // SAFETY: both HWNDs are live and belong to this GUI thread; all RECTs are writable.
        unsafe {
            let mut client = RECT::default();
            GetClientRect(hwnd, &mut client);
            let mut outer = RECT::default();
            GetWindowRect(hwnd, &mut outer);
            let mut origin = POINT { x: 0, y: 0 };
            ClientToScreen(hwnd, &mut origin);
            let left = origin.x - outer.left;
            let dpi = GetDpiForWindow(hwnd).max(96);
            let mut buttons = RECT::default();
            let hr = DwmGetWindowAttribute(
                hwnd,
                DWMWA_CAPTION_BUTTON_BOUNDS,
                &mut buttons as *mut _ as _,
                std::mem::size_of::<RECT>() as u32,
            );
            let native_reserve = (hr >= 0)
                .then(|| {
                    layout::native_button_reserve(
                        outer.right - outer.left,
                        left,
                        client.right,
                        GetSystemMetricsForDpi(SM_CXSIZEFRAME, dpi)
                            + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi),
                        buttons.left,
                        buttons.right,
                    )
                })
                .flatten();
            let right = layout::caption_width(
                client.right,
                dpi,
                native_reserve,
                &mut self.caption.button_reserve,
            );
            let geometry = CaptionGeometry {
                window_width: outer.right - outer.left,
                client_left: left,
                width: right,
                height: h,
                dpi,
                maximized: IsZoomed(hwnd) != FALSE,
            };
            let repaint = self.caption.geometry.as_ref() != Some(&geometry);
            self.caption.geometry = Some(geometry);
            // Publish ownership before SetWindowPos can synchronously repaint the parent.
            if SetPropA(hwnd, LABEL_WIDTH.as_ptr().cast(), right as usize as _) == FALSE {
                self.caption_failed();
                return;
            }
            let mut current = RECT::default();
            GetClientRect(self.caption.child, &mut current);
            let mut placed = RECT::default();
            GetWindowRect(self.caption.child, &mut placed);
            let mut child_origin = POINT {
                x: placed.left,
                y: placed.top,
            };
            ScreenToClient(hwnd, &mut child_origin);
            let resized = current.right != right
                || current.bottom != h
                || child_origin.x != 0
                || child_origin.y != 0;
            let hidden = IsWindowVisible(self.caption.child) == FALSE;
            if (resized || hidden)
                && SetWindowPos(
                    self.caption.child,
                    null_mut(),
                    0,
                    0,
                    right,
                    h,
                    SWP_NOACTIVATE | SWP_NOZORDER | SWP_SHOWWINDOW,
                ) == FALSE
            {
                self.caption_failed();
                return;
            }
            if repaint || resized || hidden {
                self.caption.invalidate();
            }
        }
        // SAFETY: same live parent, after synchronizing both child rectangles.
        unsafe {
            log_geometry(hwnd);
        }
    }
}

/// # Safety
/// parent must be a live same-thread HWND; Windows owns the returned WS_CHILD.
unsafe fn create_child(
    parent: HWND,
    state: Rc<RefCell<paint::CaptionPaint>>,
) -> anyhow::Result<HWND> {
    let name = wide_string("OnlyTermCaptionBand");
    // SAFETY: local WNDCLASS and NUL-terminated name live throughout both calls.
    unsafe {
        let instance = GetModuleHandleW(null());
        let class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(child_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: instance,
            hIcon: null_mut(),
            hCursor: null_mut(),
            hbrBackground: null_mut(),
            lpszMenuName: null(),
            lpszClassName: name.as_ptr(),
        };
        if RegisterClassW(&class) == 0 {
            let err = io::Error::last_os_error();
            if err.raw_os_error()
                != Some(winapi::shared::winerror::ERROR_CLASS_ALREADY_EXISTS as i32)
            {
                return Err(err.into());
            }
        }
        let child = CreateWindowExW(
            WS_EX_NOACTIVATE,
            name.as_ptr(),
            name.as_ptr(),
            WS_CHILD,
            0,
            0,
            0,
            0,
            parent,
            null_mut(),
            instance,
            null_mut(),
        );
        if child.is_null() {
            return Err(io::Error::last_os_error().into());
        }
        PAINT_STATES.with(|states| states.borrow_mut().insert(child as usize, state));
        Ok(child)
    }
}

/// # Safety
/// Windows supplies the HWND and original message arguments. No Rust data crosses ABI.
unsafe extern "system" fn child_proc(
    hwnd: HWND,
    msg: UINT,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let outcome = std::panic::catch_unwind(|| {
        match msg {
            WM_NCHITTEST => HTTRANSPARENT,
            WM_ERASEBKGND => 1,
            WM_PAINT => {
                // SAFETY: real WM_PAINT, with its paint DC released on every path.
                unsafe {
                    paint::paint_child(hwnd);
                }
                0
            }
            WM_PRINTCLIENT => {
                // SAFETY: WM_PRINTCLIENT supplies a live caller-owned HDC in wparam.
                unsafe {
                    paint::paint_in_dc(hwnd, wparam as HDC);
                }
                0
            }
            WM_NCDESTROY => {
                PAINT_STATES.with(|states| states.borrow_mut().remove(&(hwnd as usize)));
                // SAFETY: unchanged destruction message after releasing child-owned state.
                unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
            }
            // SAFETY: unchanged window message; no borrowed application state.
            _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
        }
    });
    match outcome {
        Ok(result) => result,
        Err(payload) => {
            // Unknown panic payloads may themselves panic on drop across the ABI.
            if !payload.is::<String>() && !payload.is::<&str>() {
                std::mem::forget(payload);
            }
            0
        }
    }
}
