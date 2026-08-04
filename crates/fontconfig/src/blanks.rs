//! The opaque `FcBlanks` type: a set of codepoints considered "blank" and
//! ignored when a font is otherwise empty for a character.
use crate::{FcBool, FcChar32};
use libc::c_void;

pub type struct__FcBlanks = c_void;

pub type FcBlanks = struct__FcBlanks;

extern "C" {

    pub fn FcBlanksCreate() -> *mut FcBlanks;

    pub fn FcBlanksDestroy(b: *mut FcBlanks);

    pub fn FcBlanksAdd(b: *mut FcBlanks, ucs4: FcChar32) -> FcBool;

    pub fn FcBlanksIsMember(b: *mut FcBlanks, ucs4: FcChar32) -> FcBool;

}
