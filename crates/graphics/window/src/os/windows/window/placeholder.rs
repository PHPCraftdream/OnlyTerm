use super::msg::frame::enable_blur_behind;
use super::msg::paint::{draw_placeholder_spinner, paint_parent_placeholder_spinner};
use super::*;
use anyhow::bail;
use onlyterm_config::ConfigHandle;
use std::io::Error as IoError;
use std::ptr::{null, null_mut};
use std::time::Instant;
use winapi::um::libloaderapi::GetModuleHandleW;
use winapi::um::wingdi::{
    CreateFontW, CreateSolidBrush, DeleteObject, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET,
    DEFAULT_PITCH, DEFAULT_QUALITY, FF_DONTCARE, FW_NORMAL, OUT_DEFAULT_PRECIS, RGB,
};

/// State for the animated "Loading..." startup placeholder painted in place
/// of the old flat placeholder fill (task #384; text style per task #405/
/// #406 follow-up -- the original 8-dot ring read as an illegible speck on
/// large/high-DPI windows and wasn't worth trying to salvage), covering the
/// gap between `ShowWindow` and the renderer's first real frame -- see
/// `WindowInner::placeholder_spinner` and `draw_placeholder_spinner` for the
/// rest of the story. The struct/function names predate this text-based
/// redesign and are kept as-is to avoid an unrelated rename churning every
/// call site; only what gets painted changed.
pub(super) struct PlaceholderSpinner {
    /// Solid brush for the client-area background, same color the old flat
    /// fill used (`config.resolved_palette.background`).
    pub(super) bg_brush: HBRUSH,
    /// Text color for the "Loading..." label, from `config.resolved_palette.
    /// foreground` (falling back like `bg_brush` does for `background`).
    pub(super) text_color: COLORREF,
    /// Font used to draw the label. Owned here (not a stock object) so its
    /// point size can be derived from the window instead of a fixed size
    /// that would look wrong at very different DPIs/window sizes.
    pub(super) font: HFONT,
    /// When the placeholder started, used only to compute an ever-increasing
    /// animation phase (`elapsed % period`) for the trailing-dots count;
    /// never reset, so the animation keeps a steady rate across however
    /// many paints it takes.
    pub(super) started: Instant,
    /// Whether `SetTimer` has been called for this window (idempotent
    /// guard: `WM_NCCREATE` may in principle run more than once per
    /// `WindowInner` in edge cases, and `SetTimer` with the same id is
    /// itself idempotent, but tracking this avoids relying on that).
    pub(super) timer_running: bool,
}

impl PlaceholderSpinner {
    /// # Safety
    /// Always safe to call: only reads plain config values and calls
    /// `CreateSolidBrush`/`CreateFontW`, neither of which can fail in a way
    /// that produces an invalid non-null handle.
    pub(super) unsafe fn new(config: &ConfigHandle) -> Self {
        let (bg_r, bg_g, bg_b, _a) = config
            .resolved_palette
            .background
            .map(|c| c.as_rgba_u8())
            .unwrap_or((0, 0, 0, 0xff));
        // Same fallback rationale as the background: the terminal's own
        // default foreground (light gray) when the palette doesn't specify
        // one, so the label stays visible against the black default
        // background rather than defaulting to another black.
        let (fg_r, fg_g, fg_b, _a) = config
            .resolved_palette
            .foreground
            .map(|c| c.as_rgba_u8())
            .unwrap_or((0xb0, 0xb0, 0xb0, 0xff));
        let face = wide_string("Segoe UI");
        // SAFETY: `face` is a live, null-terminated UTF-16 buffer for the
        // duration of this call; -24 is a plain negative character height
        // (device units, not points) that `CreateFontW` accepts to mean
        // "match this cell height", same convention GDI text APIs use
        // throughout this file. A null result (font unavailable) falls back
        // to the stock system font at paint time via `GetStockObject`, see
        // `draw_placeholder_spinner`.
        let font = CreateFontW(
            -24,
            0,
            0,
            0,
            FW_NORMAL,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            DEFAULT_QUALITY,
            DEFAULT_PITCH | FF_DONTCARE,
            face.as_ptr(),
        );
        PlaceholderSpinner {
            bg_brush: CreateSolidBrush(RGB(bg_r, bg_g, bg_b)),
            text_color: RGB(fg_r, fg_g, fg_b),
            font,
            started: Instant::now(),
            timer_running: false,
        }
    }

