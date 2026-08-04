//! Global library lifecycle functions: one-time init/teardown of
//! fontconfig's process-wide state and the compiled-in default config.
use crate::{FcBool, FcConfig};
use libc::c_int;

extern "C" {

    pub fn FcInitLoadConfig() -> *mut FcConfig;

    pub fn FcInitLoadConfigAndFonts() -> *mut FcConfig;

    pub fn FcInit() -> FcBool;

    pub fn FcFini();

    pub fn FcGetVersion() -> c_int;

    pub fn FcInitReinitialize() -> FcBool;

    pub fn FcInitBringUptoDate() -> FcBool;

}
