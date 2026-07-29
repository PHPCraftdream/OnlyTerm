#![allow(unexpected_cfgs)] // <https://github.com/SSheldon/rust-objc/issues/125>
use cocoa::base::{id, nil};
use cocoa::foundation::NSString;
use objc::rc::StrongPtr;
use objc::runtime::Object;
use objc::*;

mod app;
pub mod bitmap;
pub mod clipboard;
pub mod connection;
pub mod menu;
pub mod window;

mod keycodes;

pub use self::window::*;
pub use bitmap::*;
pub use connection::*;

/// Convert a rust string to a cocoa string
fn nsstring(s: &str) -> StrongPtr {
    // SAFETY: `NSString::alloc` returns a valid instance which `init_str` initializes.
    unsafe { StrongPtr::new(NSString::alloc(nil).init_str(s)) }
}

/// # Safety
/// `ns` must be a valid `NSString` (or `NSAttributedString`); `UTF8String`/`len`
/// then yield a valid UTF-8 buffer. The returned `&str` is valid for as long as
/// the caller keeps `ns` alive (the lifetime is asserted, not enforced).
unsafe fn nsstring_to_str<'a>(mut ns: *mut Object) -> &'a str {
    let is_astring: bool = msg_send![ns, isKindOfClass: class!(NSAttributedString)];
    if is_astring {
        ns = msg_send![ns, string];
    }
    let data = NSString::UTF8String(ns as id) as *const u8;
    let len = NSString::len(ns as id);
    // SAFETY: `data` points at `len` valid bytes produced by `UTF8String`.
    let bytes = std::slice::from_raw_parts(data, len);
    // SAFETY: `NSString` contents round-trip through `UTF8String` as valid UTF-8.
    std::str::from_utf8_unchecked(bytes)
}
