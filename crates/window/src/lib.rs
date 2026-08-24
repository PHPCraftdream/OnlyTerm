#![warn(clippy::undocumented_unsafe_blocks)]
use async_trait::async_trait;
use bitflags::bitflags;
use config::window::WindowLevel;
use config::{ConfigHandle, Dimension, GeometryOrigin};
use promise::Future;
use std::any::Any;
use std::path::PathBuf;
use url::Url;
pub mod bitmaps;
pub use onlyterm_color_types as color;
pub mod connection;
pub mod os;
pub mod screen;
mod spawn;

pub use raw_window_handle;

pub(crate) const DEFAULT_DPI: f64 = 96.0;

pub fn default_dpi() -> f64 {
    match Connection::get() {
        Some(conn) => conn.default_dpi(),
        None => DEFAULT_DPI,
    }
}

pub use bitmaps::{BitmapImage, Image};
pub use connection::*;
pub use onlyterm_input_types::*;
pub use os::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Clipboard {
    #[default]
    Clipboard,
    PrimarySelection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dimensions {
    pub pixel_width: usize,
    pub pixel_height: usize,
    pub dpi: usize,
}

pub type ULength = euclid::Length<usize, PixelUnit>;
pub type Rect = euclid::Rect<isize, PixelUnit>;
pub type RectF = euclid::Rect<f32, PixelUnit>;
pub type Size = euclid::Size2D<isize, PixelUnit>;
pub type SizeF = euclid::Size2D<f32, PixelUnit>;
pub type ScreenRect = euclid::Rect<isize, ScreenPixelUnit>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseCursor {
    Arrow,
    Hand,
    Text,
    SizeUpDown,
    SizeLeftRight,
}

/// Represents the preferred appearance of the windowing
/// environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Appearance {
    /// Standard dark-text-on-light-background presentation
    Light,
    /// Dark mode, with predominantly dark or muted colors
    Dark,
    /// dark-text-on-light-background, but in a higher contrast
    /// more accesible palette
    LightHighContrast,
    /// darker background but with higher contrast than regular
    /// dark mode
    DarkHighContrast,
}

impl std::fmt::Display for Appearance {
    fn fmt(&self, form: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Light => "Light",
            Self::Dark => "Dark",
            Self::LightHighContrast => "LightHighContrast",
            Self::DarkHighContrast => "DarkHighContrast",
        };
        form.write_str(s)
    }
}

bitflags! {
    #[derive(Default)]
    pub struct WindowState: u8 {
        /// Occupies the whole screen; cannot be resized while in this state.
        const FULL_SCREEN = 1<<1;
        /// Maximized along either or both of horizontal or vertical dimensions;
        /// cannot be resized while in this state.
        const MAXIMIZED = 1<<2;
        /// Minimized or in some kind of off-screen state. Cannot be repainted
        /// while in this state.
        const HIDDEN = 1<<3;
        /// Always on top (floating) window
        const ALWAYS_ON_TOP = 1<<4;
        /// Always on bottom (docked) window
        const ALWAYS_ON_BOTTOM = 1<<5;
        /// Tiled by the window manager along one or more edges (eg: a tiling
        /// Wayland compositor such as sway). The compositor owns the window
        /// size, so the application must not resize itself.
        const TILED = 1<<6;
    }
}

impl WindowState {
    pub fn can_resize(self) -> bool {
        !self.intersects(Self::FULL_SCREEN | Self::MAXIMIZED | Self::TILED)
    }

    pub fn can_paint(self) -> bool {
        !self.contains(Self::HIDDEN)
    }

    pub fn as_window_level(self) -> WindowLevel {
        if self.contains(Self::ALWAYS_ON_TOP) {
            WindowLevel::AlwaysOnTop
        } else if self.contains(Self::ALWAYS_ON_BOTTOM) {
            WindowLevel::AlwaysOnBottom
        } else {
            WindowLevel::Normal
        }
    }
}

#[derive(Debug, Clone)]
pub enum WindowKeyEvent {
    RawKeyEvent(RawKeyEvent),
    KeyEvent(KeyEvent),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeadKeyStatus {
    /// Not in a dead key processing hold
    None,
    /// Holding until composition is done; the string is the uncommitted
    /// composition text to show as a placeholder
    Composing(String),
}

#[derive(Debug)]
pub enum WindowEvent {
    /// Called when the window close button is clicked.
    /// The window closure is deferred and this event is
    /// sent to your application to decide whether it will
    /// really close the window.
    CloseRequested,

    /// Called when the window is being destroyed by the window system
    Destroyed,

    /// Called when the window has been resized
    Resized {
        dimensions: Dimensions,
        window_state: WindowState,
        live_resizing: bool,
    },

    /// Called when a program-requested set_inner_size() has finished
    SetInnerSizeCompleted,

    /// Called when the window has been invalidated and needs to
    /// be repainted
    NeedRepaint,

