use super::super::placeholder::{
    PlaceholderSpinner, PLACEHOLDER_FADE_TIMER_ID, PLACEHOLDER_SPINNER_TIMER_ID,
};
use super::super::*;
use crate::WindowEvent;
use std::ptr::{null, null_mut};
use winapi::um::wingdi::{SelectObject, SetBkMode, SetTextColor, TRANSPARENT};

/// Max number of trailing dots after "Loading" (0..=3, cycling).
const PLACEHOLDER_LOADER_MAX_DOTS: u128 = 3;
/// One full "Loading" -> "Loading..." -> "Loading" cycle every 1.4s -- fast
/// enough to read as "alive" within the first paint or two, slow enough not
/// to be distracting for however many seconds WebGpu init takes.
const PLACEHOLDER_SPINNER_PERIOD_MS: u128 = 1400;

/// Paint the animated "Loading..." placeholder label into `hdc`, covering
/// `rect` (the window's client area) with the background color first. Task
/// #384's original design (a ring of orbiting dots) read as an illegible
/// speck once made large enough to actually be visible on a big/high-DPI
/// window (task #406) -- a plain text label sidesteps that entirely, since
/// its size already tracks the font instead of a hand-picked pixel radius.
/// Same cheap GDI path as before (no WebGpu/render-thread involvement,
/// since that pipeline is exactly what is not ready yet).
///
/// # Safety
/// `hdc` must be a valid, live device context for a window whose client
/// area is `rect`.
pub(in crate::os::windows::window) unsafe fn draw_placeholder_spinner(
    hdc: HDC,
    rect: &RECT,
    spinner: &PlaceholderSpinner,
) {
    FillRect(hdc, rect, spinner.bg_brush);

    let phase = spinner.started.elapsed().as_millis() % PLACEHOLDER_SPINNER_PERIOD_MS;
    let num_dots = (phase * (PLACEHOLDER_LOADER_MAX_DOTS + 1) / PLACEHOLDER_SPINNER_PERIOD_MS)
        .min(PLACEHOLDER_LOADER_MAX_DOTS);
    let mut text = String::from("Loading");
    for _ in 0..num_dots {
        text.push('.');
    }
    let mut wide_text = wide_string(&text);
    // `DrawTextW` wants the length excluding any trailing NUL when given an
    // explicit count; `wide_string` null-terminates, so trim that back off
    // rather than passing -1 (which would also work, but this avoids
    // relying on `wide_text` having no embedded NULs of its own).
    let text_len = (wide_text.len() as i32 - 1).max(0);

    // SAFETY: `hdc` is the live device context passed in by the caller;
    // `spinner.font` is a live GDI object owned by `spinner` for the
    // duration of this call (or null, if `CreateFontW` failed at
    // `PlaceholderSpinner::new` time, which `SelectObject` tolerates as a
    // no-op-ish "keep current font" call). `SelectObject`/`SetTextColor`/
    // `SetBkMode` return the previous values, which are restored before
    // returning so `hdc` (owned by the window, via `CS_OWNDC`) is left
    // exactly as found. `wide_text` is a live UTF-16 buffer for the
    // duration of the `DrawTextW` call; `rect` is caller-provided and only
    // read (`DrawTextW`'s `DT_CALCRECT` is not set, so it isn't mutated).
    let old_font = SelectObject(hdc, spinner.font as _);
    let old_color = SetTextColor(hdc, spinner.text_color);
    let old_bk_mode = SetBkMode(hdc, TRANSPARENT as i32);

    let mut text_rect = *rect;
    DrawTextW(
        hdc,
        wide_text.as_mut_ptr(),
        text_len,
        &mut text_rect,
        DT_CENTER | DT_VCENTER | DT_SINGLELINE,
    );

    SelectObject(hdc, old_font);
    SetTextColor(hdc, old_color);
    SetBkMode(hdc, old_bk_mode);
}

