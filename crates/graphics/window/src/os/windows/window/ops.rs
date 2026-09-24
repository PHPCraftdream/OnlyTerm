use super::input::ime::ImmContext;
use super::msg::frame::{decorations_to_style, get_window_state};
use super::msg::mouse::{apply_mouse_cursor, client_to_screen};
use super::msg::paint::wm_paint;
use super::*;
use crate::connection::ConnectionOps;
use crate::parameters::{self, Parameters};
use crate::{
    Clipboard, MouseCursor, Point, Rect, ScreenPoint, ScreenRect, ULength, WindowDecorations,
    WindowEvent, WindowOps, WindowState,
};
use anyhow::Context;
use async_trait::async_trait;
use onlyterm_color_types::LinearRgba;
use onlyterm_config::{ConfigHandle, ImePreeditRendering};
use onlyterm_promise::Future;
use std::any::Any;
use windows::UI::ViewManagement::{UIColorType, UISettings};
use winreg::enums::HKEY_CURRENT_USER;
use winreg::RegKey;

impl WindowInner {
    fn close(&mut self) {
        // Eagerly destroy any retired WebGpu child HWNDs we can right now
        // (task #283). Not strictly required for correctness -- any left
        // in `retired_webgpu_children` are still `WS_CHILD` windows of
        // `self.hwnd`, which Windows destroys automatically as part of
        // `hwnd`'s own teardown below -- but doing it here avoids relying
        // on that implicit cleanup when we can just as easily check now.
        self.sweep_retired_webgpu_children();
        let hwnd = self.hwnd;
        onlyterm_promise::spawn::spawn(async move {
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
        onlyterm_promise::spawn::spawn(async move {
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
        self.caption.set_text(title, None);
        self.set_native_title(title);
    }

    fn set_native_title(&mut self, title: &str) {
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

    pub(super) fn set_ime_window_position(&mut self, mut cursor: Rect) {
        // SAFETY: IME coordinates must include the same live caption inset as mouse input.
        cursor.origin.y += unsafe { caption::height(self.hwnd.0) } as isize;
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
                onlyterm_promise::spawn::spawn(async move {
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
                onlyterm_promise::spawn::spawn(async move {
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

impl Window {
    /// Native title stays concise for accessibility; status is painted independently.
    pub fn set_title_and_status(&self, title: &str, status: Option<(&str, &str)>) {
        let title = title.to_owned();
        let status = status.map(|(full, compact)| (full.to_owned(), compact.to_owned()));
        Connection::with_window_inner(self.0, move |inner| {
            if inner.caption.set_text(
                &title,
                status.as_ref().map(|(a, b)| (a.as_str(), b.as_str())),
            ) {
                let native = match status {
                    Some((full, _)) => format!("{title} — [{full}]"),
                    None => title,
                };
                inner.set_native_title(&native);
            }
            Ok(())
        });
    }
}

#[async_trait(?Send)]
impl WindowOps for Window {
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

    #[cfg(windows)]
    fn notify_inline<T: Any + Send + Sync>(&self, t: T)
    where
        Self: Sized,
    {
        let conn = Connection::get().expect("Connection::init has not been called");
        let current_id = std::thread::current().id();
        let main_id = conn.main_thread_id.expect("main_thread_id not set");
        if current_id != main_id {
            panic!("notify_inline called from non-main thread");
        }
        if let Some(handle) = conn.get_window(self.0) {
            let mut inner = handle.borrow_mut();
            inner
                .events
                .dispatch(WindowEvent::Notification(Box::new(t)));
        }
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
        onlyterm_promise::spawn::spawn(async move {
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
        // SAFETY: live window; caption invalidation is independent from terminal frames.
        unsafe {
            caption::invalidate_content(hwnd);
        }
    }

    fn clear_placeholder_background(&self) {
        Connection::with_window_inner(self.0, move |inner| {
            inner.clear_placeholder_background();
            Ok(())
        });
    }

    fn notify_shell_ready(&self) {
        Connection::with_window_inner(self.0, move |inner| {
            inner.notify_shell_ready();
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
            onlyterm_promise::spawn::spawn(async move {
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