    /// # Safety
    /// Always safe to call: `bg_brush`/`font` are live GDI handles for the
    /// whole lifetime of `self` (nothing else ever deletes them), so this
    /// `DeleteObject` pair runs against still-valid handles. This must not
    /// be called more than once per value: after the first `DeleteObject`,
    /// Windows is free to recycle that same handle value for an unrelated
    /// GDI object created elsewhere in the process, and a second
    /// `DeleteObject` on it would then delete that *other* live object
    /// instead of being a harmless no-op. `destroy` has exactly one caller
    /// (`Drop::drop`, below), which Rust guarantees runs at most once per
    /// value, so that double-delete can't happen here.
    unsafe fn destroy(&self) {
        DeleteObject(self.bg_brush as _);
        DeleteObject(self.font as _);
    }
}

impl Drop for PlaceholderSpinner {
    fn drop(&mut self) {
        // SAFETY: see `destroy`'s doc comment for why a double `DeleteObject`
        // would be unsound in general and why it can't happen here: `drop`
        // is the only caller of `destroy`, and Rust guarantees `Drop::drop`
        // runs at most once per value, so this call frees the handles from
        // `PlaceholderSpinner::new` exactly once, on every code path,
        // including the early-return from `Window::new_window` when
        // `create_window` fails after the spinner was already constructed.
        unsafe {
            self.destroy();
        }
    }
}

/// Class name for the `WS_EX_LAYERED` overlay window used to cross-fade the
/// placeholder spinner into the real terminal content (task #385; see
/// `WindowInner::placeholder_fade` and `start_placeholder_fade` for the
/// full story).
const PLACEHOLDER_OVERLAY_CLASS_NAME: &str = "OnlyTermPlaceholderOverlay";

/// State for the placeholder-overlay fade-out (task #385). Unlike
/// `PlaceholderSpinner` (which owns GDI brushes/pens for as long as no
/// renderer is up), this only exists for the brief final transition: it is
/// created once both gating conditions in `start_placeholder_fade` are
/// satisfied, and torn down as soon as alpha reaches zero.
///
/// The overlay is a separate `WS_CHILD | WS_EX_LAYERED` sibling of
/// `webgpu_child_hwnd`, stacked above it in z-order. It exists because the
/// WebGpu child window is a live DXGI swapchain target: once its surface
/// presents a frame, that frame owns the window's pixels outright (flip-model
/// presentation bypasses GDI/DWM composition for that HWND), so there is no
/// way to alpha-blend GDI-painted spinner pixels against WebGpu-presented
/// pixels *within* that same HWND. A `WS_EX_LAYERED` window is DWM's
/// mechanism for exactly this: composite an independent, alpha-controlled
/// surface on top of whatever is beneath it, which is otherwise-unmodified
/// live WebGpu content in this case. The overlay paints one final still frame
/// of the spinner into itself (via the same `draw_placeholder_spinner` used
/// throughout the opaque phase, so there is no visible seam at the moment it
/// appears) and then only its whole-window alpha changes from then on --
/// `SetLayeredWindowAttributes`, stepped by `PLACEHOLDER_FADE_TIMER_ID`.
pub(super) struct PlaceholderFade {
    /// The layered overlay's own child HWND.
    hwnd: HWindow,
    /// When the fade-out began, used to derive the current alpha the same
    /// way `PlaceholderSpinner::started` derives animation phase.
    started: Instant,
}

/// How often the spinner timer fires and, in turn, how often the placeholder
/// window is invalidated to advance the animation. 30fps: smooth enough for
/// a handful of slowly-orbiting dots, cheap enough to be a non-issue on the
/// GUI thread for the few seconds this ever runs (no GPU involvement at
/// all -- see the module-level rationale for why this must stay a plain GDI
/// path). `USER_TIMER_MINIMUM` is 10ms/100fps, so this is comfortably above
/// the OS floor.
pub(super) const PLACEHOLDER_SPINNER_INTERVAL_MS: u32 = 1000 / 30;

