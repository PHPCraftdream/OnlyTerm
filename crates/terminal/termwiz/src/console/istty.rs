//! Making it a little more convenient and safe to query whether
//! something is a terminal teletype or not.
//! This module defines the IsTty trait and the is_tty method to
//! return true if the item represents a terminal.
use std::os::windows::io::AsRawHandle;
use winapi::um::consoleapi::GetConsoleMode;

/// Adds the is_tty method to types that might represent a terminal
pub trait IsTty {
    /// Returns true if the instance is a terminal teletype, false
    /// otherwise.
    fn is_tty(&self) -> bool;
}

impl<S: AsRawHandle> IsTty for S {
    fn is_tty(&self) -> bool {
        let mut mode = 0;
        // SAFETY: `self.as_raw_handle()` is a valid raw handle for the lifetime
        // of the call; `mode` is a valid u32 out-pointer live for the call.
        let ok = unsafe { GetConsoleMode(self.as_raw_handle() as *mut _, &mut mode) };
        ok == 1
    }
}
