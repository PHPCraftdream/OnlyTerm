//! The opaque `FcStrSet`/`FcStrList` collection types and the `FcStr*`
//! string-manipulation free functions (fontconfig's own `FcChar8`-based
//! string helpers, distinct from Rust `str`/`String`).
use crate::{FcBool, FcChar8};
use libc::{c_int, c_void};

pub type struct__FcStrList = c_void;

pub type FcStrList = struct__FcStrList;

pub type struct__FcStrSet = c_void;

pub type FcStrSet = struct__FcStrSet;

extern "C" {

    pub fn FcStrCopy(s: *const FcChar8) -> *mut FcChar8;

    pub fn FcStrCopyFilename(s: *const FcChar8) -> *mut FcChar8;

    pub fn FcStrPlus(s1: *const FcChar8, s2: *const FcChar8) -> *mut FcChar8;

    pub fn FcStrFree(s: *mut FcChar8);

    pub fn FcStrDowncase(s: *const FcChar8) -> *mut FcChar8;

    pub fn FcStrCmpIgnoreCase(s1: *const FcChar8, s2: *const FcChar8) -> c_int;

    pub fn FcStrCmp(s1: *const FcChar8, s2: *const FcChar8) -> c_int;

    pub fn FcStrStrIgnoreCase(s1: *const FcChar8, s2: *const FcChar8) -> *mut FcChar8;

    pub fn FcStrStr(s1: *const FcChar8, s2: *const FcChar8) -> *mut FcChar8;

    pub fn FcStrDirname(file: *const FcChar8) -> *mut FcChar8;

    pub fn FcStrBasename(file: *const FcChar8) -> *mut FcChar8;

    pub fn FcStrSetCreate() -> *mut FcStrSet;

    pub fn FcStrSetMember(set: *mut FcStrSet, s: *const FcChar8) -> FcBool;

    pub fn FcStrSetEqual(sa: *mut FcStrSet, sb: *mut FcStrSet) -> FcBool;

    pub fn FcStrSetAdd(set: *mut FcStrSet, s: *const FcChar8) -> FcBool;

    pub fn FcStrSetAddFilename(set: *mut FcStrSet, s: *const FcChar8) -> FcBool;

    pub fn FcStrSetDel(set: *mut FcStrSet, s: *const FcChar8) -> FcBool;

    pub fn FcStrSetDestroy(set: *mut FcStrSet);

    pub fn FcStrListCreate(set: *mut FcStrSet) -> *mut FcStrList;

    pub fn FcStrListNext(list: *mut FcStrList) -> *mut FcChar8;

    pub fn FcStrListDone(list: *mut FcStrList);

}