/// `SetTimer`/`KillTimer`/`WM_TIMER` id for the placeholder spinner's redraw
/// tick. Scoped to a single constant since each top-level window only ever
/// runs one of these at a time (ids are per-HWND, not global).
pub(super) const PLACEHOLDER_SPINNER_TIMER_ID: usize = 1;

/// `SetTimer`/`KillTimer`/`WM_TIMER` id for the placeholder-overlay fade
/// tick (task #385). Deliberately distinct from
/// `PLACEHOLDER_SPINNER_TIMER_ID`: although in practice the spinner timer
/// is killed by `clear_placeholder_background` before the fade timer is
/// ever armed (there is no message-dispatch point between the
/// `KillTimer(id=1)` and the `SetTimer(id=2)` in that function), using a
/// separate id means the two never collide even if that ordering changes --
/// timer ids are per-HWND, so a shared id would make the second `SetTimer`
/// silently replace the first.
pub(super) const PLACEHOLDER_FADE_TIMER_ID: usize = 2;

/// Total duration of the placeholder-overlay fade-out, once triggered.
/// Long enough to read as a deliberate cross-fade rather than a flicker,
/// short enough not to noticeably delay the terminal becoming fully opaque
/// after it's already interactive.
const PLACEHOLDER_FADE_DURATION_MS: u32 = 320;

/// How often the fade timer ticks to step alpha down. 30fps, matching the
/// spinner's own animation rate -- no reason for the fade to be smoother
/// than the animation it's fading out.
const PLACEHOLDER_FADE_INTERVAL_MS: u32 = 1000 / 30;

impl Window {
    /// Create the `WS_EX_LAYERED` overlay child window used to cross-fade
    /// the placeholder spinner out (task #385; see `PlaceholderFade`'s doc
    /// comment for why this needs to be a separate window from
    /// `webgpu_child_hwnd` at all). Parented to `parent` and sized to
    /// exactly cover `sibling`'s current bounds (i.e. the WebGpu child's,
    /// which in turn covers the parent's full client area), then placed
    /// directly above `sibling` in z-order so it visually covers the
    /// WebGpu content while opaque and lets it show through as alpha drops.
    ///
    /// `WS_EX_TRANSPARENT` makes this input-transparent exactly like the
    /// WebGpu child (`child_wnd_proc`'s `WM_NCHITTEST` handling) -- it only
    /// exists for a fraction of a second right as the terminal is becoming
    /// interactive, so the user's clicks/keys must not be able to land on it
    /// even momentarily.
    fn create_placeholder_overlay_window(parent: HWND, sibling: HWND) -> anyhow::Result<HWND> {
        let class_name = wide_string(PLACEHOLDER_OVERLAY_CLASS_NAME);
        // SAFETY: null module name returns the current process's exe handle,
        // which is always valid and non-null on Windows.
        let h_inst = unsafe { GetModuleHandleW(null()) };
        let class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(overlay_wnd_proc),
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
        // pointers and a registered `overlay_wnd_proc`; the failure case
        // (return 0) is handled below, including the benign
        // CLASS_ALREADY_EXISTS case (multiple top-level windows in the same
        // process share this class).
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
        // SAFETY: `rect` is a live stack `RECT` and `sibling` is a valid
        // window handle; `GetWindowRect` only writes into `rect`. We use
        // `sibling`'s (the WebGpu child's) screen rect rather than
        // `parent`'s client rect so the overlay matches it exactly even if
        // the two have drifted apart by a pending resize.
        unsafe {
            GetWindowRect(sibling, &mut rect);
        }
        let mut top_left = POINT {
            x: rect.left,
            y: rect.top,
        };
        // SAFETY: `parent` is a valid window handle and `top_left` is a live
        // stack `POINT`; `ScreenToClient` only writes into it.
        unsafe {
            ScreenToClient(parent, &mut top_left);
        }

        let name = wide_string(PLACEHOLDER_OVERLAY_CLASS_NAME);
        // SAFETY: `class_name`/`name` are live null-terminated UTF-16
        // buffers. `parent` is a valid top-level HWND, so this becomes a
        // `WS_CHILD` window owned by it (destroyed automatically when
        // `parent` is destroyed, and also explicitly torn down by
        // `finish_placeholder_fade`). `WS_EX_LAYERED` is what allows
        // `SetLayeredWindowAttributes` below; the window starts fully
        // opaque (alpha 255) so its first paint is indistinguishable from
        // the spinner it replaces. A null result is reported as an error.
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TRANSPARENT,
                class_name.as_ptr(),
                name.as_ptr(),
                WS_CHILD | WS_VISIBLE,
                top_left.x,
                top_left.y,
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
            bail!("CreateWindowExW (placeholder overlay): {}", err);
        }

