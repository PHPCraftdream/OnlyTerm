use self::input::keyboard_layout::KeyboardLayoutInfo;
use self::placeholder::{PlaceholderFade, PlaceholderSpinner};
use super::*;
use crate::connection::ConnectionOps;
use crate::{
    parameters, Appearance, Dimensions, Modifiers, ScreenPoint, ScreenRect, WindowEventSender,
};
use config::ConfigHandle;
use lazy_static::lazy_static;
use onlyterm_color_types::LinearRgba;
use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
    RawWindowHandle, Win32WindowHandle, WindowHandle, WindowsDisplayHandle,
};
use std::any::Any;
use std::cell::RefCell;
use std::num::NonZeroIsize;
use std::ptr::{null, null_mut};
use std::rc::Rc;
use std::sync::Mutex;
use winapi::shared::minwindef::*;
use winapi::shared::ntdef::*;
use winapi::shared::windef::*;
use winapi::um::libloaderapi::GetModuleHandleW;
use winapi::um::shellscalingapi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use winapi::um::sysinfoapi::GetVersionExW;
use winapi::um::winnt::OSVERSIONINFOW;
use winapi::um::winuser::*;
use windows::UI::Color as WUIColor;

mod caption;
mod input;
pub use self::input::ime::*;
mod create;
mod msg;
mod ops;
mod placeholder;

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
    caption: caption::Caption,
    /// Old WebGpu child HWNDs that have been superseded by a renderer
    /// rebuild (see `Window::recreate_webgpu_child_window`) but not yet
    /// `DestroyWindow`-ed, paired with a type-erased `Weak` handle to the
    /// `WebGpuState` (owned by `onlyterm-gui`, which this crate cannot name
    /// directly) whose `wgpu::Surface`/DXGI swapchain targets that HWND.
    ///
    /// Why defer the destroy at all: `begin_renderer_rebuild`
    /// (`onlyterm-gui`) only *signals* the old render thread to shut down
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

    /// Animated spinner state, used only by `wm_paint`/`WM_ERASEBKGND` to
    /// paint the client area before the first real GPU frame lands (task
    /// #384; see `PlaceholderSpinner`'s doc comment). The window class is
    /// registered with `hbrBackground: null_mut()` (see `create_window`) so
    /// that a *working* renderer never gets an extra background erase on
    /// every resize; this exists purely to cover the gap between
    /// `ShowWindow` and the renderer's first frame, where the alternative is
    /// whatever garbage happened to be in that region of the framebuffer, or
    /// (worse, on a dark theme) a stark white flash from an unpainted
    /// client area. Cleared via `clear_placeholder_background` once the
    /// first real frame has actually been *presented* (task #425, hardened
    /// by task #407 -- deliberately later than `TermWindow::created` merely
    /// installing a working `RenderState`, and, when a dedicated render
    /// thread is active (the Windows default), later still than that frame
    /// merely being handed off/enqueued to that thread; see
    /// `WindowOps::clear_placeholder_background`'s doc comment for the
    /// full reasoning and both call sites), at which point the renderer
    /// itself is responsible for every subsequent frame and `WM_ERASEBKGND`
    /// goes back to being a no-op (returning 1 without painting, matching
    /// today's behavior of a null-brush class).
    placeholder_spinner: Option<PlaceholderSpinner>,
    /// Set once `clear_placeholder_background` has run, i.e. a working
    /// `RenderState` is installed and producing frames (task #385's first
    /// gating condition -- see `start_placeholder_fade`). Kept
    /// separate from `placeholder_spinner.is_none()` even though they
    /// currently flip at the same time: this flag specifically means
    /// "renderer ready", not "spinner gone", which matters once the fade
    /// overlay (below) becomes the thing actually keeping the spinner
    /// visible on screen.
    renderer_ready: bool,
    /// Set once this window's pane(s) have produced their first non-empty
    /// pty output (task #385's second gating condition -- proxy for "the
    /// shell is alive and likely to accept input"; see
    /// `WindowOps::notify_shell_ready`'s doc comment for why this specific
    /// signal was chosen over waiting for a harder handshake).
    shell_ready: bool,
    /// The placeholder-overlay fade-out, once started (see `PlaceholderFade`
    /// and `start_placeholder_fade`). `None` before the fade begins
    /// and after it completes and tears itself down.
    placeholder_fade: Option<PlaceholderFade>,
    /// Set by `wm_ncdestroy` the moment it reclaims (via
    /// `take_rc_from_pointer`) the extra `Rc` strong ref that `new_window`
    /// stashed in `GWLP_USERDATA` for `wm_nccreate` to pick up (see
    /// `rc_to_pointer`'s call site in `new_window`).
    ///
    /// Exists to fix a double-free (task #402): `CreateWindowExW` always
    /// sends `WM_NCCREATE` before it can fail, so a *later* failure (e.g.
    /// GDI/USER handle exhaustion) still runs `wm_nccreate` -> stores the
    /// pointer in `GWLP_USERDATA` -> Windows then sends `WM_NCDESTROY` to
    /// unwind the partially-created window -> `wm_ncdestroy` reclaims and
    /// drops that same extra ref via `take_rc_from_pointer`. Without this
    /// flag, `new_window`'s `Err(err)` branch would unconditionally reclaim
    /// the pointer via `Rc::from_raw` too, double-dropping the same strong
    /// reference. When `CreateWindowExW` instead fails *before*
    /// `WM_NCCREATE` ever runs (e.g. a bad class/parent handle), this stays
    /// `false`: `GWLP_USERDATA` was never populated, `wm_ncdestroy` never
    /// runs, and `new_window` remains the sole owner responsible for
    /// dropping the extra ref itself. See `should_new_window_drop_extra_ref`
    /// for the decision this flag feeds and its unit tests for both orders.
    extra_ref_reclaimed_by_ncdestroy: std::cell::Cell<bool>,
}

