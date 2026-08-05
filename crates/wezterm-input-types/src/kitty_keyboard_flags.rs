bitflags::bitflags! {
pub struct KittyKeyboardFlags: u16 {
    const NONE = 0;
    const DISAMBIGUATE_ESCAPE_CODES = 1;
    const REPORT_EVENT_TYPES = 2;
    const REPORT_ALTERNATE_KEYS = 4;
    const REPORT_ALL_KEYS_AS_ESCAPE_CODES = 8;
    const REPORT_ASSOCIATED_TEXT = 16;
}
}