        // SAFETY: `hwnd` is the just-created, valid window handle; 255 is a
        // valid alpha value and `LWA_ALPHA` is the flag that makes
        // `SetLayeredWindowAttributes` honor it (as opposed to a color-key).
        unsafe {
            SetLayeredWindowAttributes(hwnd, 0, 255, LWA_ALPHA);
        }

        // A freshly created `WS_CHILD` window is normally already placed at
        // the top of its parent's child z-order by `CreateWindowExW`, which
        // would put it above `sibling` (the WebGpu child, created earlier in
        // the same `new_window` call) without further action. Explicitly
        // reassert `HWND_TOP` anyway rather than relying on that default:
        // being visually above the WebGpu content is not just cosmetic here
        // (it's the entire mechanism the cross-fade depends on), so it's
        // worth the one extra, cheap `SetWindowPos` call to make it
        // unconditionally true instead of implicitly true.
        // SAFETY: `hwnd` is the just-created, valid child window handle;
        // `HWND_TOP` is a reserved constant Win32 accepts in place of a
        // window handle here. `SWP_NOMOVE|SWP_NOSIZE` leave the position/size
        // just set by `CreateWindowExW` untouched; `SWP_NOACTIVATE` avoids
        // stealing focus (this window is never meant to be focusable at
        // all -- it's `WS_EX_TRANSPARENT`).
        unsafe {
            SetWindowPos(
                hwnd,
                HWND_TOP,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }

        Ok(hwnd)
    }
}