/// Decides whether `new_window`'s `CreateWindowExW`-failure path must itself
/// reclaim (`Rc::from_raw`) the extra strong ref it stashed in
/// `GWLP_USERDATA`, or whether `wm_ncdestroy` already did so as part of
/// Windows unwinding a partially-constructed window (task #402).
///
/// `already_reclaimed` is `WindowInner::extra_ref_reclaimed_by_ncdestroy`
/// read *after* `create_window` has returned, i.e. after any `WM_NCDESTROY`
/// that `CreateWindowExW` triggered while unwinding has already run
/// synchronously (Win32 delivers `WM_NCDESTROY` to `wnd_proc` before
/// `CreateWindowExW` itself returns). Kept as a free function, independent
/// of any real `HWND`/`Rc`, purely so it is unit-testable without driving
/// actual window creation.
fn should_new_window_drop_extra_ref(already_reclaimed: bool) -> bool {
    !already_reclaimed
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct Window(HWindow);

fn wuicolor_to_linearrgba(color: WUIColor) -> LinearRgba {
    LinearRgba::with_srgba(color.R, color.G, color.B, 255)
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
    let arc = unsafe {
        Rc::from_raw(std::mem::transmute::<LPVOID, *const RefCell<WindowInner>>(
            lparam,
        ))
    };
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
    unsafe {
        Rc::from_raw(std::mem::transmute::<LPVOID, *const RefCell<WindowInner>>(
            lparam,
        ))
    }
}

impl HasDisplayHandle for WindowInner {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
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
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
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

pub(crate) fn get_primary_monitor_dpi() -> u32 {
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
            // Startup-latency diagnostics: see the "startup:" checkpoints in
            // onlyterm-gui's main.rs, which this crate's own checkpoints
            // (here and in `clear_placeholder_background`) chain onto by
            // grep-matching prefix.
            if show == ShowWindowCommand::Normal {
                log::info!("startup: window shown");
            }
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

impl HasDisplayHandle for Window {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
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
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let conn = Connection::get().expect("raw_window_handle only callable on main thread");
        let handle = conn.get_window(self.0).expect("window handle invalid!?");

        let inner = handle.borrow();
        let handle = inner.window_handle()?;
        // SAFETY: `handle` is a valid `Win32WindowHandle` backed by a live `hwnd`
        // kept alive by the owning `Connection`, so borrowing it raw is sound.
        unsafe { Ok(WindowHandle::borrow_raw(handle.as_raw())) }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{Appearance, WindowEventSender};
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    /// Builds a `WindowInner` with the placeholder spinner disabled (`None`)
    /// so the test never touches GDI, only the fields this module's fix
    /// actually cares about.
    fn test_inner() -> Rc<RefCell<WindowInner>> {
        config::use_test_configuration();
        let config = config::configuration();
        Rc::new(RefCell::new(WindowInner {
            hwnd: HWindow(std::ptr::null_mut()),
            webgpu_child_hwnd: HWindow(std::ptr::null_mut()),
            caption: caption::Caption::default(),
            retired_webgpu_children: Vec::new(),
            appearance: Appearance::Light,
            events: WindowEventSender::new(|_, _| {}),
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
            config,
            paint_throttled: false,
            invalidated: true,
            placeholder_spinner: None,
            renderer_ready: false,
            shell_ready: false,
            placeholder_fade: None,
            extra_ref_reclaimed_by_ncdestroy: Cell::new(false),
        }))
    }

    /// Order 1 (the buggy order pre-fix): `CreateWindowExW` fails *after*
    /// `WM_NCCREATE` ran, so Windows also sends `WM_NCDESTROY`, which
    /// (mirroring `wm_ncdestroy`'s real body) reclaims the extra ref via
    /// `take_rc_from_pointer` and sets the flag. `new_window`'s failure path
    /// must then NOT reclaim it again -- doing so would drop the same
    /// strong reference twice.
    #[test]
    fn create_window_failure_after_nccreate_drops_extra_ref_exactly_once() {
        let inner = test_inner();
        // Baseline: the local `inner` binding is the only strong ref so far.
        assert_eq!(Rc::strong_count(&inner), 1);

        // Mirrors `new_window`: stash an extra ref for the (simulated)
        // `WM_NCCREATE`/`GWLP_USERDATA` handoff.
        let raw = rc_to_pointer(&inner);
        assert_eq!(Rc::strong_count(&inner), 2);

        // Mirrors `wm_nccreate` running successfully and Windows then
        // unwinding via `WM_NCDESTROY`, whose handler (`wm_ncdestroy`)
        // reclaims and drops the extra ref, then sets the flag.
        {
            let reclaimed = take_rc_from_pointer(raw as LPVOID);
            assert_eq!(
                Rc::strong_count(&inner),
                2,
                "take_rc_from_pointer reclaims ownership without adding a ref"
            );
            inner.borrow().extra_ref_reclaimed_by_ncdestroy.set(true);
            drop(reclaimed);
        }
        // The extra ref is gone: back down to just the local `inner` binding.
        assert_eq!(
            Rc::strong_count(&inner),
            1,
            "wm_ncdestroy's reclaim should have dropped the extra ref"
        );

        // Mirrors `new_window`'s `Err(err)` branch consulting the flag.
        let should_drop =
            should_new_window_drop_extra_ref(inner.borrow().extra_ref_reclaimed_by_ncdestroy.get());
        assert!(
            !should_drop,
            "wm_ncdestroy already reclaimed the extra ref; new_window must not drop it again"
        );
        if should_drop {
            // Not reached given the assertion above, but mirrors the real
            // guarded call site exactly, including what a regression would
            // do: drop `raw` again, which -- since it already dropped to 0
            // strong refs above -- would be the double-free this test guards
            // against.
            //
            // SAFETY: this branch is unreachable (see the `assert!` above);
            // mirrored here only so the shape matches the real guarded call
            // site in `new_window`. If it ever did run, `raw` would already
            // have been reclaimed by `take_rc_from_pointer` above, so this
            // would be exactly the double-free/UB this test exists to catch.
            drop(unsafe { Rc::from_raw(raw) });
        }

        // Final invariant: exactly one reclaim happened across the whole
        // sequence, and `inner` is still alive and uncorrupted.
        assert_eq!(Rc::strong_count(&inner), 1);
        assert!(!inner.borrow().renderer_ready);
    }

    /// Order 2 (the always-safe order): `CreateWindowExW` fails *before*
    /// `WM_NCCREATE` ever ran (bad class name/parent handle, etc.), so
    /// `GWLP_USERDATA` was never populated and `wm_ncdestroy` never runs.
    /// `new_window`'s failure path must be the one to reclaim the extra ref
    /// itself here, or it would leak permanently.
    #[test]
    fn create_window_failure_before_nccreate_still_reclaims_extra_ref() {
        let inner = test_inner();
        assert_eq!(Rc::strong_count(&inner), 1);

        let raw = rc_to_pointer(&inner);
        assert_eq!(Rc::strong_count(&inner), 2);

        // `WM_NCCREATE`/`wm_ncdestroy` never ran in this order, so the flag
        // is untouched (still `false`, as `test_inner` initialized it).
        assert!(!inner.borrow().extra_ref_reclaimed_by_ncdestroy.get());

        let should_drop =
            should_new_window_drop_extra_ref(inner.borrow().extra_ref_reclaimed_by_ncdestroy.get());
        assert!(
            should_drop,
            "wm_ncdestroy never ran; new_window must reclaim the extra ref itself or it leaks"
        );
        if should_drop {
            // SAFETY: `raw` was produced by `rc_to_pointer` above and never
            // handed to `take_rc_from_pointer`/`wm_ncdestroy` in this order.
            drop(unsafe { Rc::from_raw(raw) });
        }

        assert_eq!(
            Rc::strong_count(&inner),
            1,
            "the extra ref must be reclaimed exactly once, not leaked"
        );
    }

    /// Direct table test of the decision function in isolation, independent
    /// of any `Rc`/pointer plumbing: the two inputs the real call site can
    /// ever observe must map to opposite decisions, or either a leak or a
    /// double-free is reachable.
    #[test]
    fn should_new_window_drop_extra_ref_table() {
        assert!(
            should_new_window_drop_extra_ref(false),
            "wm_ncdestroy has not run: new_window must reclaim the ref itself"
        );
        assert!(
            !should_new_window_drop_extra_ref(true),
            "wm_ncdestroy already reclaimed the ref: new_window must not double-drop it"
        );
    }
}
