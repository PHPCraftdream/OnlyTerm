use super::*;
use crate::connection::ConnectionOps;
use crate::parameters::{self, Parameters};
use crate::{
    Appearance, Clipboard, DeadKeyStatus, Dimensions, Handled, KeyCode, KeyEvent, Modifiers,
    MouseButtons, MouseCursor, MouseEvent, MouseEventKind, MousePress, Point, RawKeyEvent, Rect,
    RequestedWindowGeometry, ResolvedGeometry, ScreenPoint, ScreenRect, ULength, WindowDecorations,
    WindowEvent, WindowEventSender, WindowOps, WindowState,
};
use anyhow::{bail, Context};
use async_trait::async_trait;
use config::{ConfigHandle, ImePreeditRendering, SystemBackdrop};
use lazy_static::lazy_static;
use promise::Future;
use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
    RawWindowHandle, Win32WindowHandle, WindowHandle, WindowsDisplayHandle,
};
use shared_library::shared_library;
use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::convert::TryInto;
use std::ffi::OsString;
use std::io::{self, Error as IoError};
use std::num::NonZeroIsize;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;
use std::ptr::{null, null_mut};
use std::rc::Rc;
use std::sync::Mutex;
use wezterm_color_types::LinearRgba;
use wezterm_font::FontConfiguration;
use wezterm_input_types::KeyboardLedStatus;
use winapi::shared::minwindef::*;
use winapi::shared::ntdef::*;
use winapi::shared::windef::*;
use winapi::shared::winerror::S_OK;
use winapi::um::imm::*;
use winapi::um::libloaderapi::GetModuleHandleW;
use winapi::um::shellapi::{DragAcceptFiles, DragFinish, DragQueryFileW, HDROP};
use winapi::um::shellscalingapi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use winapi::um::sysinfoapi::{GetTickCount, GetVersionExW};
use winapi::um::uxtheme::{
    CloseThemeData, GetThemeFont, GetThemeSysFont, OpenThemeData, SetWindowTheme,
};
use winapi::um::wingdi::{CreateSolidBrush, DeleteObject, LOGFONTW, MAKEPOINTS, RGB};
use winapi::um::winnt::OSVERSIONINFOW;
use winapi::um::winuser::*;
use windows::UI::Color as WUIColor;
use windows::UI::ViewManagement::{UIColorType, UISettings};
use winreg::enums::HKEY_CURRENT_USER;
use winreg::RegKey;

const GCS_RESULTSTR: DWORD = 0x800;
const GCS_COMPSTR: DWORD = 0x8;
const ISC_SHOWUICOMPOSITIONWINDOW: DWORD = 0x80000000;

#[allow(non_snake_case)]
#[repr(C)]
pub struct CANDIDATEFORM {
    dwIndex: DWORD,
    dwStyle: DWORD,
    ptCurrentPos: POINT,
    rcArea: RECT,
}
pub type LPCANDIDATEFORM = *mut CANDIDATEFORM;

extern "system" {
    pub fn ImmGetCompositionStringW(himc: HIMC, index: DWORD, buf: LPVOID, buflen: DWORD) -> LONG;
    pub fn ImmSetCandidateWindow(himc: HIMC, lpCandidate: LPCANDIDATEFORM) -> BOOL;
}

lazy_static! {
    static ref IS_WIN10: bool = {
        let osver = OSVERSIONINFOW {
            dwOSVersionInfoSize: std::mem::size_of::<OSVERSIONINFOW>() as _,
            ..Default::default()
        };

        // SAFETY: `osver` is a stack-local, fully-initialized `OSVERSIONINFOW`
        // with the correct `dwOSVersionInfoSize`; `GetVersionExW` only reads it.
        if unsafe { GetVersionExW(&osver as *const _ as _) } == winapi::shared::minwindef::TRUE {
            osver.dwBuildNumber < 22000
        } else {
            true
        }
    };
    static ref IS_WIN11_22H2: bool = {
        let osver = OSVERSIONINFOW {
            dwOSVersionInfoSize: std::mem::size_of::<OSVERSIONINFOW>() as _,
            ..Default::default()
        };

        // SAFETY: `osver` is a stack-local, fully-initialized `OSVERSIONINFOW`
        // with the correct `dwOSVersionInfoSize`; `GetVersionExW` only reads it.
        if unsafe { GetVersionExW(&osver as *const _ as _) } == winapi::shared::minwindef::TRUE {
            osver.dwBuildNumber >= 22621
        } else {
            true
        }
    };
    static ref TITLE_FONT: Mutex<Option<parameters::FontAndSize>> = Mutex::new(None);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub(crate) struct HWindow(HWND);
// SAFETY: `HWindow` is a plain newtype around an `HWND` used only as an opaque
// identifier/token (it is `Copy` and never dereferenced into shared state by
// these impls). An `HWND` is a process-global handle and sending/sharing the
// bare token value across threads is sound; actual window operations are only
// ever performed on the window's owning thread via the message loop.
unsafe impl Send for HWindow {}
// SAFETY: same rationale as the `Send` impl above.
unsafe impl Sync for HWindow {}

pub(crate) struct WindowInner {
    /// Non-owning reference to the window handle
    hwnd: HWindow,
    /// The `WS_CHILD` window that the WebGpu swapchain surface targets
    /// instead of `hwnd` directly (see `Window::create_webgpu_child_window`).
    /// Non-owning: Windows destroys it automatically as a child when `hwnd`
    /// is destroyed. Kept sized/positioned to exactly cover `hwnd`'s client
    /// area by `check_and_call_resize_if_needed`.
    webgpu_child_hwnd: HWindow,
    /// Old WebGpu child HWNDs that have been superseded by a renderer
    /// rebuild (see `Window::recreate_webgpu_child_window`) but not yet
    /// `DestroyWindow`-ed, paired with a type-erased `Weak` handle to the
    /// `WebGpuState` (owned by `wezterm-gui`, which this crate cannot name
    /// directly) whose `wgpu::Surface`/DXGI swapchain targets that HWND.
    ///
    /// Why defer the destroy at all: `begin_renderer_rebuild`
    /// (`wezterm-gui`) only *signals* the old render thread to shut down
    /// (`RenderThreadHandle::shutdown`, by design non-blocking -- joining
    /// would reintroduce the GUI-thread block this whole architecture
    /// exists to avoid) before starting the rebuild. If that thread is
    /// wedged inside `submit_frame`/`present()` -- the whole reason a
    /// rebuild was triggered -- it can still be holding its own strong
    /// `Arc<WebGpuState>` (and therefore the live surface) when the rebuild
    /// reaches this child-window step. `DestroyWindow`-ing the HWND out
    /// from under a still-live swapchain surface is at best undefined,
    /// driver-dependent behavior, so instead we hide the old HWND and keep
    /// it here until the `Weak` reports zero strong references, i.e. until
    /// the (possibly-late) render thread has actually returned and dropped
    /// its `Arc`. Swept by `sweep_retired_webgpu_children`, called from
    /// both `TermWindow::check_render_thread_hang_tick`'s existing ~2s
    /// timer (normal case) and from `close` (so a full window close doesn't
    /// leave any of these to `close`'s own `DestroyWindow` cleanup, though
    /// see the note on `close` -- even if we didn't, `hwnd`'s own
    /// `WS_CHILD` cleanup would catch them as a backstop, since these are
    /// still children of `hwnd` for as long as they live).
    retired_webgpu_children: Vec<(HWindow, std::sync::Weak<dyn Any + Send + Sync>)>,
    events: WindowEventSender,
    gl_state: Option<Rc<glium::backend::Context>>,
    /// Fraction of mouse scroll
    hscroll_remainder: i16,
    vscroll_remainder: i16,

    last_size: Option<Dimensions>,
    in_size_move: bool,
    dead_pending: Option<(Modifiers, u32)>,
    saved_placement: Option<WINDOWPLACEMENT>,
    track_mouse_leave: bool,
    window_drag_position: Option<ScreenPoint>,
    maximize_button_position: Option<ScreenRect>,

    keyboard_info: KeyboardLayoutInfo,
    appearance: Appearance,

    config: ConfigHandle,
    paint_throttled: bool,
    invalidated: bool,

    /// A solid brush matching the effective config palette's terminal
    /// background color, used only by `WM_ERASEBKGND` to paint the client
    /// area before the first real GPU frame lands. The window class is
    /// registered with `hbrBackground: null_mut()` (see `create_window`) so
    /// that a *working* renderer never gets an extra background erase on
    /// every resize; this brush exists purely to cover the gap between
    /// `ShowWindow` and the renderer's first frame, where the alternative is
    /// whatever garbage happened to be in that region of the framebuffer, or
    /// (worse, on a dark theme) a stark white flash from an unpainted
    /// client area. Cleared via `clear_placeholder_background` as soon as
    /// `TermWindow::created` installs a working `RenderState`, at which
    /// point the renderer itself is responsible for every subsequent frame
    /// and `WM_ERASEBKGND` goes back to being a no-op (returning 1 without
    /// painting, matching today's behavior of a null-brush class).
    placeholder_background_brush: Option<HBRUSH>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct Window(HWindow);

fn wuicolor_to_linearrgba(color: WUIColor) -> LinearRgba {
    LinearRgba::with_srgba(color.R, color.G, color.B, 255)
}

/// Build the `WM_ERASEBKGND` placeholder brush for a not-yet-rendered
/// window, colored from the config's effective, fully-resolved palette
/// (color scheme merged with any explicit `colors` overrides -- the exact
/// same `resolved_palette` field that `TermConfig::color_palette` hands to
/// the terminal model, see `crates/config/src/terminal.rs`). When the
/// palette doesn't specify a background (`None`), fall back to the same
/// default the terminal itself would use: `wezterm_term::color::
/// ColorPalette::default().background`, which is plain black. That default
/// is duplicated here as a literal rather than depending on the `term`
/// crate (this `window` crate intentionally has no such dependency) --
/// black is also the safe conservative choice if the two ever drifted,
/// since it matches `hbrBackground: null_mut()`'s previous de-facto
/// behavior of leaving stale (usually dark) framebuffer contents visible
/// on unpainted, freshly-allocated window surfaces.
///
/// # Safety
/// Always safe to call: only reads plain config values and calls
/// `CreateSolidBrush`, which cannot fail in a way that produces an invalid
/// non-null handle.
fn create_placeholder_background_brush(config: &ConfigHandle) -> HBRUSH {
    let (r, g, b, _a) = config
        .resolved_palette
        .background
        .map(|c| c.as_rgba_u8())
        .unwrap_or((0, 0, 0, 0xff));
    // SAFETY: `RGB` is a pure macro over three `u8`s; `CreateSolidBrush`
    // takes a plain `COLORREF` value and returns either a valid `HBRUSH` or
    // null on failure (e.g. GDI handle exhaustion), which is an accepted,
    // extremely unlikely degenerate case here -- see `WM_ERASEBKGND`'s
    // handler, which already tolerates a null brush by falling back to "no
    // paint" instead of dereferencing it.
    unsafe { CreateSolidBrush(RGB(r, g, b)) }
}

fn rect_width(r: &RECT) -> i32 {
    r.right - r.left
}

fn rect_height(r: &RECT) -> i32 {
    r.bottom - r.top
}

fn adjust_client_to_window_dimensions(
    style: u32,
    width: usize,
    height: usize,
    dpi: u32,
) -> (i32, i32) {
    let mut rect = RECT {
        left: 0,
        top: 0,
        right: width as _,
        bottom: height as _,
    };
    // SAFETY: `rect` is a live `RECT` and `style`/`dpi` are plain integers;
    // there is no menu (bMenu=0) and the ex-style is 0. The call only writes
    // back into `rect`.
    unsafe { AdjustWindowRectExForDpi(&mut rect, style, 0, 0, dpi) };

    (rect_width(&rect), rect_height(&rect))
}

fn rc_to_pointer(arc: &Rc<RefCell<WindowInner>>) -> *const RefCell<WindowInner> {
    let cloned = Rc::clone(arc);
    // SAFETY: `cloned` is a freshly cloned `Rc` with refcount incremented, so
    // `into_raw` leaks one strong reference that remains valid until reclaimed
    // by `Rc::from_raw`. The raw pointer is stored in the window's user data.
    Rc::into_raw(cloned)
}

fn rc_from_pointer(lparam: LPVOID) -> Rc<RefCell<WindowInner>> {
    // SAFETY: `lparam` is a pointer previously produced by `rc_to_pointer`
    // (and stored in the window's GWLP_USERDATA) and is thus a valid `Rc` raw
    // pointer with a live strong reference. We `from_raw` to borrow it, clone
    // (incrementing the refcount for the caller), then `into_raw` to leave the
    // original strong reference intact so the stored pointer stays valid.
    let arc = unsafe { Rc::from_raw(std::mem::transmute(lparam)) };
    // Add a ref for the caller
    let cloned = Rc::clone(&arc);

    // We must not drop this ref though; turn it back into a raw pointer!
    let _ = Rc::into_raw(arc);

    cloned
}

fn rc_from_hwnd(hwnd: HWND) -> Option<Rc<RefCell<WindowInner>>> {
    // SAFETY: `hwnd` is a valid window handle and `GWLP_USERDATA` was set to an
    // `Rc` raw pointer (via `rc_to_pointer`) during `WM_NCCREATE`, or is left
    // null for windows we did not create. We only reinterpret a non-null value.
    let raw = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) as LPVOID };
    if raw.is_null() {
        None
    } else {
        Some(rc_from_pointer(raw))
    }
}