impl WindowInner {
    /// Mark the renderer as ready (task #385's first gating condition) and,
    /// if the shell is also already known to be ready, hand off from the
    /// opaque spinner to the fade overlay; then drop the placeholder
    /// spinner's own timer/GDI resources (see `placeholder_spinner`'s doc
    /// comment) now that a working renderer is in place and responsible for
    /// painting every subsequent frame. Idempotent: safe to call more than
    /// once -- on the happy path, once after the first real frame has
    /// actually been *presented* (task #425, later corrected by task #407:
    /// from `TermWindow::paint_impl` directly when there is no dedicated
    /// render thread, or from `renderthread.rs`'s `submit_one_frame` after
    /// its first successful `submit_frame`/`present()` when
    /// `webgpu_render_thread` is active -- the Windows default -- since
    /// `paint_impl` returning `Ok` only means the frame was *enqueued* to
    /// that thread on that path, not yet presented; see either call site's
    /// own comment for the full reasoning) -- rather than as soon as
    /// `TermWindow::created` installs a `RenderState`, and once more as a
    /// backstop from `wm_ncdestroy` if the window is closed before a
    /// renderer ever came up -- `Option::take` makes the second call's
    /// spinner teardown a no-op, and `start_placeholder_fade` is separately
    /// guarded by `placeholder_fade.is_some()`.
    ///
    /// Order matters here: `start_placeholder_fade` must run (and, if
    /// it decides to start the fade, paint the overlay's one-shot frame)
    /// *before* the spinner's GDI objects are destroyed below, since that
    /// paint reads them via `draw_placeholder_spinner`.
    pub(super) fn clear_placeholder_background(&mut self) {
        self.renderer_ready = true;
        // The WebGpu child is created hidden (see
        // `create_webgpu_child_window`) precisely so it never composites
        // undefined pixels over the placeholder. This is the moment that
        // stops being a risk: a frame has actually been presented to its
        // swapchain, so it now has something to show.
        //
        // Must happen before `start_placeholder_fade` below: the fade
        // overlay is created directly above this child and cross-fades into
        // whatever it covers, so the child has to be visible underneath it
        // for the fade to reveal the real terminal content rather than an
        // empty client area.
        if !self.webgpu_child_hwnd.0.is_null() {
            // SAFETY: `webgpu_child_hwnd.0` is a valid `WS_CHILD` handle
            // owned by `self.hwnd.0`. `SW_SHOWNA` shows it without
            // activating, leaving focus (which lives on the parent -- this
            // child is never focused) undisturbed.
            unsafe {
                ShowWindow(self.webgpu_child_hwnd.0, SW_SHOWNA);
            }
        }

        // `enable_blur_behind` used to run at window-creation time, well
        // before any of this. On hybrid-graphics laptops, GPU init (DXGI
        // adapter enumeration + shader compile) can take several seconds,
        // during which the window already had a DWM blur-behind region
        // covering its entire client area (`hRgnBlur` in that function is
        // an infinite rect) with nothing real painted into it yet -- DWM
        // composited whatever was behind the window (other windows, video
        // playback) through that region for the whole gap, which is what
        // read as a multi-second startup flicker. Deferring the call to
        // here, once a real frame is actually about to be shown, keeps
        // whatever cosmetic effect it provides without the see-through
        // window during startup.
        //
        // Guarded because the `wm_ncdestroy` backstop call into this method
        // (window closed before a renderer ever came up) has already reset
        // `self.hwnd` to null by the time it gets here.
        if !self.hwnd.0.is_null() {
            enable_blur_behind(self.hwnd.0);
        }

        self.start_placeholder_fade();

        if let Some(spinner) = self.placeholder_spinner.take() {
            // Startup-latency diagnostics: see the "startup:" checkpoints in
            // onlyterm-gui's main.rs. `placeholder_spinner` being `Some` here
            // is this method's own idempotency marker (see its doc comment),
            // so this only fires on the actual first-frame-presented call,
            // not the harmless repeat calls.
            log::info!("startup: first frame presented, placeholder cleared");
            if spinner.timer_running {
                // SAFETY: `self.hwnd.0` is either a valid window handle
                // that `SetTimer` was previously called on with this same
                // id (see `wm_nccreate`), for the happy-path call (from
                // `TermWindow::paint_impl` or `renderthread.rs`, see this
                // method's doc comment); or null, for the `wm_ncdestroy`
                // backstop call, which runs after `wm_ncdestroy` has
                // already reset `hwnd` to null -- `KillTimer(null, ..)`
                // just fails and does nothing, which is fine, since Windows
                // already destroys every timer owned by a window as part of
                // that same `WM_NCDESTROY` teardown.
                unsafe {
                    KillTimer(self.hwnd.0, PLACEHOLDER_SPINNER_TIMER_ID);
                }
            }
        }
    }

    /// Mark the shell as ready (task #385's second gating condition -- see
    /// `WindowOps::notify_shell_ready`) and, if the renderer is also already
    /// ready, start the placeholder fade. Idempotent: `shell_ready` is only
    /// ever set to `true`, and `start_placeholder_fade` is separately
    /// guarded against starting twice.
    pub(super) fn notify_shell_ready(&mut self) {
        self.shell_ready = true;
        self.start_placeholder_fade();
    }

