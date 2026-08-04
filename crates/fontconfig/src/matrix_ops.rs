//! Operations on `FcMatrix` (see `value_types` for the struct itself).
use crate::{FcBool, FcMatrix};
use libc::*;

extern "C" {

    pub fn FcMatrixCopy(mat: *const FcMatrix) -> *mut FcMatrix;

    pub fn FcMatrixEqual(mat1: *const FcMatrix, mat2: *const FcMatrix) -> FcBool;

    pub fn FcMatrixMultiply(result: *mut FcMatrix, a: *const FcMatrix, b: *const FcMatrix);

    pub fn FcMatrixRotate(m: *mut FcMatrix, c: c_double, s: c_double);

    pub fn FcMatrixScale(m: *mut FcMatrix, sx: c_double, sy: c_double);

    pub fn FcMatrixShear(m: *mut FcMatrix, sh: c_double, sv: c_double);

}
