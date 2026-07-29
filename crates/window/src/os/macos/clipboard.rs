use crate::macos::{nsstring, nsstring_to_str};
use cocoa::appkit::{NSFilenamesPboardType, NSPasteboard, NSStringPboardType};
use cocoa::base::*;
use cocoa::foundation::NSArray;

pub struct Clipboard {
    pasteboard: id,
}

impl Clipboard {
    pub fn new() -> Self {
        // SAFETY: NSPasteboard::generalPasteboard is the documented AppKit API
        // for obtaining the system pasteboard; passing nil as the receiver is the
        // standard usage and it returns a valid non-null pasteboard (verified
        // below before storing/using it).
        let pasteboard = unsafe { NSPasteboard::generalPasteboard(nil) };
        if pasteboard.is_null() {
            panic!("NSPasteboard::generalPasteboard returned null");
        }
        Clipboard { pasteboard }
    }

    pub fn read(&self) -> anyhow::Result<String> {
        // SAFETY: self.pasteboard is a valid non-null NSPasteboard obtained in
        // new(); propertyListForType/stringForType/count/objectAtIndex are AppKit
        // FFI trait methods on it. Every returned id is null-checked before being
        // passed to nsstring_to_str, and objectAtIndex is only called with indices
        // bounded by count().
        unsafe {
            let plist = self.pasteboard.propertyListForType(NSFilenamesPboardType);
            if !plist.is_null() {
                let mut filenames = vec![];
                for i in 0..plist.count() {
                    filenames.push(
                        shlex::try_quote(nsstring_to_str(plist.objectAtIndex(i)))
                            .unwrap_or_else(|_| "".into()),
                    );
                }
                return Ok(filenames.join(" "));
            }
            let s = self.pasteboard.stringForType(NSStringPboardType);
            if !s.is_null() {
                let str = nsstring_to_str(s);
                return Ok(str.to_string());
            }
        }
        anyhow::bail!("pasteboard read returned empty");
    }

    pub fn write(&mut self, data: String) -> anyhow::Result<()> {
        // SAFETY: clearContents/writeObjects are AppKit FFI on a valid
        // pasteboard. nsstring(&data) produces a valid autoreleased NSString and
        // NSArray::arrayWithObject wraps it in a valid single-element array that
        // is consumed synchronously by writeObjects.
        unsafe {
            self.pasteboard.clearContents();
            let success: BOOL = self
                .pasteboard
                .writeObjects(NSArray::arrayWithObject(nil, *nsstring(&data)));
            anyhow::ensure!(success == YES, "pasteboard write returned false");
            Ok(())
        }
    }
}