    /// Called when the window gains/loses focus
    FocusChanged(bool),

    AdviseDeadKeyStatus(DeadKeyStatus),

    /// Called to handle a raw key event, prior to any dead key,
    /// keymap composition or other higher level treatment.
    /// If you handle this key event, you must call
    /// event.set_handled() to prevent additional processing.
    RawKeyEvent(RawKeyEvent),

    /// Called to handle a key event.
    KeyEvent(KeyEvent),

    MouseEvent(MouseEvent),
    MouseLeave,

    AppearanceChanged(Appearance),

    Notification(Box<dyn Any + Send + Sync>),

    // Called when the files are being dragged into the window
    DraggedFile(Vec<PathBuf>),

    // Called when the files are dropped into the window
    DroppedFile(Vec<PathBuf>),

    // Called when urls are dropped into the window
    DroppedUrl(Vec<Url>),

    // Called when text is dropped into the window
    DroppedString(String),

    /// Called by menubar dispatching stuff on some systems
    PerformKeyAssignment(config::keyassignment::KeyAssignment),

    AdviseModifiersLedStatus(Modifiers, KeyboardLedStatus),
}

type WindowEventHandler = Box<dyn FnMut(WindowEvent, &Window)>;

pub struct WindowEventSender {
    handler: WindowEventHandler,
    window: Option<Window>,
}

impl WindowEventSender {
    pub fn new<F: 'static + FnMut(WindowEvent, &Window)>(handler: F) -> Self {
        Self {
            handler: Box::new(handler),
            window: None,
        }
    }

    pub(crate) fn assign_window(&mut self, window: Window) {
        self.window.replace(window);
    }

    pub fn dispatch(&mut self, event: WindowEvent) {
        if let Some(window) = self.window.as_ref() {
            log::trace!("{:?}", event);
            (self.handler)(event, window);
        }
    }
}

#[async_trait(?Send)]
pub trait WindowOps {
    /// Show a hidden window
    fn show(&self);

    fn notify<T: Any + Send + Sync>(&self, t: T)
    where
        Self: Sized;

    /// Dispatch notification inline when already on the main thread.
    /// This avoids an unnecessary spawn through
    /// `Connection::with_window_inner` (which was spawn #3 in the
    /// PaneOutput delivery path). Panics if called from a non-main thread.
    fn notify_inline<T: Any + Send + Sync>(&self, t: T)
    where
        Self: Sized;

    /// Hide a visible window
    fn hide(&self);

    /// Schedule the window to be closed
    fn close(&self);

    /// Change the cursor
    fn set_cursor(&self, cursor: Option<MouseCursor>);

    /// Invalidate the window so that the entire client area will
    /// be repainted shortly
    fn invalidate(&self);

    /// Release any placeholder background painted before the renderer's
    /// first frame (Windows-only concern -- see
    /// `os::windows::window::WindowInner::placeholder_spinner`'s doc
    /// comment for the full rationale). Called the first time a real frame
    /// has actually been *presented* (task #425, hardened by task #407),
    /// not merely once a renderer object exists or a frame has merely been
    /// handed off for presentation -- the caller is `onlyterm-gui`'s
    /// `TermWindow::paint_impl` on the synchronous (no dedicated render
    /// thread) path, or its `renderthread.rs`'s `submit_one_frame` after
    /// its first successful `submit_frame` when a render thread is active
    /// (the Windows default) -- see either call site's comment for the full
    /// reasoning. No-op default for platforms that never needed a
    /// placeholder: on those, the window either isn't shown before the
    /// renderer is ready, or its native window class doesn't leave the
    /// client area unpainted the way Windows' `hbrBackground: null_mut()`
    /// class does.
    fn clear_placeholder_background(&self) {}

    /// Signal that the shell running in this window's pane(s) has produced
    /// its first output, used as a practical proxy for "the shell is alive
    /// and likely ready to accept input" (task #385 -- there is no harder
    /// "ready for input" handshake to wait for; see the call site in
    /// `onlyterm-gui`'s `TermWindow` for the exact trigger). Windows-only
    /// concern, like `clear_placeholder_background`: it gates the placeholder
    /// spinner's cross-fade into the real terminal content
    /// (`os::windows::window::WindowInner::start_placeholder_fade`), which
    /// only starts once both this and a working renderer are in place. No-op
    /// default for platforms that never painted a placeholder to begin with.
    fn notify_shell_ready(&self) {}

    /// Change the titlebar text for the window
    fn set_title(&self, title: &str);

    /// Resize the inner or client area of the window
    fn set_inner_size(&self, width: usize, height: usize);

    /// Use for windows snap layouts
    fn set_maximize_button_position(&self, _rect: ScreenRect) {}

    /// Requests the windowing system to start a window drag.
    ///
    /// This is only implemented on backends that handle
    /// window movement on the server side (Wayland).
    fn request_drag_move(&self) {}

