//! GuiWin represents a Gui TermWindow (as opposed to a Mux window) in lua code
use crate::TermWindow;
use mux::window::WindowId as MuxWindowId;

#[derive(Clone)]
pub struct GuiWin {
    #[allow(dead_code)]
    pub mux_window_id: MuxWindowId,
    pub window: ::window::Window,
}

impl GuiWin {
    pub fn new(term_window: &TermWindow) -> Self {
        let window = term_window.window.clone().unwrap();
        let mux_window_id = term_window.mux_window_id;
        Self {
            window,
            mux_window_id,
        }
    }
}
