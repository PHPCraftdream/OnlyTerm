//! UTF-8/UTF-16 <-> UCS-4 conversion helpers used internally by fontconfig
//! when it needs to walk fontconfig's own `FcChar8` byte strings as
//! codepoints.
use crate::{FcBool, FcChar32, FcChar8, FcEndian};
use libc::c_int;

extern "C" {

    pub fn FcUtf8ToUcs4(src_orig: *mut FcChar8, dst: *mut FcChar32, len: c_int) -> c_int;

    pub fn FcUtf8Len(
        string: *mut FcChar8,
        len: c_int,
        nchar: *mut c_int,
        wchar: *mut c_int,
    ) -> FcBool;

    pub fn FcUcs4ToUtf8(ucs4: FcChar32, dest: *mut FcChar8) -> c_int;

    pub fn FcUtf16ToUcs4(
        src_orig: *mut FcChar8,
        endian: FcEndian,
        dst: *mut FcChar32,
        len: c_int,
    ) -> c_int;

    pub fn FcUtf16Len(
        string: *mut FcChar8,
        endian: FcEndian,
        len: c_int,
        nchar: *mut c_int,
        wchar: *mut c_int,
    ) -> FcBool;

}
