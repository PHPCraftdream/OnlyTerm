#[cfg(feature = "serde")]
use ::serde::*;

bitflags::bitflags! {
/// The set of kitty keyboard protocol progressive-enhancement flags that an
/// application has negotiated.
///
/// This is serializable (behind the `serde` feature) because the mux
/// protocol has to be able to tell a remote client which keyboard encoding
/// the pane's application has negotiated; see
/// `codec::GetPaneRenderChangesResponse::keyboard_encoding`.
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct KittyKeyboardFlags: u16 {
    const NONE = 0;
    const DISAMBIGUATE_ESCAPE_CODES = 1;
    const REPORT_EVENT_TYPES = 2;
    const REPORT_ALTERNATE_KEYS = 4;
    const REPORT_ALL_KEYS_AS_ESCAPE_CODES = 8;
    const REPORT_ASSOCIATED_TEXT = 16;
}
}