    /// Start the placeholder-overlay fade-out (task #385) if, and only if,
    /// both gating conditions are now satisfied (`renderer_ready` -- a
    /// working `RenderState` exists and is producing frames -- and
    /// `shell_ready` -- the shell has produced its first output) and no
    /// fade has been started for this window yet.
    ///
    /// Why gate on both rather than just the renderer: the renderer being
    /// ready only means WebGpu can now present real frames -- typically
    /// still a blank/default-background terminal for the fraction of a
    /// second until the shell's startup banner/prompt actually lands. That
    /// would still show a "dead" cross-fade into apparently-nothing rather
    /// than into a live shell. Why not gate on just the shell: with a slow
    /// GPU/driver init, the shell could be ready before the renderer has
    /// even installed a `RenderState` to hand off to, in which case there is
    /// nothing yet to fade *into*.
    ///
    /// Creates the overlay window, gives it one paint of the current spinner
    /// frame (so its first visible frame is pixel-identical to what it
    /// replaces -- see `create_placeholder_overlay_window`'s doc comment),
    /// and arms the fade timer. Called from both `clear_placeholder_
    /// background` and `notify_shell_ready`, i.e. from whichever of the two
    /// gating conditions is satisfied *second* -- that's what "start on the
    /// later of the two events" means operationally: both setters call this
    /// unconditionally, but it only actually does anything once both flags
    /// are set.
    fn start_placeholder_fade(&mut self) {
        if !self.renderer_ready || !self.shell_ready || self.placeholder_fade.is_some() {
            return;
        }
        if self.placeholder_spinner.is_none() {
            // `clear_placeholder_background` already destroyed the spinner
            // (this happens whenever it runs while `shell_ready` is still
            // false: it calls `start_placeholder_fade` first, which bails
            // out on the `!self.shell_ready` check above, then tears the
            // spinner down regardless), and `notify_shell_ready` is what's
            // calling this now. There is nothing left to snapshot into the
            // overlay's one-shot frame -- creating it anyway would produce
            // an opaque, never-painted `WS_EX_LAYERED` window sitting over
            // live WebGpu content for the entire fade duration (the exact
            // "abrupt flash" defect this feature exists to prevent, just
            // moved to a different trigger). Same reasoning applies to a
            // renderer rebuild (`finish_renderer_rebuild`) re-running
            // `clear_placeholder_background` long
            // after the original fade already completed: `renderer_ready`/
            // `shell_ready` are latched `true` forever, so without this
            // guard every later rebuild would restart a fade with nothing
            // to fade from. Falling back to an instant, non-animated
            // hand-off here is strictly safer than painting nothing --
            // same trade-off already accepted below for "no WebGpu child
            // to layer over".
            return;
        }
        if self.hwnd.0.is_null() {
            // The top-level window is already gone (e.g. this is being
            // reached via `wm_ncdestroy`'s backstop call to
            // `clear_placeholder_background`, which runs after `hwnd` has
            // already been reset to null there). Nothing to layer a new
            // overlay on top of.
            return;
        }
        if self.webgpu_child_hwnd.0.is_null() {
            // No WebGpu child to layer over (surface creation fell back to
            // the top-level window itself, or never happened) -- there is
            // nothing for a layered sibling to sit above, so there's no safe
            // place to put the overlay. The spinner's own teardown in
            // `clear_placeholder_background` still runs as normal; this
            // just means that edge case gets the old instant cut instead of
            // a cross-fade, same as before task #385.
            return;
        }
        let overlay_hwnd = match Window::create_placeholder_overlay_window(
            self.hwnd.0,
            self.webgpu_child_hwnd.0,
        ) {
            Ok(hwnd) => hwnd,
            Err(err) => {
                log::warn!(
                    "Failed to create placeholder fade overlay ({:#}); \
                     falling back to an instant cut instead of a cross-fade",
                    err
                );
                return;
            }
        };

        // Paint the overlay's one and only content frame now, while
        // `self.placeholder_spinner` is still `Some` (the caller,
        // `clear_placeholder_background`, destroys it right after this
        // returns). This must NOT go through `UpdateWindow`/`WM_PAINT`: both
        // `start_placeholder_fade` callers (`clear_placeholder_background`,
        // `notify_shell_ready`) only ever run inside `Connection::
        // with_window_inner`'s `handle.borrow_mut()`, so a synchronous
        // `WM_PAINT`/`WM_ERASEBKGND` dispatched to `overlay_wnd_proc` here
        // would call `paint_parent_placeholder_spinner`, which does
        // `GetParent(overlay_hwnd)` -> this SAME top-level `self.hwnd.0` ->
        // `rc_from_hwnd` -> `try_borrow()` on the very `RefCell` already
        // mutably borrowed one frame up the stack. That `try_borrow()` would
        // always fail (silently, by design -- see `paint_parent_placeholder_
        // spinner`'s `Err(_) => return false` arm), leaving the overlay's
        // first-ever frame unpainted instead of "pixel-identical to what it
        // replaces" as intended (the exact reentrancy hazard `wm_paint`'s own
        // doc comment describes for the original spinner, reintroduced here
        // by `start_placeholder_fade` running inside an existing borrow).
        // Paint directly with a plain `GetDC`/`ReleaseDC` instead, using
        // `self.placeholder_spinner` we already have `&mut self` access to,
        // bypassing the window-procedure/borrow round-trip entirely.
        if let Some(spinner) = self.placeholder_spinner.as_ref() {
            // SAFETY: `overlay_hwnd` is the just-created, valid window
            // handle; `spinner` is `self.placeholder_spinner`, still alive
            // (the caller only drops it after this method returns);
            // `GetDC`/`ReleaseDC` are the standard paired calls for painting
            // outside a `WM_PAINT` cycle.
            unsafe {
                let mut rect = RECT {
                    left: 0,
                    top: 0,
                    right: 0,
                    bottom: 0,
                };
                GetClientRect(overlay_hwnd, &mut rect);
                let hdc = GetDC(overlay_hwnd);
                if !hdc.is_null() {
                    draw_placeholder_spinner(hdc, &rect, spinner);
                    ReleaseDC(overlay_hwnd, hdc);
                }
            }
        }

        // SAFETY: `self.hwnd.0` is a valid window handle (this method is
        // only ever reached while it is); `PLACEHOLDER_FADE_TIMER_ID` is a
        // plain nonzero id distinct from `PLACEHOLDER_SPINNER_TIMER_ID`.
        unsafe {
            SetTimer(
                self.hwnd.0,
                PLACEHOLDER_FADE_TIMER_ID,
                PLACEHOLDER_FADE_INTERVAL_MS,
                None,
            );
        }

        self.placeholder_fade = Some(PlaceholderFade {
            hwnd: HWindow(overlay_hwnd),
            started: Instant::now(),
        });
    }