fn take_rc_from_pointer(lparam: LPVOID) -> Rc<RefCell<WindowInner>> {
    // SAFETY: `lparam` is an `Rc` raw pointer produced by `rc_to_pointer` with
    // a live strong reference; `from_raw` reclaims that reference (the caller
    // transfers ownership rather than borrowing it, unlike `rc_from_pointer`).
    unsafe { Rc::from_raw(std::mem::transmute(lparam)) }
}

fn callback_behavior() -> glium::debug::DebugCallbackBehavior {
    if cfg!(debug_assertions) && false
    /* https://github.com/glium/glium/issues/1885 */
    {
        glium::debug::DebugCallbackBehavior::DebugMessageOnError
    } else {
        glium::debug::DebugCallbackBehavior::Ignore
    }
}

impl HasDisplayHandle for WindowInner {
    fn display_handle(&self) -> Result<DisplayHandle, HandleError> {
        // SAFETY: `WindowsDisplayHandle` is a zero-sized marker with no raw
        // pointers, so borrowing it raw for the lifetime of the handle is sound.
        unsafe {
            Ok(DisplayHandle::borrow_raw(RawDisplayHandle::Windows(
                WindowsDisplayHandle::new(),
            )))
        }
    }
}

impl HasWindowHandle for WindowInner {
    fn window_handle(&self) -> Result<WindowHandle, HandleError> {
        let mut handle =
            Win32WindowHandle::new(NonZeroIsize::new(self.hwnd.0 as _).expect("non-zero"));
        // SAFETY: passing `null()` for the module name returns the handle of
        // the current process's exe, which is always valid and non-null.
        handle.hinstance = NonZeroIsize::new(unsafe { GetModuleHandleW(null()) } as _);
        // SAFETY: `self.hwnd.0` is a live window handle valid for the lifetime
        // of `WindowInner`; the constructed `Win32WindowHandle` mirrors it, so
        // borrowing it raw is sound.
        unsafe { Ok(WindowHandle::borrow_raw(RawWindowHandle::Win32(handle))) }
    }
}

impl WindowInner {
    fn enable_opengl(&mut self) -> anyhow::Result<Rc<glium::backend::Context>> {
        let conn = Connection::get().unwrap();

        let gl_state = if self.config.prefer_egl {
            match conn.gl_connection.borrow().as_ref() {
                None => crate::egl::GlState::create(None, self.hwnd.0),
                Some(glconn) => {
                    crate::egl::GlState::create_with_existing_connection(glconn, self.hwnd.0)
                }
            }
        } else {
            Err(anyhow::anyhow!("Config says to avoid EGL"))
        }
        .and_then(|egl| {
            // SAFETY: `egl` is a freshly-created, valid EGL `Backend` owning a
            // current GL context; `glium::backend::Context::new` is unsafe only
            // because it trusts the backend to be a valid GL context provider.
            unsafe {
                log::trace!("Initialized EGL!");
                conn.gl_connection
                    .borrow_mut()
                    .replace(Rc::clone(egl.get_connection()));
                let backend = Rc::new(egl);
                Ok(glium::backend::Context::new(
                    backend,
                    true,
                    callback_behavior(),
                )?)
            }
        })
        .or_else(|err| {
            log::trace!("EGL init failed {:?}, fall back to WGL", err);
            super::wgl::GlState::create(self.hwnd.0).and_then(|state| {
                // SAFETY: `state` is a freshly-created, valid WGL `Backend`
                // owning a current GL context; same rationale as the EGL branch.
                unsafe {
                    Ok(glium::backend::Context::new(
                        Rc::new(state),
                        true,
                        callback_behavior(),
                    )?)
                }
            })
        })?;

        self.gl_state.replace(gl_state.clone());

        Ok(gl_state)
    }

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
        /*
        if self.gl_state.is_none() {
            // Don't cache state or generate resize callbacks until
            // we've set up opengl, otherwise we can miss propagating
            // some state during the initial window setup that results
            // in the window dimensions being out of sync with the dpi
            // when eg: the system display settings are set to 200%
            // scale factor.
            return false;
        }
        */

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

        let same = self
            .last_size
            .as_ref()
            .map(|&dims| dims == current_dims)
            .unwrap_or(false);
        self.last_size.replace(current_dims);

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

    fn apply_decoration(&mut self) {
        let hwnd = self.hwnd.0;
        schedule_apply_decoration(hwnd, self.config.window_decorations);
    }
}

fn schedule_apply_decoration(hwnd: HWND, decorations: WindowDecorations) {
    promise::spawn::spawn(async move {
        apply_decoration_immediate(hwnd, decorations);
    })
    .detach();
}

