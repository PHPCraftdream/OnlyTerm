//! The opaque `FcAtomic` type, used to make atomic (lock + rename-into-place)
//! updates to on-disk cache files.
use crate::FcBool;
use libc::c_void;

pub type struct__FcAtomic = c_void;

pub type FcAtomic = struct__FcAtomic;

extern "C" {

    pub fn FcAtomicCreate(file: *const crate::FcChar8) -> *mut FcAtomic;

    pub fn FcAtomicLock(atomic: *mut FcAtomic) -> FcBool;

    pub fn FcAtomicNewFile(atomic: *mut FcAtomic) -> *mut crate::FcChar8;

    pub fn FcAtomicOrigFile(atomic: *mut FcAtomic) -> *mut crate::FcChar8;

    pub fn FcAtomicReplaceOrig(atomic: *mut FcAtomic) -> FcBool;

    pub fn FcAtomicDeleteNew(atomic: *mut FcAtomic);

    pub fn FcAtomicUnlock(atomic: *mut FcAtomic);

    pub fn FcAtomicDestroy(atomic: *mut FcAtomic);

}