    /// Advance the placeholder-overlay fade by one timer tick: compute the
    /// current alpha from elapsed time and either apply it via
    /// `SetLayeredWindowAttributes`, or, once the fade duration has elapsed,
    /// tear the overlay down for good (kill the timer, hide+destroy the
    /// overlay window). Called from `WM_TIMER` for `PLACEHOLDER_FADE_TIMER_
    /// ID`; a no-op if no fade is in progress (e.g. a stray/racing timer
    /// tick after `finish_placeholder_fade` already ran, though `KillTimer`
    /// there should normally prevent that).
    pub(super) fn tick_placeholder_fade(&mut self) {
        let Some(fade) = self.placeholder_fade.as_ref() else {
            return;
        };
        let elapsed_ms = fade.started.elapsed().as_millis() as u32;
        if elapsed_ms >= PLACEHOLDER_FADE_DURATION_MS {
            self.finish_placeholder_fade();
            return;
        }
        let remaining = PLACEHOLDER_FADE_DURATION_MS - elapsed_ms;
        let alpha = ((remaining as u64 * 255) / PLACEHOLDER_FADE_DURATION_MS as u64) as u8;
        // SAFETY: `fade.hwnd.0` is the overlay's live window handle (this
        // method only runs between `start_placeholder_fade` creating it and
        // `finish_placeholder_fade` destroying it); `alpha` is always a
        // valid byte and `LWA_ALPHA` is the flag that makes
        // `SetLayeredWindowAttributes` honor it.
        unsafe {
            SetLayeredWindowAttributes(fade.hwnd.0, 0, alpha, LWA_ALPHA);
        }
    }