fn apply_decoration_immediate(hwnd: HWND, decorations: WindowDecorations) {
    match rc_from_hwnd(hwnd) {
        Some(inner) => {
            if inner.borrow().saved_placement.is_some() {
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

fn decorations_to_style(decorations: WindowDecorations) -> u32 {
    if decorations == WindowDecorations::RESIZE {
        WS_OVERLAPPEDWINDOW
    } else if decorations == WindowDecorations::TITLE {
        WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_MAXIMIZEBOX
    } else if decorations == WindowDecorations::NONE {
        WS_POPUP
    } else if decorations == WindowDecorations::TITLE | WindowDecorations::RESIZE {
        WS_OVERLAPPEDWINDOW
    } else {
        WS_OVERLAPPEDWINDOW
    }
}

fn get_primary_monitor_dpi() -> u32 {
    // SAFETY: a null hwnd with MONITOR_DEFAULTTOPRIMARY returns the primary
    // monitor handle, which is asserted non-null below.
    let primary = unsafe { MonitorFromWindow(null_mut(), MONITOR_DEFAULTTOPRIMARY) };
    assert!(!primary.is_null(), "MonitorFromWindow() returned NULL");
    let mut dpi_x = USER_DEFAULT_SCREEN_DPI as u32;
    let mut dpi_y = USER_DEFAULT_SCREEN_DPI as u32;
    // SAFETY: `primary` is a valid monitor handle (asserted above) and the dpi
    // out-params are valid `u32` pointers that the call only writes to.
    unsafe { GetDpiForMonitor(primary, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) };
    dpi_x
}

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
            // FIXME: this resource is specific to the wezterm build and this should
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
                std::mem::transmute(lparam),
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
            style: CS_HREDRAW | CS_VREDRAW,
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
        }

        let name = wide_string("OnlyTermWebGpuChild");
        // SAFETY: `class_name`/`name` are live null-terminated UTF-16 buffers.
        // `parent` is the valid, just-created top-level HWND, so passing it
        // makes this a `WS_CHILD` window owned by it (destroyed automatically
        // when `parent` is destroyed). No menu/custom instance/create-params
        // are needed since this window has no `WindowInner` of its own. A
        // null result is reported as an error below.
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class_name.as_ptr(),
                name.as_ptr(),
                WS_CHILD | WS_VISIBLE,
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
            None => config::configuration(),
        };
        let appearance = get_appearance();

        // Create the `WM_ERASEBKGND` placeholder brush up front, from the
        // *effective* palette (color scheme + explicit overrides already
        // resolved into `config.resolved_palette`, exactly like
        // `TermConfig::color_palette` does for the terminal model itself --
        // see `crates/config/src/terminal.rs`), not a hardcoded color. This
        // is the same background the terminal will actually paint once the
        // renderer comes up, so a placeholder-colored window handed to the
        // user before the first GPU frame is indistinguishable from (or at
        // worst a same-color no-op blend with) the real thing. Hardcoding
        // e.g. white here would be invisible for this fork's light-theme
        // default but would flash white on every dark theme -- precisely
        // the defect this placeholder exists to prevent.
        let placeholder_background_brush = Some(create_placeholder_background_brush(&config));

        let inner = Rc::new(RefCell::new(WindowInner {
            hwnd: HWindow(null_mut()),
            webgpu_child_hwnd: HWindow(null_mut()),
            retired_webgpu_children: Vec::new(),
            appearance,
            events,
            gl_state: None,
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
            placeholder_background_brush,
        }));

        // Careful: `raw` owns a ref to inner, but there is no Drop impl
        let raw = rc_to_pointer(&inner);

        let conn = Connection::get().expect("Connection::init was not called");

        let geometry = conn.resolve_geometry(geometry);

        let hwnd = match Self::create_window(config, class_name, name, geometry, raw) {
            Ok(hwnd) => HWindow(hwnd),
            Err(err) => {
                // Ensure that we drop the extra ref to raw before we return
                // SAFETY: `raw` was produced by `rc_to_pointer` above (a valid
                // `Rc` raw pointer with one extra strong ref); since window
                // creation failed it was never stored, so we reclaim it here.
                drop(unsafe { Rc::from_raw(raw) });
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
            inner_mut.events.assign_window(window_handle.clone());
        }

        apply_theme(hwnd.0);
        enable_blur_behind(hwnd.0);

        // Make window capable of accepting drag and drop
        // SAFETY: `hwnd.0` is a valid, just-created window handle.
        unsafe {
            DragAcceptFiles(hwnd.0, winapi::shared::minwindef::TRUE);
        }

        conn.windows
            .borrow_mut()
            .insert(hwnd.clone(), Rc::clone(&inner));

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
    /// name `wezterm_gui::termwindow::webgpu::WebGpuState` directly, hence
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
    /// `async` and deferred via `promise::spawn::spawn`, mirroring
    /// `enable_opengl` just below: the caller (`TermWindow`'s render-thread
    /// hang supervisor) always reaches this synchronously from inside
    /// `notify()`'s `WindowEvent::Notification` dispatch, which is itself
    /// invoked from `Connection::with_window_inner` while it still holds
    /// this exact window's `WindowInner` `RefCell` mutably borrowed (see
    /// `notify`). Borrowing it again *synchronously* here would panic with
    /// "already mutably borrowed" -- this bit the first version of this
    /// method in manual testing. Deferring the actual `get_window`/`borrow`
    /// to a freshly spawned task lets that outer borrow finish and drop
    /// first, exactly like `enable_opengl` already has to for the same
    /// reason during `new_window`.
    pub async fn recreate_webgpu_child_window(
        &self,
        old_webgpu_state: std::sync::Weak<dyn Any + Send + Sync>,
    ) -> anyhow::Result<()> {
        let window = self.0;
        promise::spawn::spawn(async move {
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
            handle.borrow_mut().webgpu_child_hwnd = HWindow(new_child);
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
    /// Deferred via `promise::spawn::spawn`, exactly like
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
        promise::spawn::spawn(async move {
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

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum ShowWindowCommand {
    Normal,
    Minimize,
    Maximize,
}

fn schedule_show_window(hwnd: HWindow, show: ShowWindowCommand) {
    // ShowWindow can call to the window proc and may attempt
    // to lock inner, so we avoid locking it ourselves here
    log::trace!("scheduling ShowWindowCommand {show:?}");
    promise::spawn::spawn(async move {
        // SAFETY: `hwnd.0` is a valid window handle and the show command is a
        // valid `SW_*` constant.
        unsafe {
            log::trace!("applying ShowWindowCommand {show:?}");
            ShowWindow(
                hwnd.0,
                match show {
                    ShowWindowCommand::Normal => SW_NORMAL,
                    ShowWindowCommand::Minimize => SW_MINIMIZE,
                    ShowWindowCommand::Maximize => SW_MAXIMIZE,
                },
            );
            // Force a repaint of the whole client area now that the window
            // is on screen. Making a window visible does *not* invalidate
            // it: the client area keeps whatever the redirection surface
            // already held, and any painting we did while it was still
            // hidden was never composited. Without this, a window shown
            // before its renderer is up (early show, task #331) sits there
            // blank -- white, whatever the configured background is --
            // until something else happens to invalidate it, which for an
            // idle window is not until the renderer's own first frame
            // several seconds later. `RDW_ALLCHILDREN` matters as much as
            // the invalidate itself: the WebGpu child window (created up
            // front, `WS_VISIBLE`, covering the whole client area) is what
            // the user actually sees, and a plain `InvalidateRect` on the
            // parent does not reach it. `RDW_ERASE` so the placeholder fill
            // runs as part of the resulting paint cycle.
            RedrawWindow(
                hwnd.0,
                null(),
                null_mut(),
                RDW_INVALIDATE | RDW_ERASE | RDW_ALLCHILDREN,
            );
        }
    })
    .detach();
}

impl WindowInner {
    /// Destroy any retired WebGpu child HWNDs whose paired `Weak` has hit
    /// zero strong references. Shared body for `Window::sweep_retired_webgpu_children`
    /// (reached via `Connection::get_window`) and `close` below (which
    /// already holds `&mut self` directly, so it can call this without an
    /// extra `Connection` round-trip). See `retired_webgpu_children`'s doc
    /// comment for the full rationale.
    fn sweep_retired_webgpu_children(&mut self) {
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

    /// Drop the `WM_ERASEBKGND` placeholder brush (see
    /// `placeholder_background_brush`'s doc comment) once a working
    /// renderer is in place and responsible for painting every frame.
    /// Idempotent: safe to call more than once (e.g. once from
    /// `TermWindow::created` on the happy path, and once more as a backstop
    /// from `wm_ncdestroy` if the window is closed before a renderer ever
    /// came up) -- `Option::take` makes the second call a no-op.
    fn clear_placeholder_background(&mut self) {
        if let Some(brush) = self.placeholder_background_brush.take() {
            // SAFETY: `brush` was created by `CreateSolidBrush` in
            // `create_placeholder_background_brush` and is not shared with
            // any other GDI object or in use by an in-flight `FillRect`
            // call (both `WM_ERASEBKGND` and this method only ever run on
            // this window's single GUI/message-loop thread), so deleting it
            // now cannot race a concurrent use.
            unsafe {
                DeleteObject(brush as _);
            }
        }
    }

    fn close(&mut self) {
        // Eagerly destroy any retired WebGpu child HWNDs we can right now
        // (task #283). Not strictly required for correctness -- any left
        // in `retired_webgpu_children` are still `WS_CHILD` windows of
        // `self.hwnd`, which Windows destroys automatically as part of
        // `hwnd`'s own teardown below -- but doing it here avoids relying
        // on that implicit cleanup when we can just as easily check now.
        self.sweep_retired_webgpu_children();
        let hwnd = self.hwnd;
        promise::spawn::spawn(async move {
            // SAFETY: `hwnd.0` is a valid window handle; `DestroyWindow` is
            // queued on the owning thread via the spawned task.
            unsafe {
                DestroyWindow(hwnd.0);
            }
        })
        .detach();
    }

    fn set_cursor(&mut self, cursor: Option<MouseCursor>) {
        apply_mouse_cursor(cursor);
    }

    fn set_window_position(&self, coords: ScreenPoint) {
        let hwnd = self.hwnd.0;
        log::trace!("set_window_position wants {coords:?}");
        promise::spawn::spawn(async move {
            log::trace!("set_window_position apply {coords:?}");
            let mut rect = RECT {
                left: 0,
                bottom: 0,
                right: 0,
                top: 0,
            };
            // SAFETY: `hwnd` is the window's own live HWND; `rect` is a valid
            // out-parameter for `GetWindowRect`, and `client_to_screen` below
            // takes the same valid `hwnd`.
            unsafe {
                GetWindowRect(hwnd, &mut rect);

                let origin = client_to_screen(hwnd, Point::new(0, 0));
                let delta_x = origin.x as i32 - rect.left;
                let delta_y = origin.y as i32 - rect.top;

                MoveWindow(
                    hwnd,
                    coords.x as i32 - delta_x,
                    coords.y as i32 - delta_y,
                    rect_width(&rect),
                    rect_height(&rect),
                    1,
                );
            }
        })
        .detach();
    }

    fn set_title(&mut self, title: &str) {
        let title = wide_string(title);
        // SAFETY: `self.hwnd.0` is a valid window handle and `title` is a live
        // null-terminated UTF-16 buffer.
        unsafe {
            SetWindowTextW(self.hwnd.0, title.as_ptr());
        }
    }

    fn set_text_cursor_position(&mut self, cursor: Rect) {
        self.set_ime_window_position(cursor);
    }

    fn set_ime_window_position(&mut self, cursor: Rect) {
        let imc = ImmContext::get(self.hwnd.0);
        match self.config.ime_preedit_rendering {
            ImePreeditRendering::Builtin => imc.set_candidate_window_position(cursor),
            ImePreeditRendering::System => imc.set_composition_window_position(cursor),
        }
    }

    fn config_did_change(&mut self, config: &ConfigHandle) {
        self.config = config.clone();
        self.apply_decoration();
    }

    fn toggle_fullscreen(&mut self) {
        // SAFETY: `self.hwnd.0` is a valid window handle; all FFI calls receive
        // valid handle/pointer args (zeroed/sized `WINDOWPLACEMENT`/`MONITORINFO`,
        // valid style integers and no-op size flags). The window state is only
        // mutated via `SetWindow*` from the owning thread.
        unsafe {
            let hwnd = self.hwnd.0;
            let style = GetWindowLongW(hwnd, GWL_STYLE);
            let config = self.config.clone();
            if let Some(placement) = self.saved_placement.take() {
                promise::spawn::spawn(async move {
                    let style = decorations_to_style(config.window_decorations);
                    SetWindowLongW(hwnd, GWL_STYLE, style as i32);
                    SetWindowPlacement(hwnd, &placement);
                    SetWindowPos(
                        hwnd,
                        std::ptr::null_mut(),
                        0,
                        0,
                        0,
                        0,
                        SWP_NOMOVE
                            | SWP_NOSIZE
                            | SWP_NOZORDER
                            | SWP_NOOWNERZORDER
                            | SWP_FRAMECHANGED,
                    );
                })
                .detach();
            } else {
                let mut placement: WINDOWPLACEMENT = std::mem::zeroed();
                GetWindowPlacement(hwnd, &mut placement);

                self.saved_placement.replace(placement);
                promise::spawn::spawn(async move {
                    let mut mi: MONITORINFO = std::mem::zeroed();
                    mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
                    GetMonitorInfoW(MonitorFromWindow(hwnd, MONITOR_DEFAULTTOPRIMARY), &mut mi);
                    SetWindowLongW(hwnd, GWL_STYLE, style & !(WS_OVERLAPPEDWINDOW as i32));
                    SetWindowPos(
                        hwnd,
                        HWND_TOP,
                        mi.rcMonitor.left,
                        mi.rcMonitor.top,
                        mi.rcMonitor.right - mi.rcMonitor.left,
                        mi.rcMonitor.bottom - mi.rcMonitor.top,
                        SWP_NOOWNERZORDER | SWP_FRAMECHANGED,
                    );
                })
                .detach();
            }
        }
    }
}

impl HasDisplayHandle for Window {
    fn display_handle(&self) -> Result<DisplayHandle, HandleError> {
        // SAFETY: `WindowsDisplayHandle` is a zero-sized marker with no raw
        // pointers, so borrowing it raw is sound.
        unsafe {
            Ok(DisplayHandle::borrow_raw(RawDisplayHandle::Windows(
                WindowsDisplayHandle::new(),
            )))
        }
    }
}

impl HasWindowHandle for Window {
    fn window_handle(&self) -> Result<WindowHandle, HandleError> {
        let conn = Connection::get().expect("raw_window_handle only callable on main thread");
        let handle = conn.get_window(self.0).expect("window handle invalid!?");

        let inner = handle.borrow();
        let handle = inner.window_handle()?;
        // SAFETY: `handle` is a valid `Win32WindowHandle` backed by a live `hwnd`
        // kept alive by the owning `Connection`, so borrowing it raw is sound.
        unsafe { Ok(WindowHandle::borrow_raw(handle.as_raw())) }
    }
}

#[async_trait(?Send)]
impl WindowOps for Window {
    async fn enable_opengl(&self) -> anyhow::Result<Rc<glium::backend::Context>> {
        let window = self.0;
        promise::spawn::spawn(async move {
            if let Some(handle) = Connection::get().unwrap().get_window(window) {
                let mut inner = handle.borrow_mut();
                inner.enable_opengl()
            } else {
                anyhow::bail!("invalid window");
            }
        })
        .await
    }

    fn notify<T: Any + Send + Sync>(&self, t: T)
    where
        Self: Sized,
    {
        Connection::with_window_inner(self.0, move |inner| {
            inner
                .events
                .dispatch(WindowEvent::Notification(Box::new(t)));
            Ok(())
        });
    }

    fn close(&self) {
        Connection::with_window_inner(self.0, |inner| {
            inner.close();
            Ok(())
        });
    }

    fn show(&self) {
        schedule_show_window(self.0, ShowWindowCommand::Normal);
    }

    fn hide(&self) {
        schedule_show_window(self.0, ShowWindowCommand::Minimize);
    }

    fn focus(&self) {
        let window = self.0;
        let handle = window.0;
        promise::spawn::spawn(async move {
            // In some situation, calling SetForegroundWindow could not bring up the window,
            // This is a little hack which can "steal" the foreground window permission
            // We only call this function in the window creation, so it should be fine.
            // See : https://stackoverflow.com/questions/10740346/setforegroundwindow-only-working-while-visual-studio-is-open
            // SAFETY: `handle` is a valid window handle; the two `INPUT` structs are
            // fully initialized keyboard events and `SendInput` receives their
            // correct count/pointer/size.
            unsafe {
                let alt_sc = MapVirtualKeyW(VK_MENU as u32, MAPVK_VK_TO_VSC);

                let mut inputs: [INPUT; 2] = [
                    INPUT {
                        type_: INPUT_KEYBOARD,
                        u: Default::default(),
                    },
                    INPUT {
                        type_: INPUT_KEYBOARD,
                        u: Default::default(),
                    },
                ];
                *inputs[0].u.ki_mut() = KEYBDINPUT {
                    wVk: VK_LMENU as u16,
                    wScan: alt_sc as u16,
                    dwFlags: KEYEVENTF_EXTENDEDKEY,
                    dwExtraInfo: 0,
                    time: 0,
                };
                *inputs[1].u.ki_mut() = KEYBDINPUT {
                    wVk: VK_LMENU as u16,
                    wScan: alt_sc as u16,
                    dwFlags: KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP,
                    dwExtraInfo: 0,
                    time: 0,
                };

                // Simulate a key press and release
                SendInput(
                    inputs.len() as u32,
                    inputs.as_mut_ptr(),
                    std::mem::size_of::<INPUT>() as i32,
                );

                SetForegroundWindow(handle);
            }
        })
        .detach();
    }

    fn maximize(&self) {
        schedule_show_window(self.0, ShowWindowCommand::Maximize);
    }

    fn restore(&self) {
        schedule_show_window(self.0, ShowWindowCommand::Normal);
    }

    fn set_cursor(&self, cursor: Option<MouseCursor>) {
        Connection::with_window_inner(self.0, move |inner| {
            inner.set_cursor(cursor);
            Ok(())
        });
    }

    fn invalidate(&self) {
        let hwnd = self.0 .0;
        log::trace!("WindowOps::invalidate calling InvalidateRect");
        // SAFETY: `hwnd` is a valid window handle; a null rect invalidates the
        // whole client area and the erase flag (0) is a valid constant.
        unsafe {
            InvalidateRect(hwnd, null(), 0);
        }
    }

    fn clear_placeholder_background(&self) {
        Connection::with_window_inner(self.0, move |inner| {
            inner.clear_placeholder_background();
            Ok(())
        });
    }

    fn set_title(&self, title: &str) {
        let title = title.to_owned();
        Connection::with_window_inner(self.0, move |inner| {
            inner.set_title(&title);
            Ok(())
        });
    }

    fn toggle_fullscreen(&self) {
        Connection::with_window_inner(self.0, move |inner| {
            inner.toggle_fullscreen();
            Ok(())
        });
    }

    fn config_did_change(&self, config: &ConfigHandle) {
        let config = config.clone();
        Connection::with_window_inner(self.0, move |inner| {
            inner.config_did_change(&config);
            Ok(())
        });
    }

    fn set_text_cursor_position(&self, cursor: Rect) {
        Connection::with_window_inner(self.0, move |inner| {
            inner.set_text_cursor_position(cursor);
            Ok(())
        });
    }

    fn set_inner_size(&self, width: usize, height: usize) {
        Connection::with_window_inner(self.0, move |inner| {
            let hwnd = inner.hwnd;
            let decorations = inner.config.window_decorations;
            promise::spawn::spawn(async move {
                log::trace!("set_inner_size called with {width}x{height}");
                // SAFETY: `hwnd.0` is a valid window handle.
                let frame_dpi = unsafe { GetDpiForWindow(hwnd.0) };
                let (width, height) = adjust_client_to_window_dimensions(
                    decorations_to_style(decorations),
                    width,
                    height,
                    frame_dpi,
                );
                let window_state = get_window_state(hwnd.0);
                if window_state.can_resize() {
                    log::trace!("set_inner_size now calling SetWindowPos with {width}x{height}");
                    // SAFETY: `hwnd.0` is a valid handle; NOMOVE|NOZORDER
                    // make position/insert-after args inert.
                    unsafe {
                        SetWindowPos(
                            hwnd.0,
                            hwnd.0,
                            0,
                            0,
                            width,
                            height,
                            SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOZORDER,
                        );
                        wm_paint(hwnd.0, 0, 0, 0);
                        if let Some(inner) = rc_from_hwnd(hwnd.0) {
                            let mut inner = inner.borrow_mut();
                            inner.events.dispatch(WindowEvent::SetInnerSizeCompleted);
                        }
                    }
                } else {
                    log::trace!(
                        "ignoring set_inner_size({width}, {height}) call \
                                because window_state is {window_state:?}"
                    );
                }
            })
            .detach();
            Ok(())
        });
    }

    fn set_maximize_button_position(&self, coords: ScreenRect) {
        Connection::with_window_inner(self.0, move |inner| {
            inner.maximize_button_position = Some(coords);
            Ok(())
        });
    }

    fn set_window_position(&self, coords: ScreenPoint) {
        Connection::with_window_inner(self.0, move |inner| {
            inner.set_window_position(coords);
            Ok(())
        });
    }

    fn get_clipboard(&self, _clipboard: Clipboard) -> Future<String> {
        Future::result(
            clipboard_win::get_clipboard_string()
                .map(|s| s.replace("\r\n", "\n"))
                .context("Error getting clipboard"),
        )
    }

    fn set_clipboard(&self, _clipboard: Clipboard, text: String) {
        clipboard_win::set_clipboard_string(&text).ok();
    }

    fn set_window_drag_position(&self, coords: ScreenPoint) {
        Connection::with_window_inner(self.0, move |inner| {
            inner.window_drag_position = Some(coords);

            Ok(())
        });
    }

    fn get_os_parameters(
        &self,
        config: &ConfigHandle,
        window_state: WindowState,
    ) -> anyhow::Result<Option<Parameters>> {
        let hwnd = self.0 .0;
        anyhow::ensure!(!hwnd.is_null(), "HWND is null");

        // SAFETY: `GetFocus` takes no arguments and returns the HWND of the
        // window with keyboard focus on the calling thread's message queue,
        // or null; comparing it to our own `hwnd` is a plain value comparison.
        let has_focus = unsafe { GetFocus() } == hwnd;
        let is_full_screen = window_state.contains(WindowState::FULL_SCREEN);

        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let use_accent = hkcu
            .open_subkey("SOFTWARE\\Microsoft\\Windows\\DWM")?
            .get_value::<u32, _>("ColorPrevalence")?;
        let settings = UISettings::new()?;
        let top_border_color = if has_focus {
            if use_accent == 1 {
                wuicolor_to_linearrgba(settings.GetColorValue(UIColorType::Accent)?)
            } else {
                if *IS_WIN10 {
                    LinearRgba(0.01, 0.01, 0.01, 0.67)
                } else {
                    LinearRgba(0.026, 0.026, 0.026, 0.5)
                }
            }
        } else {
            if *IS_WIN10 {
                LinearRgba(0.024, 0.024, 0.024, 0.5)
            } else {
                LinearRgba(0.028, 0.028, 0.028, 0.5)
            }
        };

        const BASE_BORDER: ULength = ULength::new(0);
        let is_resize = config.window_decorations == WindowDecorations::RESIZE;

        let title_font = {
            let font = TITLE_FONT.lock().expect("locking title_font");
            (*font).clone()
        };

        Ok(Some(Parameters {
            title_bar: parameters::TitleBar {
                padding_left: ULength::new(0),
                padding_right: ULength::new(0),
                height: None,
                font_and_size: title_font,
            },
            border_dimensions: Some(parameters::Border {
                top: if is_resize && !*IS_WIN10 && !is_full_screen {
                    BASE_BORDER + ULength::new(1)
                } else {
                    BASE_BORDER
                },
                left: BASE_BORDER,
                bottom: if is_resize && *IS_WIN10 && !is_full_screen {
                    BASE_BORDER + ULength::new(2)
                } else {
                    BASE_BORDER
                },
                right: BASE_BORDER,
                color: top_border_color,
            }),
        }))
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
        *font = wezterm_font::locator::gdi::parse_log_font(&lf, hdc).ok();
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
unsafe fn wm_nccreate(hwnd: HWND, _msg: UINT, _wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
    // SAFETY: `lparam` points at the `CREATESTRUCTW` Win32 passes to WM_NCCREATE.
    let create: &CREATESTRUCTW = &*(lparam as *const CREATESTRUCTW);
    let inner = rc_from_pointer(create.lpCreateParams);
    // SAFETY: valid hwnd; storing the `Rc` raw pointer in GWLP_USERDATA for later
    // recovery (balanced by `wm_ncdestroy`).
    SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as _);
    inner.borrow_mut().hwnd = HWindow(hwnd);

    None
}

/// Called when the window is being destroyed.
/// Goal is to release the WindowInner reference that was stashed
/// in the window by wm_nccreate.
///
/// # Safety
/// `hwnd` must be a valid window handle whose `GWLP_USERDATA` was set by
/// `wm_nccreate` (or is null).
unsafe fn wm_ncdestroy(
    hwnd: HWND,
    _msg: UINT,
    _wparam: WPARAM,
    _lparam: LPARAM,
) -> Option<LRESULT> {
    let raw = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as LPVOID;
    if !raw.is_null() {
        let inner = take_rc_from_pointer(raw);
        let mut inner = inner.borrow_mut();
        inner.events.dispatch(WindowEvent::Destroyed);
        inner.hwnd = HWindow(null_mut());
        // Backstop in case this window is closed before a renderer ever
        // came up (so `TermWindow::created` never ran and never called
        // `clear_placeholder_background` itself): make sure the brush is
        // always deleted rather than leaked. No-op if it was already
        // cleared.
        inner.clear_placeholder_background();
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
    }

    None
}

fn no_native_title_bar(decorations: WindowDecorations) -> bool {
    decorations == WindowDecorations::RESIZE
        || decorations.contains(WindowDecorations::INTEGRATED_BUTTONS)
}

/// # Safety
/// `hwnd` must be a valid window handle and `wparam`/`lparam` the values from a
/// real `WM_NCCALCSIZE` message (when `wparam==1`, `lparam` points at a valid
/// `NCCALCSIZE_PARAMS`).
unsafe fn wm_nccalcsize(hwnd: HWND, _msg: UINT, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
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
unsafe fn wm_nchittest(hwnd: HWND, msg: UINT, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
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

fn get_window_state(hwnd: HWND) -> WindowState {
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
        },
    }
}

/// "Blur behind" is the old vista term for a cool blurring
/// effect that the DWM could enable.  Subsequent windows
/// versions have removed the blurring.  We use this call
/// to tell DWM that we set proper alpha channel info as
/// a result of rendering our window content.
fn enable_blur_behind(hwnd: HWND) {
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

fn apply_theme(hwnd: HWND) -> Option<LRESULT> {
    // Check for OS app theme, and set window attributes accordingly.
    // Note that the MS terminal app uses the logic found here for this stuff:
    // https://github.com/microsoft/terminal/blob/9b92986b49bed8cc41fde4d6ef080921c41e6d9e/src/interactivity/win32/windowtheme.cpp#L62
    use winapi::um::dwmapi::{DwmExtendFrameIntoClientArea, DwmSetWindowAttribute};
    use winapi::um::uxtheme::MARGINS;

    #[allow(non_snake_case)]
    type WINDOWCOMPOSITIONATTRIB = u32;
    const WCA_USEDARKMODECOLORS: WINDOWCOMPOSITIONATTRIB = 26;

    #[allow(non_snake_case)]
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
                    cyTopHeight: margins,
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
unsafe fn wm_enter_exit_size_move(
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
unsafe fn wm_windowposchanged(
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
unsafe fn wm_size(hwnd: HWND, _msg: UINT, _wparam: WPARAM, _lparam: LPARAM) -> Option<LRESULT> {
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
unsafe fn wm_set_focus(
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
unsafe fn wm_kill_focus(
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

/// # Safety
/// `hwnd` must be a valid window handle; the `PAINTSTRUCT` is fully
/// initialized before being passed to `BeginPaint`/`EndPaint`.
unsafe fn wm_paint(hwnd: HWND, _msg: UINT, _wparam: WPARAM, _lparam: LPARAM) -> Option<LRESULT> {
    let inner = rc_from_hwnd(hwnd)?;
    let mut inner = inner.borrow_mut();

    if inner.paint_throttled {
        inner.invalidated = true;
        return Some(0);
    }

    let mut ps = PAINTSTRUCT {
        fErase: 0,
        fIncUpdate: 0,
        fRestore: 0,
        hdc: std::ptr::null_mut(),
        rcPaint: RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        },
        rgbReserved: [0; 32],
    };
    let hdc = BeginPaint(hwnd, &mut ps);
    // Fill the placeholder background here rather than leaving it to
    // `wm_erasebkgnd`. That handler can never do it during our own paint
    // cycle: `BeginPaint` sends `WM_ERASEBKGND` *synchronously*, from
    // inside this function, while we are still holding `borrow_mut()` on
    // the same `RefCell` -- so its `try_borrow` always fails and it skips
    // the fill. (And the repaint we schedule below uses
    // `InvalidateRect(.., bErase = 0)`, which doesn't request an erase in
    // the first place.) Doing it here, where `inner` is already borrowed,
    // is what actually makes a shown-but-not-yet-rendered window come up
    // in the terminal's background color instead of an unpainted white.
    if let Some(brush) = inner.placeholder_background_brush {
        if !hdc.is_null() {
            let mut rect = RECT {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            };
            // SAFETY: `hwnd` is the valid window handle passed in; `rect`
            // is a live stack `RECT` that `GetClientRect` only writes into.
            GetClientRect(hwnd, &mut rect);
            // SAFETY: `hdc` is the non-null device context just returned by
            // `BeginPaint` and still live until `EndPaint`; `brush` is a
            // live `HBRUSH` owned by `inner` (created in
            // `create_placeholder_background_brush`, deleted only in
            // `clear_placeholder_background`/`wm_ncdestroy`).
            FillRect(hdc, &rect, brush);
        }
    }
    EndPaint(hwnd, &mut ps);

    inner.invalidated = false;
    // Ask the app to repaint in a bit
    inner.events.dispatch(WindowEvent::NeedRepaint);

    inner.paint_throttled = true;
    let window_id = inner.hwnd;
    let max_fps = inner.config.max_fps;
    promise::spawn::spawn(async move {
        async_io::Timer::after(std::time::Duration::from_millis(1000 / max_fps as u64)).await;
        Connection::with_window_inner(window_id, move |inner| {
            inner.paint_throttled = false;
            if inner.invalidated {
                InvalidateRect(inner.hwnd.0, null(), 0);
            }
            Ok(())
        });
    })
    .detach();

    Some(0)
}

/// Handles `WM_ERASEBKGND`.
///
/// The window class is registered with `hbrBackground: null_mut()` (see
/// `create_window`), so ordinarily this message would go straight to
/// `DefWindowProc`, which does nothing with a null brush -- that's exactly
/// the behavior we want to preserve for a window whose renderer is already
/// up: no extra background erase (and therefore no flicker) on every
/// resize. The one gap is the window between `ShowWindow` and the
/// renderer's first real frame: with nothing painting the client area at
/// all, it would show whatever was previously in that region of the
/// framebuffer (garbage, or another window's content underneath).
///
/// While `placeholder_background_brush` is set, fill `rcPaint` with it and
/// report the background as erased (return 1). Once
/// `clear_placeholder_background` has dropped the brush (called from
/// `TermWindow::created` after a working `RenderState` is installed), fall
/// straight back to returning 1 without painting -- identical to today's
/// null-brush behavior.
///
/// # Safety
/// `hwnd` must be a valid window handle and `wparam` the `HDC` passed by
/// the real `WM_ERASEBKGND` message.
unsafe fn wm_erasebkgnd(
    hwnd: HWND,
    _msg: UINT,
    wparam: WPARAM,
    _lparam: LPARAM,
) -> Option<LRESULT> {
    let inner = match rc_from_hwnd(hwnd) {
        Some(inner) => inner,
        // No `WindowInner` yet (e.g. during early window-creation messages
        // before `WM_NCCREATE` has stashed it) -- nothing to paint with,
        // but still claim the erase happened so `DefWindowProc`'s no-op
        // null-brush path isn't reached either.
        None => return Some(1),
    };

    // `try_borrow`, not `borrow`: this message is not only posted by the
    // system, it is also sent *synchronously by `BeginPaint`* when the
    // update region was invalidated with erasing requested -- and
    // `wm_paint` calls `BeginPaint` while holding `borrow_mut()` on this
    // same `RefCell`. A plain `borrow()` would panic there. Nothing is lost
    // by skipping the fill in that case: we are already inside a paint
    // cycle that is about to produce a real frame.
    let inner = match inner.try_borrow() {
        Ok(inner) => inner,
        Err(_) => return Some(1),
    };

    if let Some(brush) = inner.placeholder_background_brush {
        let hdc = wparam as HDC;
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        // SAFETY: `hwnd` is the valid window handle passed in; `rect` is a
        // live stack `RECT` that `GetClientRect` only writes into.
        GetClientRect(hwnd, &mut rect);
        // SAFETY: `hdc` comes from `wparam` of a real `WM_ERASEBKGND`
        // message and is therefore a valid device context for this window;
        // `brush` is a live `HBRUSH` owned by `inner` (created in
        // `create_placeholder_background_brush`, deleted only in
        // `clear_placeholder_background`/`wm_ncdestroy`) and `rect` is the
        // just-populated client rect.
        FillRect(hdc, &rect, brush);
    }

    Some(1)
}

fn mods_and_buttons(wparam: WPARAM) -> (Modifiers, MouseButtons) {
    let mut modifiers = Modifiers::default();
    let mut buttons = MouseButtons::default();
    if wparam & MK_CONTROL != 0 {
        modifiers |= Modifiers::CTRL;
    }
    if wparam & MK_SHIFT != 0 {
        modifiers |= Modifiers::SHIFT;
    }
    // SAFETY: `GetKeyState` takes a plain virtual-key-code value (`VK_MENU`)
    // and returns the key's current thread-message-queue state; no pointers
    // are involved.
    if unsafe { GetKeyState(VK_MENU) } as u16 & 0x8000 != 0 {
        modifiers |= Modifiers::ALT;
    }
    if wparam & MK_LBUTTON != 0 {
        buttons |= MouseButtons::LEFT;
    }
    if wparam & MK_MBUTTON != 0 {
        buttons |= MouseButtons::MIDDLE;
    }
    if wparam & MK_RBUTTON != 0 {
        buttons |= MouseButtons::RIGHT;
    }
    // TODO: XBUTTON1 and XBUTTON2?
    (modifiers, buttons)
}

fn mouse_coords(lparam: LPARAM) -> Point {
    let point = MAKEPOINTS(lparam as _);
    Point::new(point.x as _, point.y as _)
}

fn nc_mouse_coords(hwnd: HWND, lparam: LPARAM) -> Point {
    let point = MAKEPOINTS(lparam as _);
    let point = ScreenPoint::new(point.x as _, point.y as _);
    screen_to_client(hwnd, point)
}

fn screen_to_client(hwnd: HWND, point: ScreenPoint) -> Point {
    let mut point = POINT {
        x: point.x.try_into().unwrap(),
        y: point.y.try_into().unwrap(),
    };
    // SAFETY: `hwnd` is a valid window handle and `point` is a live `POINT`.
    unsafe { ScreenToClient(hwnd, &mut point as *mut _) };
    Point::new(point.x.try_into().unwrap(), point.y.try_into().unwrap())
}

fn client_to_screen(hwnd: HWND, point: Point) -> ScreenPoint {
    let mut point = POINT {
        x: point.x.try_into().unwrap(),
        y: point.y.try_into().unwrap(),
    };
    // SAFETY: `hwnd` is a valid window handle and `point` is a live `POINT`.
    unsafe { ClientToScreen(hwnd, &mut point as *mut _) };
    ScreenPoint::new(point.x.try_into().unwrap(), point.y.try_into().unwrap())
}

fn apply_mouse_cursor(cursor: Option<MouseCursor>) {
    match cursor {
        // SAFETY: passing a null cursor simply resets to the default; no args.
        None => unsafe {
            SetCursor(null_mut());
        },
        // SAFETY: null instance loads a system (OCR_*) cursor; the matched
        // `IDC_*` constants are all valid system cursor identifiers.
        Some(cursor) => unsafe {
            SetCursor(LoadCursorW(
                null_mut(),
                match cursor {
                    MouseCursor::Arrow => IDC_ARROW,
                    MouseCursor::Hand => IDC_HAND,
                    MouseCursor::Text => IDC_IBEAM,
                    MouseCursor::SizeUpDown => IDC_SIZENS,
                    MouseCursor::SizeLeftRight => IDC_SIZEWE,
                },
            ));
        },
    }
}

/// # Safety
/// `hwnd` must be a valid window handle and `msg`/`wparam`/`lparam` the values
/// from a real client-area mouse-button message.
unsafe fn mouse_button(hwnd: HWND, msg: UINT, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
    let inner = rc_from_hwnd(hwnd)?;
    // To support dragging the window, capture when the left
    // button goes down and release when it goes up.
    // Without this, the drag state can be confused when dragging
    // the mouse up outside of the client area.
    if msg == WM_LBUTTONDOWN {
        SetCapture(hwnd);
    } else if msg == WM_LBUTTONUP {
        ReleaseCapture();
    }
    let (modifiers, mouse_buttons) = mods_and_buttons(wparam);
    let coords = mouse_coords(lparam);
    let event = MouseEvent {
        kind: match msg {
            WM_LBUTTONDOWN => MouseEventKind::Press(MousePress::Left),
            WM_LBUTTONUP => MouseEventKind::Release(MousePress::Left),
            WM_RBUTTONDOWN => MouseEventKind::Press(MousePress::Right),
            WM_RBUTTONUP => MouseEventKind::Release(MousePress::Right),
            WM_MBUTTONDOWN => MouseEventKind::Press(MousePress::Middle),
            WM_MBUTTONUP => MouseEventKind::Release(MousePress::Middle),
            _ => return None,
        },
        coords,
        screen_coords: client_to_screen(hwnd, coords),
        mouse_buttons,
        modifiers,
    };
    inner
        .borrow_mut()
        .events
        .dispatch(WindowEvent::MouseEvent(event));
    Some(0)
}

/// # Safety
/// `hwnd` must be a valid window handle and the args from a real
/// non-client mouse-button message.
unsafe fn nc_mouse_button(
    hwnd: HWND,
    msg: UINT,
    wparam: WPARAM,
    lparam: LPARAM,
) -> Option<LRESULT> {
    let inner = rc_from_hwnd(hwnd)?;

    let no_native_title_bar = no_native_title_bar(inner.borrow().config.window_decorations);
    if !no_native_title_bar {
        // Don't mess with this event unless we're doing our own custom
        // titlebar
        return None;
    }

    // To support dragging the window, capture when the left
    // button goes down and release when it goes up.
    // Without this, the drag state can be confused when dragging
    // the mouse up outside of the client area.

    if msg == WM_LBUTTONDOWN {
        SetCapture(hwnd);
    } else if msg == WM_LBUTTONUP {
        ReleaseCapture();
    }

    if wparam != HTMAXBUTTON as usize {
        return None;
    }

    let (modifiers, mouse_buttons) = mods_and_buttons(0);
    let coords = nc_mouse_coords(hwnd, lparam);

    let event = MouseEvent {
        kind: match msg {
            WM_NCLBUTTONDOWN | WM_NCLBUTTONDBLCLK => MouseEventKind::Press(MousePress::Left),
            _ => return None,
        },
        coords,
        screen_coords: client_to_screen(hwnd, coords),
        mouse_buttons,
        modifiers,
    };
    inner
        .borrow_mut()
        .events
        .dispatch(WindowEvent::MouseEvent(event));
    Some(0)
}

/// # Safety
/// `hwnd` must be a valid window handle and `wparam`/`lparam` from a real
/// `WM_MOUSEMOVE` message.
unsafe fn mouse_move(hwnd: HWND, _msg: UINT, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
    let inner = rc_from_hwnd(hwnd)?;
    let mut inner = inner.borrow_mut();

    if !inner.track_mouse_leave {
        inner.track_mouse_leave = true;

        let mut trk = TRACKMOUSEEVENT {
            cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
            dwFlags: TME_LEAVE,
            hwndTrack: hwnd,
            dwHoverTime: 0,
        };

        inner.track_mouse_leave = TrackMouseEvent(&mut trk) == winapi::shared::minwindef::TRUE;
    }

    let (modifiers, mouse_buttons) = mods_and_buttons(wparam);
    let coords = mouse_coords(lparam);
    let event = MouseEvent {
        kind: MouseEventKind::Move,
        coords,
        screen_coords: client_to_screen(hwnd, coords),
        mouse_buttons,
        modifiers,
    };

    inner.events.dispatch(WindowEvent::MouseEvent(event));
    Some(0)
}

/// # Safety
/// `hwnd` must be a valid window handle and the args from a real non-client
/// `WM_NCMOUSEMOVE` message.
unsafe fn nc_mouse_move(hwnd: HWND, _msg: UINT, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
    let inner = rc_from_hwnd(hwnd)?;
    let mut inner = inner.borrow_mut();

    if !inner.track_mouse_leave {
        inner.track_mouse_leave = true;

        let mut trk = TRACKMOUSEEVENT {
            cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
            dwFlags: TME_LEAVE | TME_NONCLIENT,
            hwndTrack: hwnd,
            dwHoverTime: 0,
        };

        inner.track_mouse_leave = TrackMouseEvent(&mut trk) == winapi::shared::minwindef::TRUE;
    }

    if wparam != HTMAXBUTTON as usize {
        return None;
    }

    let (modifiers, mouse_buttons) = mods_and_buttons(0);
    let coords = nc_mouse_coords(hwnd, lparam);

    let event = MouseEvent {
        kind: MouseEventKind::Move,
        coords,
        screen_coords: client_to_screen(hwnd, coords),
        mouse_buttons,
        modifiers,
    };

    inner.events.dispatch(WindowEvent::MouseEvent(event));
    inner.events.dispatch(WindowEvent::NeedRepaint);

    Some(0)
}

/// # Safety
/// `hwnd` must be a valid window handle.
unsafe fn mouse_leave(hwnd: HWND, _msg: UINT, _wparam: WPARAM, _lparam: LPARAM) -> Option<LRESULT> {
    let inner = rc_from_hwnd(hwnd)?;
    let mut inner = inner.borrow_mut();

    inner.track_mouse_leave = false;
    inner.events.dispatch(WindowEvent::MouseLeave);

    Some(0)
}

lazy_static! {
    static ref WHEEL_SCROLL_LINES: i16 = read_scroll_speed("WheelScrollLines").unwrap_or(3);
    static ref WHEEL_SCROLL_CHARS: i16 = read_scroll_speed("WheelScrollChars").unwrap_or(3);
}

fn read_scroll_speed(name: &str) -> io::Result<i16> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let desktop = hkcu.open_subkey("Control Panel\\Desktop")?;
    desktop
        .get_value::<String, _>(name)
        .and_then(|v| v.parse().map_err(|_| io::ErrorKind::InvalidData.into()))
}

/// # Safety
/// `hwnd` must be a valid window handle and the args from a real mouse-wheel
/// message.
unsafe fn mouse_wheel(hwnd: HWND, msg: UINT, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
    let inner = rc_from_hwnd(hwnd)?;
    let (modifiers, mouse_buttons) = mods_and_buttons(wparam);
    // Wheel events return screen coordinates!
    let coords = mouse_coords(lparam);
    let screen_coords = ScreenPoint::new(coords.x, coords.y);
    let coords = screen_to_client(hwnd, screen_coords);
    let delta = GET_WHEEL_DELTA_WPARAM(wparam);
    let scaled_delta = if msg == WM_MOUSEWHEEL {
        delta * (*WHEEL_SCROLL_LINES)
    } else {
        delta * (*WHEEL_SCROLL_CHARS)
    };
    let mut position = scaled_delta / WHEEL_DELTA;
    let remainder = scaled_delta % WHEEL_DELTA;
    let event = MouseEvent {
        kind: if msg == WM_MOUSEHWHEEL {
            let mut inner = inner.borrow_mut();
            if inner.hscroll_remainder.signum() != remainder.signum() {
                // Reset remainder when changing scroll direction
                inner.hscroll_remainder = 0;
            }
            inner.hscroll_remainder += remainder;
            position += inner.hscroll_remainder / WHEEL_DELTA;
            inner.hscroll_remainder %= WHEEL_DELTA;
            log::trace!(
                "mouse_hwheel delta={} scaled={} remainder={} pos={}",
                delta,
                scaled_delta,
                inner.hscroll_remainder,
                position
            );
            if position == 0 {
                return Some(0);
            }
            MouseEventKind::HorzWheel(position)
        } else {
            let mut inner = inner.borrow_mut();
            if inner.vscroll_remainder.signum() != remainder.signum() {
                // Reset remainder when changing scroll direction
                inner.vscroll_remainder = 0;
            }
            inner.vscroll_remainder += remainder;
            position += inner.vscroll_remainder / WHEEL_DELTA;
            inner.vscroll_remainder %= WHEEL_DELTA;
            log::trace!(
                "mouse_wheel delta={} scaled={} remainder={} pos={}",
                delta,
                scaled_delta,
                inner.vscroll_remainder,
                position
            );
            if position == 0 {
                return Some(0);
            }
            MouseEventKind::VertWheel(position)
        },
        coords,
        screen_coords,
        mouse_buttons,
        modifiers,
    };
    inner
        .borrow_mut()
        .events
        .dispatch(WindowEvent::MouseEvent(event));
    Some(0)
}

/// Helper for managing the IME Manager
struct ImmContext {
    hwnd: HWND,
    imc: HIMC,
}

impl ImmContext {
    /// Obtain the IMM context; it will be released automatically
    /// when dropped
    pub fn get(hwnd: HWND) -> Self {
        Self {
            hwnd,
            // SAFETY: `hwnd` is a valid window handle; `ImmGetContext` returns an
            // HIMC that is released in `Drop`.
            imc: unsafe { ImmGetContext(hwnd) },
        }
    }

    /// Set the position of the IME candidate window relative to the cursor.
    pub fn set_candidate_window_position(&self, cursor: Rect) {
        let mut cf = CANDIDATEFORM {
            dwIndex: 0,
            // Don't draw the IME candidate window on the cursor
            // to prevent the window from hiding composition (preedit) string
            dwStyle: CFS_EXCLUDE,
            // cursor position the IME candidate window bases on
            ptCurrentPos: POINT {
                x: cursor.origin.x.max(0) as i32,
                y: cursor.origin.y.max(0) as i32,
            },
            // cursor rectangle the IME candidate window excludes
            rcArea: RECT {
                left: cursor.min_x().max(0) as i32,
                top: cursor.min_y().max(0) as i32,
                right: cursor.max_x().max(0) as i32,
                bottom: cursor.max_y().max(0) as i32,
            },
        };
        // SAFETY: `self.imc` is a valid HIMC and `cf` is a fully-initialized
        // `CANDIDATEFORM`.
        unsafe {
            ImmSetCandidateWindow(self.imc, &mut cf);
        }
    }

    /// Set the position of the IME composition window relative to the cursor.
    pub fn set_composition_window_position(&self, cursor: Rect) {
        let mut cf = COMPOSITIONFORM {
            dwStyle: CFS_POINT,
            ptCurrentPos: POINT {
                x: cursor.origin.x.max(0) as i32,
                y: cursor.origin.y.max(0) as i32,
            },
            rcArea: RECT::default(),
        };
        // SAFETY: `self.imc` is a valid HIMC and `cf` is a fully-initialized
        // `COMPOSITIONFORM`.
        unsafe {
            ImmSetCompositionWindow(self.imc, &mut cf);
        }
    }

    pub fn get_str(&self, which: DWORD) -> Result<String, OsString> {
        // This returns a size in bytes even though it is for a buffer of u16!
        // SAFETY: a null buffer/zero size queries the byte length without writing.
        let byte_size =
            unsafe { ImmGetCompositionStringW(self.imc, which, std::ptr::null_mut(), 0) };
        if byte_size > 0 {
            let word_size = byte_size as usize / 2;
            let mut wide_buf = vec![0u16; word_size];
            // SAFETY: `wide_buf` holds `word_size` `u16`s and `byte_size` matches
            // the queried length, so the write is in-bounds; `self.imc` is valid.
            unsafe {
                ImmGetCompositionStringW(
                    self.imc,
                    which,
                    wide_buf.as_mut_ptr() as *mut _,
                    byte_size as u32,
                )
            };
            OsString::from_wide(&wide_buf).into_string()
        } else {
            Ok(String::new())
        }
    }
}

impl Drop for ImmContext {
    fn drop(&mut self) {
        // SAFETY: `self.hwnd`/`self.imc` are the valid pair obtained in `get`;
        // `ImmReleaseContext` releases the context exactly once.
        unsafe {
            ImmReleaseContext(self.hwnd, self.imc);
        }
    }
}

/// # Safety
/// `hwnd` must be a valid window handle and the args from a real
/// `WM_IME_SETCONTEXT` message.
unsafe fn ime_set_context(
    hwnd: HWND,
    msg: UINT,
    wparam: WPARAM,
    lparam: LPARAM,
) -> Option<LRESULT> {
    let use_system_rendering = {
        let inner = rc_from_hwnd(hwnd)?;
        let inner = inner.borrow();
        inner.config.ime_preedit_rendering == ImePreeditRendering::System
    };

    if use_system_rendering {
        return None;
    }

    // Don't show system CompositionWindow because application itself draws it.
    // Note: DefWindowProcW may trigger other window messages, so we must
    // release the borrow before calling it.
    let lparam = lparam & !(ISC_SHOWUICOMPOSITIONWINDOW as LPARAM);
    let result = DefWindowProcW(hwnd, msg, wparam, lparam);
    Some(result)
}

/// # Safety
/// `hwnd` must be a valid window handle and the args from a real
/// `WM_IME_ENDCOMPOSITION` message.
unsafe fn ime_end_composition(
    hwnd: HWND,
    _msg: UINT,
    _wparam: WPARAM,
    _lparam: LPARAM,
) -> Option<LRESULT> {
    // IME was cancelled
    let inner = rc_from_hwnd(hwnd)?;
    let mut inner = inner.borrow_mut();

    if inner.config.ime_preedit_rendering == ImePreeditRendering::System {
        return None;
    }

    inner
        .events
        .dispatch(WindowEvent::AdviseDeadKeyStatus(DeadKeyStatus::None));
    Some(1)
}

/// # Safety
/// `hwnd` must be a valid window handle and the args from a real
/// `WM_IME_COMPOSITION` message.
unsafe fn ime_composition(
    hwnd: HWND,
    _msg: UINT,
    _wparam: WPARAM,
    lparam: LPARAM,
) -> Option<LRESULT> {
    let inner = rc_from_hwnd(hwnd)?;
    let mut inner = inner.borrow_mut();

    if inner.config.ime_preedit_rendering == ImePreeditRendering::System {
        return None;
    }

    let imc = ImmContext::get(hwnd);

    let lparam = lparam as DWORD;

    if lparam == 0 {
        // IME was cancelled
        inner
            .events
            .dispatch(WindowEvent::AdviseDeadKeyStatus(DeadKeyStatus::None));
        return Some(1);
    }

    if lparam & GCS_RESULTSTR == 0 {
        // No finished result; continue with the default
        // processing
        if let Ok(composing) = imc.get_str(GCS_COMPSTR) {
            inner
                .events
                .dispatch(WindowEvent::AdviseDeadKeyStatus(DeadKeyStatus::Composing(
                    composing,
                )));
        }
        // We will show the composing string ourselves.
        // Suppress the default composition display.
        return Some(1);
    }

    match imc.get_str(GCS_RESULTSTR) {
        Ok(s) if !s.is_empty() => {
            let key = KeyEvent {
                key: KeyCode::Composed(s),
                modifiers: Modifiers::NONE,
                leds: KeyboardLedStatus::empty(),
                repeat_count: 1,
                key_is_down: true,
                raw: None,
                win32_uni_char: None,
            };
            inner
                .events
                .dispatch(WindowEvent::AdviseDeadKeyStatus(DeadKeyStatus::None));
            inner.events.dispatch(WindowEvent::KeyEvent(key));

            return Some(1);
        }
        Ok(_) => {}
        Err(_) => eprintln!("cannot represent IME as unicode string!?"),
    };
    None
}

/// Holds information about the current keyboard layout.
/// This is used to determine whether the layout includes
/// an AltGr key or just has a regular Right-Alt key,
/// as well as to build out information about dead keys.
struct KeyboardLayoutInfo {
    layout: HKL,
    has_alt_gr: bool,
    dead_keys: HashMap<(Modifiers, u8), DeadKey>,
}

#[derive(Debug)]
struct DeadKey {
    dead_char: char,
    _vk: u8,
    _mods: Modifiers,
    map: HashMap<(Modifiers, u8), char>,
}

#[derive(Debug)]
enum ResolvedDeadKey {
    InvalidDeadKey,
    Combined(char),
    InvalidCombination(char),
}

impl KeyboardLayoutInfo {
    pub fn new() -> Self {
        Self {
            layout: std::ptr::null_mut(),
            has_alt_gr: false,
            dead_keys: HashMap::new(),
        }
    }

    /// # Safety
/// Calls `keybd_event` with synthesized key state; only valid VK/scan codes are
/// produced from `ToAsciiEx` results.
unsafe fn clear_key_state() {
        let mut out = [0u16; 16];
        let state = [0u8; 256];
        let scan = MapVirtualKeyW(VK_DECIMAL as _, MAPVK_VK_TO_VSC);
        // keep clocking the state to clear out its effects
        while ToUnicode(
            VK_DECIMAL as _,
            scan,
            state.as_ptr(),
            out.as_mut_ptr(),
            out.len() as i32,
            0,
        ) < 0
        {}
    }

    /// Probe to detect whether an AltGr key is present.
    /// This is done by synthesizing a keyboard state with control and alt
    /// pressed and then testing the virtual key presses.  If we find that
    /// one of these yields a single unicode character output then we assume that
    /// it does have AltGr.
    /// # Safety
    /// `KeyboardLayoutInfo` holds no window handle - the underlying `ToUnicode`/
    /// `MapVirtualKeyW` calls only take VK/scan-code values, all of which are
    /// constructed locally and valid. The real precondition is that this reads
    /// and mutates *thread-global* keyboard state (`GetKeyboardState`/
    /// `ToUnicode`), so it must only be called from the UI thread that owns
    /// the keyboard focus, never concurrently from multiple threads.
unsafe fn probe_alt_gr(&mut self) {
        self.has_alt_gr = false;

        let mut state = [0u8; 256];
        state[VK_CONTROL as usize] = 0x80;
        state[VK_MENU as usize] = 0x80;

        for vk in 0..=255u32 {
            if vk == VK_PACKET as u32 {
                // Avoid false positives
                continue;
            }

            let mut out = [0u16; 16];
            let ret = ToUnicode(vk, 0, state.as_ptr(), out.as_mut_ptr(), out.len() as i32, 0);
            if ret == 1 {
                self.has_alt_gr = true;
                break;
            }

            if ret == -1 {
                // Dead key.
                // keep clocking the state to clear out its effects
                while ToUnicode(vk, 0, state.as_ptr(), out.as_mut_ptr(), out.len() as i32, 0) < 0 {}
            }
        }
    }

    fn apply_mods(mods: Modifiers, state: &mut [u8; 256]) {
        if mods.contains(Modifiers::SHIFT) {
            state[VK_SHIFT as usize] = 0x80;
        }
        if mods.contains(Modifiers::CTRL) || mods.contains(Modifiers::RIGHT_ALT) {
            state[VK_CONTROL as usize] = 0x80;
        }
        if mods.contains(Modifiers::RIGHT_ALT) || mods.contains(Modifiers::ALT) {
            state[VK_MENU as usize] = 0x80;
        }
    }

    /// Probe the keymap to figure out which keys are dead keys
    /// # Safety
    /// Same rationale as `probe_alt_gr` above: no window handle is involved,
    /// but this reads/mutates thread-global keyboard state and must only run
    /// on the UI thread, not concurrently with other callers of these APIs.
unsafe fn probe_dead_keys(&mut self) {
        self.dead_keys.clear();

        let shift_states = [
            Modifiers::NONE,
            Modifiers::SHIFT,
            Modifiers::SHIFT | Modifiers::CTRL,
            Modifiers::ALT,
            Modifiers::RIGHT_ALT, // AltGr
        ];

        for &mods in &shift_states {
            let mut state = [0u8; 256];
            Self::apply_mods(mods, &mut state);

            for vk in 0..=255u32 {
                if vk == VK_PACKET as u32 {
                    // Avoid false positives
                    continue;
                }

                let scan = MapVirtualKeyW(vk, MAPVK_VK_TO_VSC);

                Self::clear_key_state();
                let mut out = [0u16; 16];
                let ret = ToUnicode(
                    vk,
                    scan,
                    state.as_ptr(),
                    out.as_mut_ptr(),
                    out.len() as i32,
                    0,
                );

                if ret != -1 {
                    continue;
                }

                // Found a Dead key.
                let dead_char = std::char::from_u32_unchecked(out[0] as u32);

                let mut map = HashMap::new();

                for &smod in &shift_states {
                    let mut second_state = [0u8; 256];
                    Self::apply_mods(smod, &mut second_state);

                    for ik in 0..=255u32 {
                        // Re-initiate the dead key starting state
                        Self::clear_key_state();
                        if ToUnicode(
                            vk,
                            scan,
                            state.as_ptr(),
                            out.as_mut_ptr(),
                            out.len() as i32,
                            0,
                        ) != -1
                        {
                            continue;
                        }

                        let scan = MapVirtualKeyW(ik, MAPVK_VK_TO_VSC);

                        let ret = ToUnicode(
                            ik,
                            scan,
                            second_state.as_ptr(),
                            out.as_mut_ptr(),
                            out.len() as i32,
                            0,
                        );

                        if ret == 1 {
                            // Found a combination
                            let c = std::char::from_u32_unchecked(out[0] as u32);
                            // clock through again to get the base
                            ToUnicode(
                                ik,
                                scan,
                                second_state.as_ptr(),
                                out.as_mut_ptr(),
                                out.len() as i32,
                                0,
                            );
                            let base = std::char::from_u32_unchecked(out[0] as u32);

                            if ((smod == Modifiers::CTRL)
                                || (smod == Modifiers::CTRL | Modifiers::SHIFT))
                                && c == base
                                && (c as u32) < 0x20
                            {
                                continue;
                            }

                            log::trace!(
                                "{:?}: {:?} {:?} + {:?} {:?} -> {:?} base={:?}",
                                dead_char,
                                mods,
                                vk,
                                smod,
                                ik,
                                c,
                                base
                            );

                            map.insert((smod, ik as u8), c);
                        }
                    }
                }

                self.dead_keys.insert(
                    (mods, vk as u8),
                    DeadKey {
                        dead_char,
                        _mods: mods,
                        _vk: vk as u8,
                        map,
                    },
                );
            }
        }
        Self::clear_key_state();
    }

    /// # Safety
    /// Same rationale as `probe_alt_gr`/`probe_dead_keys`: no window handle is
    /// involved; this must only run on the UI thread since it reads/mutates
    /// thread-global keyboard state.
unsafe fn update(&mut self) {
        let current_layout = GetKeyboardLayout(0);
        if current_layout == self.layout {
            // Avoid recomputing this if the layout hasn't changed
            return;
        }

        let mut saved_state = [0u8; 256];
        if GetKeyboardState(saved_state.as_mut_ptr()) == 0 {
            return;
        }

        self.probe_alt_gr();
        self.probe_dead_keys();
        log::trace!("dead_keys: {:#x?}", self.dead_keys);

        SetKeyboardState(saved_state.as_mut_ptr());
        self.layout = current_layout;
    }

    pub fn has_alt_gr(&mut self) -> bool {
        // SAFETY: called from the UI thread (window event handling), per
        // `update`'s documented thread-affinity requirement.
        unsafe {
            self.update();
        }
        self.has_alt_gr
    }

    /// Similar to Modifiers::remove_positional_mods except that it preserves
    /// RIGHT_ALT
    fn fixup_mods(mods: Modifiers) -> Modifiers {
        mods - (Modifiers::LEFT_SHIFT
            | Modifiers::RIGHT_SHIFT
            | Modifiers::LEFT_CTRL
            | Modifiers::RIGHT_CTRL
            | Modifiers::LEFT_ALT)
    }

    pub fn is_dead_key_leader(&mut self, mods: Modifiers, vk: u32) -> Option<char> {
        // SAFETY: called from the UI thread (window event handling), per
        // `update`'s documented thread-affinity requirement.
        unsafe {
            self.update();
        }
        if vk <= (u8::MAX as u32) {
            self.dead_keys
                .get(&(Self::fixup_mods(mods), vk as u8))
                .map(|dead| dead.dead_char)
        } else {
            None
        }
    }

    pub fn resolve_dead_key(
        &mut self,
        leader: (Modifiers, u32),
        key: (Modifiers, u32),
    ) -> ResolvedDeadKey {
        // SAFETY: called from the UI thread (window event handling), per
        // `update`'s documented thread-affinity requirement.
        unsafe {
            self.update();
        }
        if leader.1 <= (u8::MAX as u32) && key.1 <= (u8::MAX as u32) {
            if let Some(dead) = self
                .dead_keys
                .get(&(Self::fixup_mods(leader.0), leader.1 as u8))
            {
                if let Some(c) = dead
                    .map
                    .get(&(Self::fixup_mods(key.0), key.1 as u8))
                    .map(|&c| c)
                {
                    ResolvedDeadKey::Combined(c)
                } else {
                    ResolvedDeadKey::InvalidCombination(dead.dead_char)
                }
            } else {
                ResolvedDeadKey::InvalidDeadKey
            }
        } else {
            ResolvedDeadKey::InvalidDeadKey
        }
    }
}

/// Generate a MSG and call TranslateMessage upon it
/// # Safety
/// `hwnd` must be a valid window handle and the args from a real key message.
unsafe fn translate_message(hwnd: HWND, msg: UINT, wparam: WPARAM, lparam: LPARAM) {
    TranslateMessage(&MSG {
        hwnd,
        message: msg,
        wParam: wparam,
        lParam: lparam,
        pt: POINT { x: 0, y: 0 },
        time: GetTickCount(),
    });
}

/// # Safety
/// `hwnd` must be a valid window handle and the args from a real key message.
unsafe fn key(hwnd: HWND, msg: UINT, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
    let inner = rc_from_hwnd(hwnd)?;
    let mut inner = inner.borrow_mut();
    let repeat = (lparam & 0xffff) as u16;
    let scan_code = ((lparam >> 16) & 0xff) as u8;
    let releasing = (lparam & (1 << 31)) != 0;
    let ime_active = wparam == VK_PROCESSKEY as WPARAM;
    let phys_code = super::keycodes::vkey_to_phys(wparam);

    let alt_pressed = (lparam & (1 << 29)) != 0;
    let is_extended = (lparam & (1 << 24)) != 0;
    let was_down = (lparam & (1 << 30)) != 0;
    let label = match msg {
        WM_CHAR => "WM_CHAR",
        WM_IME_CHAR => "WM_IME_CHAR",
        WM_KEYDOWN => "WM_KEYDOWN",
        WM_KEYUP => "WM_KEYUP",
        WM_SYSKEYUP => "WM_SYSKEYUP",
        WM_SYSKEYDOWN => "WM_SYSKEYDOWN",
        WM_SYSCHAR => "WM_SYSCHAR",
        WM_DEADCHAR => "WM_DEADCHAR",
        _ => "WAT",
    };
    log::trace!(
        "{} c=`{}` repeat={} scan={} is_extended={} alt_pressed={} was_down={} \
             releasing={} IME={} dead_pending={:?}",
        label,
        wparam,
        repeat,
        scan_code,
        is_extended,
        alt_pressed,
        was_down,
        releasing,
        ime_active,
        inner.dead_pending,
    );

    if ime_active {
        // If the IME is active, allow Windows to perform default processing
        // to drive it forwards.  It will generate a call to `ime_composition`
        // or `ime_endcomposition` when it completes.

        if msg == WM_KEYDOWN {
            // Release the borrow before calling translate_message:
            // TranslateMessage can trigger other window messages (like WM_SIZE)
            // via CtfImeCreateInputContext, which would otherwise cause a
            // RefCell borrow conflict while inner is still borrowed.
            drop(inner);
            // Explicitly allow the built-in translation to occur for the IME
            translate_message(hwnd, msg, wparam, lparam);
            return Some(0);
        }

        return None;
    }

    if msg == WM_DEADCHAR {
        // Ignore WM_DEADCHAR; we only care about the resultant WM_CHAR
        return Some(0);
    }

    let keys = {
        let mut keys = [0u8; 256];
        GetKeyboardState(keys.as_mut_ptr());
        keys
    };

    let mut modifiers = Modifiers::default();
    if keys[VK_SHIFT as usize] & 0x80 != 0 {
        modifiers |= Modifiers::SHIFT;
    }
    if keys[VK_LSHIFT as usize] & 0x80 != 0 {
        modifiers |= Modifiers::LEFT_SHIFT;
    }
    if keys[VK_RSHIFT as usize] & 0x80 != 0 {
        modifiers |= Modifiers::RIGHT_SHIFT;
    }
    if keys[VK_LCONTROL as usize] & 0x80 != 0 {
        modifiers |= Modifiers::LEFT_CTRL;
    }
    if keys[VK_RCONTROL as usize] & 0x80 != 0 {
        modifiers |= Modifiers::RIGHT_CTRL;
    }
    modifiers.set(Modifiers::ENHANCED_KEY, is_extended);

    if inner.keyboard_info.has_alt_gr()
        && (keys[VK_RMENU as usize] & 0x80 != 0)
        && (keys[VK_CONTROL as usize] & 0x80 != 0)
    {
        // AltGr is pressed; while AltGr is on the RHS of the keyboard
        // is not the same thing as right-alt.
        // Windows sets RMENU and CONTROL to indicate AltGr and we
        // have to keep these in the key state in order for ToUnicode
        // to map the key correctly.
        // We set RIGHT_ALT as a hint to ourselves that AltGr is in
        // use (we use regular ALT otherwise) so that our dead key
        // resolution can do the right thing.
        modifiers |= Modifiers::RIGHT_ALT;
    } else if inner.keyboard_info.has_alt_gr()
        && inner.config.treat_left_ctrlalt_as_altgr
        && (keys[VK_MENU as usize] & 0x80 != 0)
        && (keys[VK_CONTROL as usize] & 0x80 != 0)
    {
        // When running inside a VNC session, VNC emulates the AltGr keypresses
        // by sending plain VK_MENU (rather than VK_RMENU) + VK_CONTROL.
        // For compatibility with that the option `treat_left_ctrlalt_as_altgr` allows
        // to treat MENU+CONTROL as equivalent to RMENU+CONTROL (AltGr) even though it is
        // technically a lossy transformation.
        //
        // We only do that when the keyboard layout has AltGr and the option is enabled,
        // so that we don't screw things up by default or for other keyboard layouts.
        // See issue #392 & #472 for some more context.
        modifiers |= Modifiers::RIGHT_ALT;
    } else {
        if keys[VK_CONTROL as usize] & 0x80 != 0 {
            modifiers |= Modifiers::CTRL;
        }
        if keys[VK_MENU as usize] & 0x80 != 0 {
            modifiers |= Modifiers::ALT;
        }
    }
    if keys[VK_LWIN as usize] & 0x80 != 0 || keys[VK_RWIN as usize] & 0x80 != 0 {
        modifiers |= Modifiers::SUPER;
    }

    let mut leds = KeyboardLedStatus::empty();
    if keys[VK_CAPITAL as usize] & 1 != 0 {
        leds |= KeyboardLedStatus::CAPS_LOCK;
    }
    if keys[VK_NUMLOCK as usize] & 1 != 0 {
        leds |= KeyboardLedStatus::NUM_LOCK;
    }

    let handled_raw = Handled::new();
    let raw_key_event = RawKeyEvent {
        key: match phys_code {
            Some(phys) => KeyCode::Physical(phys),
            None => KeyCode::RawCode(wparam as _),
        },
        phys_code,
        raw_code: wparam as _,
        scan_code: scan_code as _,
        leds,
        modifiers,
        repeat_count: 1,
        key_is_down: !releasing,
        handled: handled_raw.clone(),
    };

    let (key, win32_uni_char) = if msg == WM_IME_CHAR || msg == WM_CHAR {
        // If we were sent a character by the IME, some other apps,
        // or by ourselves via TranslateMessage, then take that
        // value as-is.
        (
            Some(KeyCode::Char(std::char::from_u32_unchecked(wparam as u32))),
            None,
        )
    } else {
        // Otherwise we're dealing with a raw key message.
        // ToUnicode has frustrating statefulness so we take care to
        // call it only when we think it will give consistent results.

        inner
            .events
            .dispatch(WindowEvent::RawKeyEvent(raw_key_event.clone()));
        if handled_raw.is_handled() {
            // Cancel any pending dead key
            if inner.dead_pending.take().is_some() {
                inner
                    .events
                    .dispatch(WindowEvent::AdviseDeadKeyStatus(DeadKeyStatus::None));
            }
            log::trace!("raw key was handled; not processing further");
            return Some(0);
        }

        let is_modifier_only = phys_code.map(|p| p.is_modifier()).unwrap_or(false);
        if is_modifier_only {
            // If this event is only modifiers then don't ask the system
            // for further resolution, as we don't want ToUnicode to
            // perturb its inscrutable global state.
            // Modifier-only keypresses are reported as NUL when using win32 input mode.
            (phys_code.map(|p| p.to_key_code()), Some('\x00'))
        } else {
            // If we think this might be a dead key, process it for ourselves.
            // Our KeyboardLayoutInfo struct probed the layout for the key
            // combinations that start a dead key sequence, as well as those
            // that are valid end states for dead keys, so we can resolve
            // these for ourselves in a couple of quick hash lookups.
            let vk = wparam as u32;

            if releasing && inner.dead_pending.is_some() {
                // Don't care about key-up events while processing dead keys
                return Some(0);
            }

            // If we previously had the start of a dead key...
            let dead = if let Some(leader) = inner.dead_pending.take() {
                inner
                    .events
                    .dispatch(WindowEvent::AdviseDeadKeyStatus(DeadKeyStatus::None));
                // look to see how the current event resolves it
                match inner
                    .keyboard_info
                    .resolve_dead_key(leader, (modifiers, vk))
                {
                    // Valid combination produces a single character
                    ResolvedDeadKey::Combined(c) => Some(KeyCode::Char(c)),
                    ResolvedDeadKey::InvalidCombination(c) => {
                        // An invalid combination results in the deferred
                        // keypress triggering the original key first,
                        // and then we process the current key.

                        // Emit an event for the leader of the failed
                        // dead key combination
                        let key = KeyEvent {
                            key: KeyCode::Char(c),
                            modifiers,
                            leds,
                            repeat_count: 1,
                            key_is_down: !releasing,
                            win32_uni_char: Some(c),
                            raw: Some(RawKeyEvent {
                                scan_code: 0,
                                ..raw_key_event.clone()
                            }),
                        }
                        .normalize_shift()
                        .resurface_positional_modifier_key()
                        .normalize_ctrl();

                        inner.events.dispatch(WindowEvent::KeyEvent(key.clone()));

                        // And then we'll perform normal processing on the
                        // current key press
                        if let Some(new_dead_char) =
                            inner.keyboard_info.is_dead_key_leader(modifiers, vk)
                        {
                            if new_dead_char != c {
                                // Happens to be the start of its own new,
                                // different, dead key sequence
                                inner.dead_pending.replace((modifiers, vk));
                                return Some(0);
                            }

                            // They pressed the same dead key twice,
                            // emit the underlying char again and call
                            // it done.
                            // <https://github.com/wezterm/wezterm/issues/1729>
                            inner.events.dispatch(WindowEvent::KeyEvent(key.clone()));
                            return Some(0);
                        }

                        // We don't know; allow normal ToUnicode processing
                        None
                    }

                    // We thought we had a dead key last time around,
                    // but this time it didn't resolve.  Most likely
                    // because the keyboard layout changed in the middle
                    // of the keypress.
                    // We're effectively swallowing the original dead
                    // key event here, but we could potentially re-process
                    // the original and current one here if needed.
                    // Seems like a real edge case.
                    ResolvedDeadKey::InvalidDeadKey => None,
                }
            } else if let Some(c) = inner.keyboard_info.is_dead_key_leader(modifiers, vk) {
                if releasing {
                    // Don't care about key-up events while processing dead keys
                    return Some(0);
                }

                // They pressed a dead key.
                // If they want dead key processing, then record that and
                // wait for a subsequent keypress.
                if inner.config.use_dead_keys {
                    inner.dead_pending.replace((modifiers, vk));
                    inner.events.dispatch(WindowEvent::AdviseDeadKeyStatus(
                        DeadKeyStatus::Composing(c.to_string()),
                    ));
                    return Some(0);
                }
                // They don't want dead keys; just return the base character
                Some(KeyCode::Char(c))
            } else {
                // Not a dead key as far as we know
                None
            };

            if dead.is_some() {
                (dead, None)
            } else {
                // We get here for the various UP (but not DOWN as we shortcircuit
                // those above) messages.
                // We perform conversion to unicode for ourselves,
                // rather than calling TranslateMessage to do it for us,
                // so that we have tighter control over the key processing.
                let mut out = [0u16; 16];

                let win32_uni_char = {
                    let res = ToUnicode(
                        wparam as u32,
                        scan_code as u32,
                        keys.as_ptr(),
                        out.as_mut_ptr(),
                        out.len() as i32,
                        0,
                    );

                    match res {
                        1 => Some(std::char::from_u32_unchecked(out[0] as u32)),
                        0 => Some('\x00'),
                        _ => None,
                    }
                };

                let mut keys = keys;
                // If control is pressed, clear that out and remember it in our
                // own set of modifiers.
                // We used to also remove shift from this set, but it impacts
                // handling of eg: ctrl+shift+' (which is equivalent to ctrl+" in a US English
                // layout.
                // The shift normalization is now handled by the normalize_shift() method.
                if modifiers.contains(Modifiers::CTRL) {
                    keys[VK_CONTROL as usize] = 0;
                    keys[VK_LCONTROL as usize] = 0;
                    keys[VK_RCONTROL as usize] = 0;
                }

                let res = ToUnicode(
                    wparam as u32,
                    scan_code as u32,
                    keys.as_ptr(),
                    out.as_mut_ptr(),
                    out.len() as i32,
                    0,
                );

                // CTRL and (non-AltGr) ALT combinations are keybinding
                // modifiers, not text input: `modifiers` only ever
                // contains CTRL/ALT here when AltGr was ruled out above
                // (AltGr sets RIGHT_ALT instead), so this can't misfire
                // on eg. a German/French AltGr+letter that legitimately
                // produces a different character. `ToUnicode` still
                // layout-translates the physical key even with a
                // modifier held (eg. physical "V" maps to Cyrillic 'м'
                // under a Russian layout), which then fails to match a
                // keybinding registered against the US-QWERTY letter (eg.
                // `ALT|CTRL+V`). Prefer the layout-independent physical
                // key identity for these so bindings resolve the same
                // way regardless of the active keyboard layout, matching
                // how every other modifier-based terminal shortcut is
                // expected to behave.
                let prefer_physical_for_binding =
                    (modifiers.contains(Modifiers::CTRL) || modifiers.contains(Modifiers::ALT))
                        && phys_code.map(|p| p.to_key_code()).is_some();

                let key = if prefer_physical_for_binding {
                    phys_code.map(|p| p.to_key_code())
                } else {
                    match res {
                        1 => Some(KeyCode::Char(std::char::from_u32_unchecked(out[0] as u32))),
                        // No mapping, so use our raw info
                        0 => {
                            log::trace!(
                                "ToUnicode had no mapping for {:?} wparam={}",
                                phys_code,
                                wparam
                            );
                            phys_code.map(|p| p.to_key_code())
                        }
                        _ => {
                            // dead key: if our dead key mapping in KeyboardLayoutInfo was
                            // correct, we shouldn't be able to get here as we should have
                            // landed in the dead key case above.
                            // If somehow we do get here, we don't have a valid mapping
                            // as -1 indicates the start of a dead key sequence,
                            // and any other n > 1 indicates an ambiguous expansion.
                            // Either way, indicate that we don't have a valid result.
                            log::error!(
                                "unexpected dead key expansion: \
                                 modifiers={:?} vk={:?} res={} releasing={} {:?}",
                                modifiers,
                                vk,
                                res,
                                releasing,
                                out
                            );
                            KeyboardLayoutInfo::clear_key_state();
                            None
                        }
                    }
                };

                (key, win32_uni_char)
            }
        }
    };

    if let Some(key) = key {
        // FIXME: verify this behavior: Urgh, special case for ctrl and non-latin layouts.
        // In order to avoid a situation like #678, if CTRL is the only
        // modifier and we've got composed text, then discard the composed
        // text.
        let key = KeyEvent {
            key,
            modifiers,
            leds,
            repeat_count: repeat,
            key_is_down: !releasing,
            win32_uni_char,
            raw: Some(raw_key_event),
        }
        .normalize_shift();

        // Special case for ALT-space to show the system menu, and
        // ALT-F4 to close the window.
        if key.modifiers == Modifiers::ALT
            && (key.key == KeyCode::Char(' ') || key.key == KeyCode::Function(4))
        {
            translate_message(hwnd, msg, wparam, lparam);
            return None;
        }

        inner.events.dispatch(WindowEvent::KeyEvent(key));
        return Some(0);
    }
    None
}

/// # Safety
/// `hwnd` must be a valid window handle and `wparam` the `HDROP` from a real
/// `WM_DROPFILES` message.
unsafe fn drop_files(hwnd: HWND, _msg: UINT, wparam: WPARAM, _lparam: LPARAM) -> Option<LRESULT> {
    let inner = rc_from_hwnd(hwnd)?;
    let h_drop = wparam as HDROP;

    // Get the number of files dropped
    // SAFETY: `h_drop` is the valid `HDROP` from the message; a null buffer
    // with index 0xFFFFFFFF queries the file count without writing.
    let file_count = DragQueryFileW(h_drop, 0xFFFFFFFF, null_mut(), 0);

    let mut filenames: Vec<PathBuf> = Vec::with_capacity(file_count as usize);

    for idx in 0..file_count {
        // The returned size of buffer is in characters, not including the terminating null character
        // SAFETY: null buffer queries the per-file length without writing.
        let buf_size = DragQueryFileW(h_drop, idx, null_mut(), 0);
        if buf_size > 0 {
            // Windows will truncate the filename and add null terminator if space isn't enough
            let buf_size = buf_size as usize + 1;
            let mut wide_buf = vec![0u16; buf_size];
            // SAFETY: `wide_buf` is large enough for the queried length plus the
            // null terminator; `h_drop` is the valid drop handle.
            DragQueryFileW(h_drop, idx, wide_buf.as_mut_ptr(), wide_buf.len() as u32);
            wide_buf.pop(); // Drops the null terminator
            filenames.push(OsString::from_wide(&wide_buf).into());
        }
    }

    let mut inner = inner.borrow_mut();
    inner.events.dispatch(WindowEvent::DroppedFile(filenames));

    // SAFETY: `h_drop` is the valid drop handle being released once.
    DragFinish(h_drop);
    Some(0)
}

/// # Safety
/// `hwnd` must be a valid window handle and the args the values from the Win32
/// message being dispatched.
unsafe fn do_wnd_proc(hwnd: HWND, msg: UINT, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
    match msg {
        WM_NCCREATE => wm_nccreate(hwnd, msg, wparam, lparam),
        WM_NCDESTROY => wm_ncdestroy(hwnd, msg, wparam, lparam),
        WM_NCCALCSIZE => wm_nccalcsize(hwnd, msg, wparam, lparam),
        WM_NCHITTEST => wm_nchittest(hwnd, msg, wparam, lparam),
        WM_PAINT => wm_paint(hwnd, msg, wparam, lparam),
        WM_ENTERSIZEMOVE | WM_EXITSIZEMOVE => wm_enter_exit_size_move(hwnd, msg, wparam, lparam),
        WM_WINDOWPOSCHANGED => wm_windowposchanged(hwnd, msg, wparam, lparam),
        WM_SETFOCUS => wm_set_focus(hwnd, msg, wparam, lparam),
        WM_KILLFOCUS => wm_kill_focus(hwnd, msg, wparam, lparam),
        WM_DEADCHAR | WM_KEYDOWN | WM_KEYUP | WM_SYSCHAR | WM_CHAR | WM_IME_CHAR | WM_SYSKEYUP
        | WM_SYSKEYDOWN => key(hwnd, msg, wparam, lparam),
        WM_SIZING => {
            // Allow events to be processed during live resize
            crate::spawn::SPAWN_QUEUE.run();
            None
        }
        WM_SETTINGCHANGE | WM_DWMCOMPOSITIONCHANGED => apply_theme(hwnd),
        WM_IME_SETCONTEXT => ime_set_context(hwnd, msg, wparam, lparam),
        WM_IME_COMPOSITION => ime_composition(hwnd, msg, wparam, lparam),
        WM_IME_ENDCOMPOSITION => ime_end_composition(hwnd, msg, wparam, lparam),
        WM_INPUTLANGCHANGEREQUEST => {
            // Handle explicitly: otherwise DefWindowProc deadlocks on keyboard layout switch (upstream #7066)
            let layout = lparam as HKL;
            ActivateKeyboardLayout(layout, KLF_REPLACELANG);
            Some(0)
        },
        WM_MOUSEMOVE => mouse_move(hwnd, msg, wparam, lparam),
        WM_MOUSELEAVE => mouse_leave(hwnd, msg, wparam, lparam),
        WM_MOUSEHWHEEL | WM_MOUSEWHEEL => mouse_wheel(hwnd, msg, wparam, lparam),
        WM_LBUTTONDBLCLK | WM_RBUTTONDBLCLK | WM_MBUTTONDBLCLK | WM_LBUTTONDOWN | WM_LBUTTONUP
        | WM_RBUTTONDOWN | WM_RBUTTONUP | WM_MBUTTONDOWN | WM_MBUTTONUP => {
            mouse_button(hwnd, msg, wparam, lparam)
        }
        WM_DROPFILES => drop_files(hwnd, msg, wparam, lparam),
        WM_ERASEBKGND => wm_erasebkgnd(hwnd, msg, wparam, lparam),
        WM_CLOSE => {
            if let Some(inner) = rc_from_hwnd(hwnd) {
                let mut inner = inner.borrow_mut();
                inner.events.dispatch(WindowEvent::CloseRequested);
                // Don't let it close
                return Some(0);
            }
            None
        }
        _ => {
            if matches!(
                msg,
                WM_NCMOUSEMOVE | WM_NCMOUSELEAVE | WM_NCLBUTTONDOWN | WM_NCLBUTTONDBLCLK
            ) {
                let use_snap_layouts = !*IS_WIN10;
                if use_snap_layouts {
                    return match msg {
                        WM_NCMOUSEMOVE => nc_mouse_move(hwnd, msg, wparam, lparam),
                        WM_NCMOUSELEAVE => mouse_leave(hwnd, msg, wparam, lparam),
                        WM_NCLBUTTONDOWN | WM_NCLBUTTONDBLCLK => {
                            nc_mouse_button(hwnd, msg, wparam, lparam)
                        }
                        _ => None,
                    };
                }
            }

            None
        }
    }
}

/// # Safety
/// This is the `WNDCLASSW::lpfnWndProc` callback: Win32 supplies a valid `hwnd`
/// and the raw message arguments.
unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: UINT,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match std::panic::catch_unwind(|| {
        do_wnd_proc(hwnd, msg, wparam, lparam)
            .unwrap_or_else(|| DefWindowProcW(hwnd, msg, wparam, lparam))
    }) {
        Ok(result) => result,
        Err(e) => {
            log::error!("caught {:?}", e);
            std::process::exit(1)
        }
    }
}

/// The placeholder brush belonging to `child`'s parent top-level window, or
/// `None` once the renderer is up (or if the parent has no `WindowInner`
/// yet). Used by `child_wnd_proc` -- the WebGpu child window has no
/// `WindowInner` of its own, so it borrows its parent's.
///
/// # Safety
/// `child` must be a valid window handle.
unsafe fn parent_placeholder_brush(child: HWND) -> Option<HBRUSH> {
    let parent = GetParent(child);
    if parent.is_null() {
        return None;
    }
    let inner = rc_from_hwnd(parent)?;
    // `try_borrow` for the same reason the top-level `WM_ERASEBKGND`
    // handler uses it: this can be reached synchronously from a
    // `BeginPaint` on a stack frame that already holds the borrow.
    let inner = inner.try_borrow().ok()?;
    inner.placeholder_background_brush
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
            // entire client area. So the parent's own placeholder fill is
            // painted *underneath* it and never visible, while this window
            // paints nothing at all until the swapchain presents its first
            // frame seconds later. That gap is what the user sees as a
            // blank white rectangle. Fill it with the parent's placeholder
            // brush; once the renderer is up, `clear_placeholder_background`
            // drops the brush and this goes back to being a no-op, leaving
            // the swapchain in sole control of these pixels.
            if let Some(brush) = parent_placeholder_brush(hwnd) {
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
                // SAFETY: `hdc` is the device context Win32 passed in
                // `wparam` of a real `WM_ERASEBKGND`; `brush` is owned by
                // the parent's `WindowInner` and outlives this call, which
                // runs on the same (GUI) thread that would destroy it.
                FillRect(hdc, &rect, brush);
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
