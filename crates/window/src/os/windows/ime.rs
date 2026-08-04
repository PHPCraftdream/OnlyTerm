use crate::Rect;
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use winapi::shared::minwindef::*;
use winapi::shared::ntdef::*;
use winapi::shared::windef::*;
use winapi::um::imm::*;

#[allow(non_snake_case)]
#[repr(C)]
pub struct CANDIDATEFORM {
    dwIndex: DWORD,
    dwStyle: DWORD,
    ptCurrentPos: POINT,
    rcArea: RECT,
}
pub type LPCANDIDATEFORM = *mut CANDIDATEFORM;

extern "system" {
    pub fn ImmGetCompositionStringW(himc: HIMC, index: DWORD, buf: LPVOID, buflen: DWORD) -> LONG;
    pub fn ImmSetCandidateWindow(himc: HIMC, lpCandidate: LPCANDIDATEFORM) -> BOOL;
}
/// Helper for managing the IME Manager
pub(super) struct ImmContext {
    hwnd: HWND,
    imc: HIMC,
}

impl ImmContext {
    /// Obtain the IMM context; it will be released automatically
    /// when dropped
    pub fn get(hwnd: HWND) -> Self {
        Self {
            hwnd,
            // SAFETY: `hwnd` is a valid window handle; `ImmGetContext` returns an
            // HIMC that is released in `Drop`.
            imc: unsafe { ImmGetContext(hwnd) },
        }
    }

    /// Set the position of the IME candidate window relative to the cursor.
    pub fn set_candidate_window_position(&self, cursor: Rect) {
        let mut cf = CANDIDATEFORM {
            dwIndex: 0,
            // Don't draw the IME candidate window on the cursor
            // to prevent the window from hiding composition (preedit) string
            dwStyle: CFS_EXCLUDE,
            // cursor position the IME candidate window bases on
            ptCurrentPos: POINT {
                x: cursor.origin.x.max(0) as i32,
                y: cursor.origin.y.max(0) as i32,
            },
            // cursor rectangle the IME candidate window excludes
            rcArea: RECT {
                left: cursor.min_x().max(0) as i32,
                top: cursor.min_y().max(0) as i32,
                right: cursor.max_x().max(0) as i32,
                bottom: cursor.max_y().max(0) as i32,
            },
        };
        // SAFETY: `self.imc` is a valid HIMC and `cf` is a fully-initialized
        // `CANDIDATEFORM`.
        unsafe {
            ImmSetCandidateWindow(self.imc, &mut cf);
        }
    }

    /// Set the position of the IME composition window relative to the cursor.
    pub fn set_composition_window_position(&self, cursor: Rect) {
        let mut cf = COMPOSITIONFORM {
            dwStyle: CFS_POINT,
            ptCurrentPos: POINT {
                x: cursor.origin.x.max(0) as i32,
                y: cursor.origin.y.max(0) as i32,
            },
            rcArea: RECT::default(),
        };
        // SAFETY: `self.imc` is a valid HIMC and `cf` is a fully-initialized
        // `COMPOSITIONFORM`.
        unsafe {
            ImmSetCompositionWindow(self.imc, &mut cf);
        }
    }

    pub fn get_str(&self, which: DWORD) -> Result<String, OsString> {
        // This returns a size in bytes even though it is for a buffer of u16!
        // SAFETY: a null buffer/zero size queries the byte length without writing.
        let byte_size =
            unsafe { ImmGetCompositionStringW(self.imc, which, std::ptr::null_mut(), 0) };
        if byte_size > 0 {
            let word_size = byte_size as usize / 2;
            let mut wide_buf = vec![0u16; word_size];
            // SAFETY: `wide_buf` holds `word_size` `u16`s and `byte_size` matches
            // the queried length, so the write is in-bounds; `self.imc` is valid.
            unsafe {
                ImmGetCompositionStringW(
                    self.imc,
                    which,
                    wide_buf.as_mut_ptr() as *mut _,
                    byte_size as u32,
                )
            };
            OsString::from_wide(&wide_buf).into_string()
        } else {
            Ok(String::new())
        }
    }
}

impl Drop for ImmContext {
    fn drop(&mut self) {
        // SAFETY: `self.hwnd`/`self.imc` are the valid pair obtained in `get`;
        // `ImmReleaseContext` releases the context exactly once.
        unsafe {
            ImmReleaseContext(self.hwnd, self.imc);
        }
    }
}
