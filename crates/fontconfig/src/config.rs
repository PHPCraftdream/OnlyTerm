//! The opaque `FcConfig` type -- fontconfig's top-level configuration
//! object (font directories, cache, substitution rules) -- plus the
//! `FcMatchKind`/`FcSetName` enums used to steer substitution and font-set
//! selection against a config.
use crate::{FcBlanks, FcBool, FcCache, FcChar8, FcFontSet, FcPattern, FcStrList};
use libc::{c_int, c_uint, c_void};

pub type struct__FcConfig = c_void;

pub type FcConfig = struct__FcConfig;

pub type enum__FcMatchKind = c_uint;
pub const FcMatchPattern: u32 = 0_u32;
pub const FcMatchFont: u32 = 1_u32;
pub const FcMatchScan: u32 = 2_u32;

pub type FcMatchKind = enum__FcMatchKind;

pub type enum__FcSetName = c_uint;
pub const FcSetSystem: u32 = 0_u32;
pub const FcSetApplication: u32 = 1_u32;

pub type FcSetName = enum__FcSetName;

extern "C" {

    pub fn FcDirCacheUnlink(dir: *const FcChar8, config: *mut FcConfig) -> FcBool;

    pub fn FcConfigHome() -> *mut FcChar8;

    pub fn FcConfigEnableHome(enable: FcBool) -> FcBool;

    pub fn FcConfigFilename(url: *const FcChar8) -> *mut FcChar8;

    pub fn FcConfigCreate() -> *mut FcConfig;

    pub fn FcConfigReference(config: *mut FcConfig) -> *mut FcConfig;

    pub fn FcConfigDestroy(config: *mut FcConfig);

    pub fn FcConfigSetCurrent(config: *mut FcConfig) -> FcBool;

    pub fn FcConfigGetCurrent() -> *mut FcConfig;

    pub fn FcConfigUptoDate(config: *mut FcConfig) -> FcBool;

    pub fn FcConfigBuildFonts(config: *mut FcConfig) -> FcBool;

    pub fn FcConfigGetFontDirs(config: *mut FcConfig) -> *mut FcStrList;

    pub fn FcConfigGetConfigDirs(config: *mut FcConfig) -> *mut FcStrList;

    pub fn FcConfigGetConfigFiles(config: *mut FcConfig) -> *mut FcStrList;

    pub fn FcConfigGetCache(config: *mut FcConfig) -> *mut FcChar8;

    pub fn FcConfigGetBlanks(config: *mut FcConfig) -> *mut FcBlanks;

    pub fn FcConfigGetCacheDirs(config: *const FcConfig) -> *mut FcStrList;

    pub fn FcConfigGetRescanInterval(config: *mut FcConfig) -> c_int;

    pub fn FcConfigSetRescanInterval(config: *mut FcConfig, rescanInterval: c_int) -> FcBool;

    pub fn FcConfigGetFonts(config: *mut FcConfig, set: FcSetName) -> *mut FcFontSet;

    pub fn FcConfigAppFontAddFile(config: *mut FcConfig, file: *const FcChar8) -> FcBool;

    pub fn FcConfigAppFontAddDir(config: *mut FcConfig, dir: *const FcChar8) -> FcBool;

    pub fn FcConfigAppFontClear(config: *mut FcConfig);

    pub fn FcConfigSubstituteWithPat(
        config: *mut FcConfig,
        p: *mut FcPattern,
        p_pat: *mut FcPattern,
        kind: FcMatchKind,
    ) -> FcBool;

    pub fn FcConfigSubstitute(
        config: *mut FcConfig,
        p: *mut FcPattern,
        kind: FcMatchKind,
    ) -> FcBool;

    pub fn FcDirCacheLoad(
        dir: *const FcChar8,
        config: *mut FcConfig,
        cache_file: *mut *mut FcChar8,
    ) -> *mut FcCache;

    pub fn FcDirCacheRead(
        dir: *const FcChar8,
        force: FcBool,
        config: *mut FcConfig,
    ) -> *mut FcCache;

    //pub fn FcDirCacheLoadFile(cache_file: *mut FcChar8, file_stat: *mut struct_stat) -> *mut FcCache;

    pub fn FcConfigParseAndLoad(
        config: *mut FcConfig,
        file: *const FcChar8,
        complain: FcBool,
    ) -> FcBool;

}