    /// Signal to the windowing system that the mouse is over
    /// a window dragging area.
    ///
    /// This is only implemented on backends that need to
    /// know if the mouse is in a drag area to handle the
    /// click before forwarding the event (Windows).
    fn set_window_drag_position(&self, _coords: ScreenPoint) {}

    /// Changes the location of the window on the screen.
    /// The coordinates are of the top left pixel of the
    /// client area.
    ///
    /// This is only implemented on backends that allow
    /// windows to move themselves (not Wayland).
    fn set_window_position(&self, _coords: ScreenPoint) {}

    /// inform the windowing system of the current textual
    /// cursor input location.  This is used primarily for
    /// the platform specific input method editor
    fn set_text_cursor_position(&self, _cursor: Rect) {}

    /// Initiate textual transfer from the clipboard
    fn get_clipboard(&self, clipboard: Clipboard) -> Future<String>;

    /// Set some text in the clipboard
    fn set_clipboard(&self, clipboard: Clipboard, text: String);

    /// Set window level. Depending on the environment and user preferences
    fn set_window_level(&self, _level: WindowLevel) {}

    /// Set the icon for the window.
    /// Depending on the system this may be shown in its titlebar
    /// and/or in the task manager/task switcher
    fn set_icon(&self, _image: Image) {}

    fn maximize(&self) {}
    fn restore(&self) {}
    fn focus(&self) {}

    fn toggle_fullscreen(&self) {}

    fn config_did_change(&self, _config: &config::ConfigHandle) {}

    /// Configure the Window so that the desktop environment
    /// will constrain resizes so that they are multiples of
    /// the x and y values specified.
    /// This may not be supported or respected by the desktop
    /// environment.
    fn set_resize_increments(&self, _incr: ResizeIncrement) {}

    fn get_os_parameters(
        &self,
        _config: &ConfigHandle,
        _window_state: WindowState,
    ) -> anyhow::Result<Option<os::parameters::Parameters>> {
        Ok(None)
    }
}

#[derive(Debug, Clone, Default)]
pub struct RequestedWindowGeometry {
    pub width: Dimension,
    pub height: Dimension,
    pub x: Option<Dimension>,
    pub y: Option<Dimension>,
    /// Specifies basis for evaluating x/y coords.
    /// Also applies to width/height when computing % based dimensions
    pub origin: GeometryOrigin,
}

#[derive(Debug, Clone)]
pub struct ResolvedGeometry {
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub width: usize,
    pub height: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct ResizeIncrement {
    pub x: u16,
    pub y: u16,
    pub base_width: u16,
    pub base_height: u16,
}

impl ResizeIncrement {
    /// Use this as a readable shorthand for disabling the feature
    pub fn disabled() -> Self {
        Self {
            x: 1,
            y: 1,
            base_width: 0,
            base_height: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::WindowState;

    #[test]
    fn empty_state_can_resize_and_paint() {
        let state = WindowState::default();
        assert!(state.can_resize(), "empty state should be resizable");
        assert!(state.can_paint(), "empty state should be paintable");
    }

    #[test]
    fn fullscreen_and_maximized_block_resize() {
        // Established behavior that must not regress when TILED is added
        // to the can_resize intersection.
        assert!(!WindowState::FULL_SCREEN.can_resize());
        assert!(!WindowState::MAXIMIZED.can_resize());
        // HIDDEN is intentionally NOT in the can_resize intersection:
        // it blocks painting, not resizing.
        assert!(WindowState::HIDDEN.can_resize());
    }

    #[test]
    fn tiled_blocks_resize() {
        // A window tiled by the compositor (eg. sway) must not resize
        // itself; the compositor owns the geometry.
        assert!(
            !WindowState::TILED.can_resize(),
            "tiled window must not be resizable"
        );
    }

    #[test]
    fn tiled_combined_with_other_flags_blocks_resize() {
        // Any combination that includes TILED must block resizing, even
        // if none of the legacy blocking bits (FULL_SCREEN/MAXIMIZED) are set.
        assert!(
            WindowState::empty().can_resize(),
            "empty state is resizable"
        );
        assert!(!(WindowState::TILED | WindowState::ALWAYS_ON_TOP).can_resize());
        assert!(!(WindowState::TILED | WindowState::HIDDEN).can_resize());
        assert!(
            !(WindowState::TILED | WindowState::FULL_SCREEN | WindowState::MAXIMIZED).can_resize()
        );
    }

    #[test]
    fn non_blocking_flags_remain_resizable() {
        // Flags unrelated to geometry ownership must not block resizing.
        assert!((WindowState::ALWAYS_ON_TOP).can_resize());
        assert!((WindowState::ALWAYS_ON_BOTTOM).can_resize());
        assert!((WindowState::ALWAYS_ON_TOP | WindowState::ALWAYS_ON_BOTTOM).can_resize());
    }
}
