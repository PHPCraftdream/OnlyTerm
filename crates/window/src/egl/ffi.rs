// gl_generator emits these weird cyclical and redundant type references;
// the types appear to have to be in a module and need to reference super,
// with some of the types specified in both scopes :-/
pub mod generated {
    pub type khronos_utime_nanoseconds_t = super::khronos_utime_nanoseconds_t;
    pub type khronos_uint64_t = super::khronos_uint64_t;
    pub type khronos_ssize_t = super::khronos_ssize_t;
    pub type EGLNativeDisplayType = super::EGLNativeDisplayType;
    pub type EGLNativePixmapType = super::EGLNativePixmapType;
    pub type EGLNativeWindowType = super::EGLNativeWindowType;
    pub type EGLint = super::EGLint;
    pub type NativeDisplayType = super::EGLNativeDisplayType;
    pub type NativePixmapType = super::EGLNativePixmapType;
    pub type NativeWindowType = super::EGLNativeWindowType;

    include!(concat!(env!("OUT_DIR"), "/egl_bindings.rs"));
}

pub use generated::*;

use std::os::raw;

pub type EGLint = i32;
pub type khronos_ssize_t = raw::c_long;
pub type khronos_utime_nanoseconds_t = khronos_uint64_t;
pub type khronos_uint64_t = u64;
pub type EGLNativeDisplayType = *const raw::c_void;
pub type EGLNativePixmapType = *const raw::c_void;

#[cfg(target_os = "windows")]
pub type EGLNativeWindowType = winapi::shared::windef::HWND;
#[cfg(not(target_os = "windows"))]
pub type EGLNativeWindowType = *const raw::c_void;
