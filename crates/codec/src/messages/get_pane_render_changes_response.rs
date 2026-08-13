use std::collections::HashMap;
use std::ops::Range;

use mux::pane::PaneId;
use mux::renderable::{RenderableDimensions, StableCursorPosition};
use mux::tab::SerdeUrl;
use serde::{Deserialize, Serialize};
use termwiz::input::KeyboardEncoding;
use termwiz::surface::SequenceNo;
use wezterm_term::StableRowIndex;

use crate::input_serial::InputSerial;
use crate::lines::SerializedLines;

/// `KeyboardEncoding` has no `Default` impl of its own, and `Xterm` is what
/// the `Pane` trait itself reports when nothing has been negotiated, so it is
/// the right "no protocol in play" value for a payload that omitted the field.
fn no_keyboard_encoding_negotiated() -> KeyboardEncoding {
    KeyboardEncoding::Xterm
}

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct GetPaneRenderChangesResponse {
    pub pane_id: PaneId,
    pub mouse_grabbed: bool,
    pub cursor_position: StableCursorPosition,
    pub dimensions: RenderableDimensions,
    pub dirty_lines: Vec<Range<StableRowIndex>>,
    pub title: String,
    pub working_dir: Option<SerdeUrl>,
    /// Lines that the server thought we'd almost certainly
    /// want to fetch as soon as we received this response
    pub bonus_lines: SerializedLines,

    pub input_serial: Option<InputSerial>,
    pub seqno: SequenceNo,
    #[serde(default)]
    pub user_vars: HashMap<String, String>,

    /// The keyboard encoding protocol (win32-input-mode / kitty / CSI-u /
    /// none) that the application running in this pane has negotiated with
    /// the *server-side* terminal.
    ///
    /// The client-side `ClientPane` has no terminal of its own, so without
    /// this it can only report the `Pane` trait's default of
    /// `KeyboardEncoding::Xterm` ("nothing negotiated"), which makes the GUI
    /// send raw legacy bytes for synthetic key events (`SendEnterOrNewline`,
    /// `SendChar`, `CopySelectionOrInterrupt`) to applications that
    /// explicitly asked for an encoded form. Piggy-backing on this response
    /// -- rather than adding a dedicated push PDU -- means a freshly attached
    /// pane learns the *current* encoding with its first render update,
    /// including protocol negotiation that happened before the client
    /// attached.
    ///
    /// `PerPane::compute_changes` diffs this field, so a DEC private mode
    /// change that alters nothing else on screen still produces a push.
    #[serde(default = "no_keyboard_encoding_negotiated")]
    pub keyboard_encoding: KeyboardEncoding,
}
