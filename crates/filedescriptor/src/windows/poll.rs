use crate::{pollfd, Result};
use std::time::Duration;
use winapi::um::winsock2::WSAPoll;

#[doc(hidden)]
pub fn poll_impl(pfd: &mut [pollfd], duration: Option<Duration>) -> Result<usize> {
    // SAFETY: WSAPoll is the winsock polling function; `pfd.as_mut_ptr()` is
    // a valid array of `pfd.len()` pollfd entries.
    let poll_result = unsafe {
        WSAPoll(
            pfd.as_mut_ptr(),
            pfd.len() as _,
            duration
                .map(|wait| wait.as_millis() as libc::c_int)
                .unwrap_or(-1),
        )
    };
    if poll_result < 0 {
        Err(std::io::Error::last_os_error().into())
    } else {
        Ok(poll_result as usize)
    }
}
