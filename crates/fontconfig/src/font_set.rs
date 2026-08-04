//! The `FcFontSet` collection type and the font matching/listing/sorting
//! algorithms that populate or consume it (`FcFontMatch`, `FcFontList`,
//! `FcFontSort`, render preparation, ...).
use crate::{FcBool, FcCharSet, FcConfig, FcObjectSet, FcPattern, FcResult};
use libc::c_int;

#[repr(C)]
#[allow(missing_copy_implementations)]
pub struct struct__FcFontSet {
    pub nfont: c_int,
    pub sfont: c_int,
    pub fonts: *mut *mut FcPattern,
}

pub type FcFontSet = struct__FcFontSet;

extern "C" {

    pub fn FcFontSetPrint(s: *mut FcFontSet);

    pub fn FcFontSetCreate() -> *mut FcFontSet;

    pub fn FcFontSetDestroy(s: *mut FcFontSet);

    pub fn FcFontSetAdd(s: *mut FcFontSet, font: *mut FcPattern) -> FcBool;

    pub fn FcFontSetList(
        config: *mut FcConfig,
        sets: *mut *mut FcFontSet,
        nsets: c_int,
        p: *mut FcPattern,
        os: *mut FcObjectSet,
    ) -> *mut FcFontSet;

    pub fn FcFontList(
        config: *mut FcConfig,
        p: *mut FcPattern,
        os: *mut FcObjectSet,
    ) -> *mut FcFontSet;

    pub fn FcFontSetMatch(
        config: *mut FcConfig,
        sets: *mut *mut FcFontSet,
        nsets: c_int,
        p: *mut FcPattern,
        result: *mut FcResult,
    ) -> *mut FcPattern;

    pub fn FcFontMatch(
        config: *mut FcConfig,
        p: *mut FcPattern,
        result: *mut FcResult,
    ) -> *mut FcPattern;

    pub fn FcFontRenderPrepare(
        config: *mut FcConfig,
        pat: *mut FcPattern,
        font: *mut FcPattern,
    ) -> *mut FcPattern;

    pub fn FcFontSetSort(
        config: *mut FcConfig,
        sets: *mut *mut FcFontSet,
        nsets: c_int,
        p: *mut FcPattern,
        trim: FcBool,
        csp: *mut *mut FcCharSet,
        result: *mut FcResult,
    ) -> *mut FcFontSet;

    pub fn FcFontSort(
        config: *mut FcConfig,
        p: *mut FcPattern,
        trim: FcBool,
        csp: *mut *mut FcCharSet,
        result: *mut FcResult,
    ) -> *mut FcFontSet;

    pub fn FcFontSetSortDestroy(fs: *mut FcFontSet);

}
