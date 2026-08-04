//! The opaque `FcLangSet` type (a set of RFC 3066 language tags a font
//! supports) and its comparison result enum, plus the free functions that
//! build/query language coverage.
use crate::{FcBool, FcChar32, FcChar8, FcCharSet, FcStrSet};
use libc::{c_uint, c_void};

pub type struct__FcLangSet = c_void;

pub type FcLangSet = struct__FcLangSet;

pub type enum__FcLangResult = c_uint;
pub const FcLangEqual: u32 = 0_u32;
pub const FcLangDifferentCountry: u32 = 1_u32;
pub const FcLangDifferentTerritory: u32 = 1_u32;
pub const FcLangDifferentLang: u32 = 2_u32;

pub type FcLangResult = enum__FcLangResult;

extern "C" {

    pub fn FcGetLangs() -> *mut FcStrSet;

    pub fn FcLangGetCharSet(lang: *const FcChar8) -> *mut FcCharSet;

    pub fn FcLangSetCreate() -> *mut FcLangSet;

    pub fn FcLangSetDestroy(ls: *mut FcLangSet);

    pub fn FcLangSetCopy(ls: *const FcLangSet) -> *mut FcLangSet;

    pub fn FcLangSetAdd(ls: *mut FcLangSet, lang: *const FcChar8) -> FcBool;

    pub fn FcLangSetHasLang(ls: *const FcLangSet, lang: *const FcChar8) -> FcLangResult;

    pub fn FcLangSetCompare(lsa: *const FcLangSet, lsb: *const FcLangSet) -> FcLangResult;

    pub fn FcLangSetContains(lsa: *const FcLangSet, lsb: *const FcLangSet) -> FcBool;

    pub fn FcLangSetEqual(lsa: *const FcLangSet, lsb: *const FcLangSet) -> FcBool;

    pub fn FcLangSetHash(ls: *const FcLangSet) -> FcChar32;

    pub fn FcLangSetGetLangs(ls: *const FcLangSet) -> *mut FcStrSet;

}
