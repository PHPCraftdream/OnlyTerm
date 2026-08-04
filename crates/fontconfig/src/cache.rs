//! On-disk font cache types (`FcCache`, and `FcFileCache` which is
//! fontconfig's older global-cache alias) plus the directory/file scanning
//! functions that populate a `FcFontSet` from disk (with or without a
//! cache).
use crate::{FcBlanks, FcBool, FcChar8, FcFontSet, FcPattern, FcStrSet};
use libc::{c_int, c_void};

pub type struct__FcGlobalCache = c_void;

pub type FcFileCache = struct__FcGlobalCache;

pub type struct__FcCache = c_void;

pub type FcCache = struct__FcCache;

extern "C" {

    pub fn FcCacheDir(c: *mut FcCache) -> *const FcChar8;

    pub fn FcCacheCopySet(c: *const FcCache) -> *mut FcFontSet;

    pub fn FcCacheSubdir(c: *const FcCache, i: c_int) -> *const FcChar8;

    pub fn FcCacheNumSubdir(c: *const FcCache) -> c_int;

    pub fn FcCacheNumFont(c: *const FcCache) -> c_int;

    pub fn FcDirCacheValid(cache_file: *const FcChar8) -> FcBool;

    pub fn FcFileIsDir(file: *const FcChar8) -> FcBool;

    pub fn FcFileScan(
        set: *mut FcFontSet,
        dirs: *mut FcStrSet,
        cache: *mut FcFileCache,
        blanks: *mut FcBlanks,
        file: *const FcChar8,
        force: FcBool,
    ) -> FcBool;

    pub fn FcDirScan(
        set: *mut FcFontSet,
        dirs: *mut FcStrSet,
        cache: *mut FcFileCache,
        blanks: *mut FcBlanks,
        dir: *const FcChar8,
        force: FcBool,
    ) -> FcBool;

    pub fn FcDirSave(set: *mut FcFontSet, dirs: *const FcStrSet, dir: *mut FcChar8) -> FcBool;

    pub fn FcDirCacheUnload(cache: *mut FcCache);

    pub fn FcFreeTypeQuery(
        file: *const FcChar8,
        id: c_int,
        blanks: *mut FcBlanks,
        count: *mut c_int,
    ) -> *mut FcPattern;

}