    /// Tear down the placeholder-overlay fade: kill its timer, hide and
    /// destroy the overlay window. Idempotent via `Option::take`. Called
    /// once `tick_placeholder_fade` observes the fade duration has elapsed,
    /// and from the resize/rebuild paths (`check_and_call_resize_if_needed`,
    /// `recreate_webgpu_child_window`) when geometry or z-order changes
    /// mid-fade.
    ///
    /// # Reentrancy invariant
    ///
    /// `DestroyWindow` on the overlay child sends `WM_PARENTNOTIFY`
    /// synchronously to the parent window. `do_wnd_proc` currently has no
    /// handler for `WM_PARENTNOTIFY` (it falls through to `DefWindowProcW`),
    /// so this is safe: no code path triggered by the synchronous dispatch
    /// tries to borrow `WindowInner`. If a `WM_PARENTNOTIFY` handler is ever
    /// added, it MUST use `try_borrow`/`try_borrow_mut` (or this teardown
    /// must be deferred out of the borrowed scope), otherwise it will fail
    /// silently or panic under the borrow already held by `wm_timer`'s
    /// `try_borrow_mut` / `tick_placeholder_fade`.
    pub(super) fn finish_placeholder_fade(&mut self) {
        if let Some(fade) = self.placeholder_fade.take() {
            // SAFETY: `self.hwnd.0` is the valid window handle this
            // timer was armed on. (The `wm_ncdestroy` path no longer calls this
            // function -- it just `take()`s the `PlaceholderFade` to drop
            // bookkeeping -- so `self.hwnd.0` is never null here.)
            unsafe {
                KillTimer(self.hwnd.0, PLACEHOLDER_FADE_TIMER_ID);
            }
            if !fade.hwnd.0.is_null() {
                // SAFETY: `fade.hwnd.0` is a live `WS_CHILD` window handle
                // owned by `self.hwnd.0`. All remaining callers of
                // `finish_placeholder_fade` (timer tick, resize, WebGpu child
                // rebuild) reach it only while both the parent and the
                // overlay are still live -- the `wm_ncdestroy` teardown path
                // does NOT call this (it just `take()`s the `PlaceholderFade`
                // to drop bookkeeping), because by `WM_NCDESTROY` the overlay
                // HWND is already dead. `ShowWindow(SW_HIDE)` immediately
                // stops it from occluding anything even if `DestroyWindow`
                // below is deferred/queued by the message loop; explicitly
                // destroying it (rather than just relying on `WS_CHILD`
                // auto-cleanup when `self.hwnd.0` eventually goes away) frees
                // its GDI/DWM-side resources promptly instead of leaving them
                // until the whole top-level window closes, which for a
                // long-lived terminal session could be hours or days away.
                unsafe {
                    ShowWindow(fade.hwnd.0, SW_HIDE);
                    DestroyWindow(fade.hwnd.0);
                }
            }
        }
    }
}

/// Window procedure for the `WS_EX_LAYERED` placeholder-overlay child window
/// (task #385; see `PlaceholderFade` and `Window::create_placeholder_overlay_
/// window`).
///
/// Like `child_wnd_proc`, this window has no `WindowInner`/`GWLP_USERDATA` of
/// its own and is input-transparent (`WM_NCHITTEST` -> `HTTRANSPARENT`, same
/// rationale). `WM_ERASEBKGND`/`WM_PAINT` both paint the parent's spinner via
/// `paint_parent_placeholder_spinner` -- the exact same GDI content the
/// parent/WebGpu-child were showing right up until this overlay appeared, so
/// there is no visible seam at the moment it takes over, only the fade that
/// follows via `SetLayeredWindowAttributes` (driven by `PLACEHOLDER_FADE_
/// TIMER_ID` on the parent's `WindowInner`, not by this window at all -- this
/// wndproc only ever repaints the *content*, never touches alpha itself).
///
/// # Safety
/// This is the `WNDCLASSW::lpfnWndProc` callback: Win32 supplies a valid
/// `hwnd` and the raw message arguments.
unsafe extern "system" fn overlay_wnd_proc(
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
            let hdc = wparam as HDC;
            let mut rect = RECT {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            };
            // SAFETY: `hwnd` is this valid child window handle; `rect` is a
            // live stack `RECT` that `GetClientRect` only writes into.
            GetClientRect(hwnd, &mut rect);
            // SAFETY: `hwnd` is this valid child window handle; `hdc` is the
            // device context Win32 passed in `wparam` of a real
            // `WM_ERASEBKGND` and is live for `rect`, this window's full
            // client area.
            if paint_parent_placeholder_spinner(hwnd, hdc, &rect) {
                return 1;
            }
        }
        if msg == WM_PAINT {
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
            // SAFETY: `hwnd` is this valid child window handle and `ps` is a
            // fully-initialized, live `PAINTSTRUCT`.
            let hdc = BeginPaint(hwnd, &mut ps);
            if !hdc.is_null() {
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
                // SAFETY: `hdc` is the non-null device context just
                // returned by `BeginPaint` and live until `EndPaint`.
                paint_parent_placeholder_spinner(hwnd, hdc, &rect);
            }
            // SAFETY: `hwnd`/`ps` are the same valid handle/live
            // `PAINTSTRUCT` passed to the matching `BeginPaint` above.
            EndPaint(hwnd, &ps);
            return 0;
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
