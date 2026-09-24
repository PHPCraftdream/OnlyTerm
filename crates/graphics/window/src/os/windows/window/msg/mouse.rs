use super::super::*;
use super::frame::no_native_title_bar;
use crate::{
    Modifiers, MouseButtons, MouseCursor, MouseEvent, MouseEventKind, MousePress, Point,
    ScreenPoint, WindowEvent,
};
use lazy_static::lazy_static;
use std::convert::TryInto;
use std::io;
use std::ptr::null_mut;
use winapi::um::wingdi::MAKEPOINTS;
use winreg::enums::HKEY_CURRENT_USER;
use winreg::RegKey;

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

pub(in crate::os::windows::window) fn mouse_coords(lparam: LPARAM) -> Point {
    let point = MAKEPOINTS(lparam as _);
    Point::new(point.x as _, point.y as _)
}

fn nc_mouse_coords(hwnd: HWND, lparam: LPARAM) -> Point {
    let point = MAKEPOINTS(lparam as _);
    let point = ScreenPoint::new(point.x as _, point.y as _);
    screen_to_client(hwnd, point)
}

pub(in crate::os::windows::window) fn screen_to_client(hwnd: HWND, point: ScreenPoint) -> Point {
    let mut point = POINT {
        x: point.x.try_into().unwrap(),
        y: point.y.try_into().unwrap(),
    };
    // SAFETY: `hwnd` is a valid window handle and `point` is a live `POINT`.
    unsafe {
        ScreenToClient(hwnd, &mut point as *mut _);
        point.y -= caption::height(hwnd);
    }
    Point::new(point.x.try_into().unwrap(), point.y.try_into().unwrap())
}

pub(in crate::os::windows::window) fn client_to_screen(hwnd: HWND, point: Point) -> ScreenPoint {
    let mut point = POINT {
        x: point.x.try_into().unwrap(),
        y: point.y.try_into().unwrap(),
    };
    // SAFETY: `hwnd` is a valid window handle and `point` is a live `POINT`.
    unsafe {
        point.y += caption::height(hwnd);
        ClientToScreen(hwnd, &mut point as *mut _);
    }
    ScreenPoint::new(point.x.try_into().unwrap(), point.y.try_into().unwrap())
}

pub(in crate::os::windows::window) fn apply_mouse_cursor(cursor: Option<MouseCursor>) {
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
pub(super) unsafe fn mouse_button(
    hwnd: HWND,
    msg: UINT,
    wparam: WPARAM,
    lparam: LPARAM,
) -> Option<LRESULT> {
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
    let mut coords = mouse_coords(lparam);
    // SAFETY: client messages use the parent's raw origin, not the terminal origin.
    coords.y -= unsafe { caption::height(hwnd) } as isize;
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
pub(super) unsafe fn nc_mouse_button(
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
pub(super) unsafe fn mouse_move(
    hwnd: HWND,
    _msg: UINT,
    wparam: WPARAM,
    lparam: LPARAM,
) -> Option<LRESULT> {
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
    let mut coords = mouse_coords(lparam);
    // SAFETY: client messages use the parent's raw origin, not the terminal origin.
    coords.y -= unsafe { caption::height(hwnd) } as isize;
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
pub(super) unsafe fn nc_mouse_move(
    hwnd: HWND,
    _msg: UINT,
    wparam: WPARAM,
    lparam: LPARAM,
) -> Option<LRESULT> {
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
pub(super) unsafe fn mouse_leave(
    hwnd: HWND,
    _msg: UINT,
    _wparam: WPARAM,
    _lparam: LPARAM,
) -> Option<LRESULT> {
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
pub(super) unsafe fn mouse_wheel(
    hwnd: HWND,
    msg: UINT,
    wparam: WPARAM,
    lparam: LPARAM,
) -> Option<LRESULT> {
    let inner = rc_from_hwnd(hwnd)?;
    let (modifiers, mouse_buttons) = mods_and_buttons(wparam);
    // Wheel events return screen coordinates!
    let coords = mouse_coords(lparam);
    let screen_coords = ScreenPoint::new(coords.x, coords.y);
    let coords = screen_to_client(hwnd, screen_coords);
    let delta = GET_WHEEL_DELTA_WPARAM(wparam);
    let event = MouseEvent {
        kind: if msg == WM_MOUSEHWHEEL {
            let mut inner = inner.borrow_mut();
            let position = super::super::input::wheel::lines(
                delta,
                *WHEEL_SCROLL_CHARS,
                &mut inner.hscroll_remainder,
            );
            log::trace!(
                "mouse_hwheel delta={} remainder={} pos={}",
                delta,
                inner.hscroll_remainder,
                position
            );
            if position == 0 {
                return Some(0);
            }
            MouseEventKind::HorzWheel(position)
        } else {
            let mut inner = inner.borrow_mut();
            let position = super::super::input::wheel::lines(
                delta,
                *WHEEL_SCROLL_LINES,
                &mut inner.vscroll_remainder,
            );
            log::trace!(
                "mouse_wheel delta={} remainder={} pos={}",
                delta,
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
