//! The opaque `FcCharSet` type and its operations (create/copy/destroy,
//! set algebra, membership tests and page-based coverage iteration).
use crate::{FcBool, FcChar32};
use libc::*;

pub type struct__FcCharSet = c_void;

pub type FcCharSet = struct__FcCharSet;

extern "C" {

    pub fn FcCharSetCreate() -> *mut FcCharSet;

    pub fn FcCharSetNew() -> *mut FcCharSet;

    pub fn FcCharSetDestroy(fcs: *mut FcCharSet);

    pub fn FcCharSetAddChar(fcs: *mut FcCharSet, ucs4: FcChar32) -> FcBool;

    pub fn FcCharSetCopy(src: *mut FcCharSet) -> *mut FcCharSet;

    pub fn FcCharSetEqual(a: *const FcCharSet, b: *const FcCharSet) -> FcBool;

    pub fn FcCharSetIntersect(a: *const FcCharSet, b: *const FcCharSet) -> *mut FcCharSet;

    pub fn FcCharSetUnion(a: *const FcCharSet, b: *const FcCharSet) -> *mut FcCharSet;

    pub fn FcCharSetSubtract(a: *const FcCharSet, b: *const FcCharSet) -> *mut FcCharSet;

    pub fn FcCharSetMerge(a: *mut FcCharSet, b: *const FcCharSet, changed: *mut FcBool) -> FcBool;

    pub fn FcCharSetHasChar(fcs: *const FcCharSet, ucs4: FcChar32) -> FcBool;

    pub fn FcCharSetCount(a: *const FcCharSet) -> FcChar32;

    pub fn FcCharSetIntersectCount(a: *const FcCharSet, b: *const FcCharSet) -> FcChar32;

    pub fn FcCharSetSubtractCount(a: *const FcCharSet, b: *const FcCharSet) -> FcChar32;

    pub fn FcCharSetIsSubset(a: *const FcCharSet, bi: *const FcCharSet) -> FcBool;

    pub fn FcCharSetFirstPage(
        a: *const FcCharSet,
        map: *mut FcChar32,
        next: *mut FcChar32,
    ) -> FcChar32;

    pub fn FcCharSetNextPage(
        a: *const FcCharSet,
        map: *mut FcChar32,
        next: *mut FcChar32,
    ) -> FcChar32;

    pub fn FcCharSetCoverage(
        a: *const FcCharSet,
        page: FcChar32,
        result: *mut FcChar32,
    ) -> FcChar32;

}