/// # Safety
/// `hwnd` must be a valid window handle; the `PAINTSTRUCT` is fully
/// initialized before being passed to `BeginPaint`/`EndPaint`.
pub(in crate::os::windows::window) unsafe fn wm_paint(
    hwnd: HWND,
    _msg: UINT,
    _wparam: WPARAM,
    _lparam: LPARAM,
) -> Option<LRESULT> {
    let inner = rc_from_hwnd(hwnd)?;
    let mut inner = inner.borrow_mut();

    if inner.paint_throttled {
        inner.invalidated = true;
        // Mark the update region valid so Windows doesn't immediately
        // re-synthesize WM_PAINT: leaving the region dirty makes the
        // message loop spin without ever blocking until the throttle
        // timer below fires (up to 1000/max_fps later). The timer's own
        // InvalidateRect, or any invalidation arriving in the meantime,
        // is what brings the next WM_PAINT.
        // SAFETY: `hwnd` is the valid window handle this message was
        // dispatched for; a null rect validates the whole client area.
        unsafe { ValidateRect(hwnd, null()) };
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
    // SAFETY: paint DC uses parent-client coordinates; keep DWM's caption-button layer visible.
    caption::clear_frame_background(hwnd, hdc);
    // Paint the placeholder spinner here rather than leaving it to
    // `wm_erasebkgnd`. That handler can never do it during our own paint
    // cycle: `BeginPaint` sends `WM_ERASEBKGND` *synchronously*, from
    // inside this function, while we are still holding `borrow_mut()` on
    // the same `RefCell` -- so its `try_borrow` always fails and it skips
    // the paint. (And the repaint we schedule below uses
    // `InvalidateRect(.., bErase = 0)`, which doesn't request an erase in
    // the first place.) Doing it here, where `inner` is already borrowed,
    // is what actually makes a shown-but-not-yet-rendered window come up
    // showing the spinner instead of unpainted white.
    if let Some(spinner) = inner.placeholder_spinner.as_ref() {
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
            // SAFETY: placeholder must cover terminal content, not the caption band.
            caption::content_rect(hwnd, &mut rect);
            // SAFETY: `hdc` is the non-null device context just returned by
            // `BeginPaint` and still live until `EndPaint`; `spinner`'s GDI
            // objects are owned by `inner` (created in
            // `PlaceholderSpinner::new`, deleted only in
            // `clear_placeholder_background`/`wm_ncdestroy`).
            draw_placeholder_spinner(hdc, &rect, spinner);
        }
    }
    EndPaint(hwnd, &ps);

    inner.invalidated = false;
    // Ask the app to repaint in a bit
    inner.events.dispatch(WindowEvent::NeedRepaint);

    inner.paint_throttled = true;
    let window_id = inner.hwnd;
    let max_fps = inner.config.max_fps;
    promise::spawn::spawn(async move {
        async_io::Timer::after(std::time::Duration::from_millis(1000 / max_fps)).await;
        Connection::with_window_inner(window_id, move |inner| {
            inner.paint_throttled = false;
            if inner.invalidated {
                caption::invalidate_content(inner.hwnd.0);
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
/// While `placeholder_spinner` is set, paint the spinner into `rcPaint` and
/// report the background as erased (return 1). Once
/// `clear_placeholder_background` has dropped it (called once the first
/// real frame has actually been *presented* -- task #425, hardened by task
/// #407; see `WindowOps::clear_placeholder_background`'s doc comment for
/// which call site does this on which path), fall straight back to
/// returning 1 without painting -- identical to today's null-brush behavior.
///
/// # Safety
/// `hwnd` must be a valid window handle and `wparam` the `HDC` passed by
/// the real `WM_ERASEBKGND` message.
pub(super) unsafe fn wm_erasebkgnd(
    hwnd: HWND,
    _msg: UINT,
    wparam: WPARAM,
    _lparam: LPARAM,
) -> Option<LRESULT> {
    // SAFETY: WM_ERASEBKGND supplies a live parent-client paint DC.
    caption::clear_frame_background(hwnd, wparam as HDC);
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

    if let Some(spinner) = inner.placeholder_spinner.as_ref() {
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
        // SAFETY: initialized client rectangle for this live parent window.
        caption::content_rect(hwnd, &mut rect);
        // SAFETY: `hdc` comes from `wparam` of a real `WM_ERASEBKGND`
        // message and is therefore a valid device context for this window;
        // `spinner`'s GDI objects are owned by `inner` (created in
        // `PlaceholderSpinner::new`, deleted only in
        // `clear_placeholder_background`/`wm_ncdestroy`) and `rect` is the
        // just-populated client rect.
        draw_placeholder_spinner(hdc, &rect, spinner);
    }

    Some(1)
}

/// Handles `WM_TIMER`. Two timer ids are used: `PLACEHOLDER_SPINNER_TIMER_ID`
/// (armed in `wm_nccreate`), which invalidates the client area to advance the
/// spinner animation; and `PLACEHOLDER_FADE_TIMER_ID` (armed in
/// `start_placeholder_fade`), which steps the overlay's alpha down via
/// `tick_placeholder_fade`. Once the spinner timer is killed by
/// `clear_placeholder_background` and the fade completes (or
/// `finish_placeholder_fade` kills it early), no more of either arrive.
///
/// # Safety
/// `hwnd` must be a valid window handle.
pub(super) unsafe fn wm_timer(
    hwnd: HWND,
    _msg: UINT,
    wparam: WPARAM,
    _lparam: LPARAM,
) -> Option<LRESULT> {
    if wparam == PLACEHOLDER_SPINNER_TIMER_ID {
        // SAFETY: `hwnd` is the valid window handle passed in. `RDW_ERASE`
        // so `WM_ERASEBKGND` fires again too (needed since the spinner also
        // paints there for the `BeginPaint`-synchronous-erase case, see
        // `wm_erasebkgnd`'s doc comment). `RDW_ALLCHILDREN` matters as much
        // as the invalidate itself here, for the same reason it does in
        // `schedule_show_window`: the WebGpu child window covers the entire
        // client area and is what the user actually sees, and a plain
        // `InvalidateRect` on the parent does not reach it, so the spinner
        // would never animate.
        RedrawWindow(
            hwnd,
            null(),
            null_mut(),
            RDW_INVALIDATE | RDW_ERASE | RDW_ALLCHILDREN,
        );
        return Some(0);
    }
    if wparam == PLACEHOLDER_FADE_TIMER_ID {
        // Unlike the spinner timer above, this does not repaint anything --
        // `tick_placeholder_fade` only ever changes the overlay's whole-
        // window alpha via `SetLayeredWindowAttributes`, which DWM composites
        // without the overlay (or anything beneath it) needing to repaint at
        // all.
        if let Some(inner) = rc_from_hwnd(hwnd) {
            // `try_borrow_mut` (not `borrow_mut`): a re-entrant `WM_TIMER`
            // arriving through a nested message pump (move/size loop, system
            // menu) while `WindowInner` is already borrowed would panic
            // here, and `catch_unwind` in `wnd_proc` turns that into
            // `std::process::exit(1)`. A skipped tick is harmless -- alpha
            // is derived from `fade.started.elapsed()`, not accumulated
            // incrementally.
            if let Ok(mut inner) = inner.try_borrow_mut() {
                inner.tick_placeholder_fade();
            }
        }
        return Some(0);
    }
    None
}

/// Paint the placeholder spinner belonging to `child`'s parent top-level
/// window into `hdc`/`rect`, if the parent still has one (i.e. the renderer
/// isn't up yet). Returns whether anything was painted. Used by
/// `child_wnd_proc` -- the WebGpu child window has no `WindowInner` of its
/// own, so it borrows its parent's. Unlike the old single-`HBRUSH` version,
/// this can't just hand back a `Copy` handle and let the caller draw with
/// it afterwards: `PlaceholderSpinner` isn't `Copy` (it owns a `started:
/// Instant` and several GDI handles), so the actual `draw_placeholder_
/// spinner` call has to happen here, inside the parent's borrow.
///
/// # Safety
/// `child` must be a valid window handle; `hdc` must be a valid, live
/// device context for `child`'s client area, which must equal `rect`.
pub(in crate::os::windows::window) unsafe fn paint_parent_placeholder_spinner(
    child: HWND,
    hdc: HDC,
    rect: &RECT,
) -> bool {
    let parent = GetParent(child);
    if parent.is_null() {
        return false;
    }
    let inner = match rc_from_hwnd(parent) {
        Some(inner) => inner,
        None => return false,
    };
    // `try_borrow` for the same reason the top-level `WM_ERASEBKGND`
    // handler uses it: this can be reached synchronously from a
    // `BeginPaint` on a stack frame that already holds the borrow.
    let inner = match inner.try_borrow() {
        Ok(inner) => inner,
        Err(_) => return false,
    };
    match inner.placeholder_spinner.as_ref() {
        Some(spinner) => {
            draw_placeholder_spinner(hdc, rect, spinner);
            true
        }
        None => false,
    }
}
