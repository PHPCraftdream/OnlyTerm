//! The opaque `FcPattern` type -- fontconfig's core "bag of properties"
//! used both to describe a font query and to describe a matched font --
//! together with `FcObjectSet`/`FcObjectType`/`FcConstant` (which describe
//! and constrain pattern properties), `FcValue` accessors, and the
//! `FcName*` pattern (de)serialization functions.
use crate::{
    FcBool, FcChar32, FcChar8, FcCharSet, FcLangSet, FcMatrix, FcResult, FcType, FcValue,
};
use libc::{c_char, c_double, c_int, c_void};

pub type struct__FcPattern = c_void;

pub type FcPattern = struct__FcPattern;

#[repr(C)]
#[allow(missing_copy_implementations)]
pub struct struct__FcObjectType {
    pub object: *mut c_char,
    pub _type: FcType,
}

pub type FcObjectType = struct__FcObjectType;

#[repr(C)]
#[allow(missing_copy_implementations)]
pub struct struct__FcConstant {
    pub name: *mut FcChar8,
    pub object: *mut c_char,
    pub value: c_int,
}

pub type FcConstant = struct__FcConstant;

#[repr(C)]
#[allow(missing_copy_implementations)]
pub struct struct__FcObjectSet {
    pub nobject: c_int,
    pub sobject: c_int,
    pub objects: *mut *mut c_char,
}

pub type FcObjectSet = struct__FcObjectSet;

extern "C" {

    pub fn FcValuePrint(v: FcValue);

    pub fn FcPatternPrint(p: *const FcPattern);

    pub fn FcDefaultSubstitute(pattern: *mut FcPattern);

    pub fn FcObjectSetCreate() -> *mut FcObjectSet;

    pub fn FcObjectSetAdd(os: *mut FcObjectSet, object: *const c_char) -> FcBool;

    pub fn FcObjectSetDestroy(os: *mut FcObjectSet);

    //pub fn FcObjectSetVaBuild(first: *mut c_char, va: *mut __va_list_tag) -> *mut FcObjectSet;

    pub fn FcObjectSetBuild(first: *mut c_char, ...) -> *mut FcObjectSet;

    pub fn FcNameRegisterObjectTypes(types: *const FcObjectType, ntype: c_int) -> FcBool;

    pub fn FcNameUnregisterObjectTypes(types: *const FcObjectType, ntype: c_int) -> FcBool;

    pub fn FcNameGetObjectType(object: *const c_char) -> *const FcObjectType;

    pub fn FcNameRegisterConstants(consts: *const FcConstant, nconsts: c_int) -> FcBool;

    pub fn FcNameUnregisterConstants(consts: *const FcConstant, nconsts: c_int) -> FcBool;

    pub fn FcNameGetConstant(string: *mut FcChar8) -> *const FcConstant;

    pub fn FcNameConstant(string: *mut FcChar8, result: *mut c_int) -> FcBool;

    pub fn FcNameParse(name: *const FcChar8) -> *mut FcPattern;

    pub fn FcNameUnparse(pat: *mut FcPattern) -> *mut FcChar8;

    pub fn FcPatternCreate() -> *mut FcPattern;

    pub fn FcPatternDuplicate(p: *const FcPattern) -> *mut FcPattern;

    pub fn FcPatternReference(p: *mut FcPattern);

    pub fn FcPatternFilter(p: *mut FcPattern, os: *const FcObjectSet) -> *mut FcPattern;

    pub fn FcValueDestroy(v: FcValue);

    pub fn FcValueEqual(va: FcValue, vb: FcValue) -> FcBool;

    pub fn FcValueSave(v: FcValue) -> FcValue;

    pub fn FcPatternDestroy(p: *mut FcPattern);

    pub fn FcPatternEqual(pa: *const FcPattern, pb: *const FcPattern) -> FcBool;

    pub fn FcPatternEqualSubset(
        pa: *const FcPattern,
        pb: *const FcPattern,
        os: *const FcObjectSet,
    ) -> FcBool;

    pub fn FcPatternHash(p: *const FcPattern) -> FcChar32;

    pub fn FcPatternAdd(
        p: *mut FcPattern,
        object: *const c_char,
        value: FcValue,
        append: FcBool,
    ) -> FcBool;

    pub fn FcPatternAddWeak(
        p: *mut FcPattern,
        object: *const c_char,
        value: FcValue,
        append: FcBool,
    ) -> FcBool;

    pub fn FcPatternGet(
        p: *mut FcPattern,
        object: *const c_char,
        id: c_int,
        v: *mut FcValue,
    ) -> FcResult;

    pub fn FcPatternDel(p: *mut FcPattern, object: *const c_char) -> FcBool;

    pub fn FcPatternRemove(p: *mut FcPattern, object: *const c_char, id: c_int) -> FcBool;

    pub fn FcPatternAddInteger(p: *mut FcPattern, object: *const c_char, i: c_int) -> FcBool;

    pub fn FcPatternAddDouble(p: *mut FcPattern, object: *const c_char, d: c_double) -> FcBool;

    pub fn FcPatternAddString(
        p: *mut FcPattern,
        object: *const c_char,
        s: *const FcChar8,
    ) -> FcBool;

    pub fn FcPatternAddMatrix(
        p: *mut FcPattern,
        object: *const c_char,
        s: *const FcMatrix,
    ) -> FcBool;

    pub fn FcPatternAddCharSet(
        p: *mut FcPattern,
        object: *const c_char,
        c: *const FcCharSet,
    ) -> FcBool;

    pub fn FcPatternAddBool(p: *mut FcPattern, object: *const c_char, b: FcBool) -> FcBool;

    pub fn FcPatternAddLangSet(
        p: *mut FcPattern,
        object: *const c_char,
        ls: *const FcLangSet,
    ) -> FcBool;

    pub fn FcPatternGetInteger(
        p: *mut FcPattern,
        object: *const c_char,
        n: c_int,
        i: *mut c_int,
    ) -> FcResult;

    pub fn FcPatternGetDouble(
        p: *mut FcPattern,
        object: *const c_char,
        n: c_int,
        d: *mut c_double,
    ) -> FcResult;

    pub fn FcPatternGetString(
        p: *mut FcPattern,
        object: *const c_char,
        n: c_int,
        s: *mut *mut FcChar8,
    ) -> FcResult;

    pub fn FcPatternGetMatrix(
        p: *mut FcPattern,
        object: *const c_char,
        n: c_int,
        s: *mut *mut FcMatrix,
    ) -> FcResult;

    pub fn FcPatternGetCharSet(
        p: *mut FcPattern,
        object: *const c_char,
        n: c_int,
        c: *mut *mut FcCharSet,
    ) -> FcResult;

    pub fn FcPatternGetBool(
        p: *mut FcPattern,
        object: *const c_char,
        n: c_int,
        b: *mut FcBool,
    ) -> FcResult;

    pub fn FcPatternGetLangSet(
        p: *mut FcPattern,
        object: *const c_char,
        n: c_int,
        ls: *mut *mut FcLangSet,
    ) -> FcResult;

    //pub fn FcPatternVaBuild(p: *mut FcPattern, va: *mut __va_list_tag) -> *mut FcPattern;

    pub fn FcPatternBuild(p: *mut FcPattern, ...) -> *mut FcPattern;

    pub fn FcPatternFormat(pat: *mut FcPattern, format: *const FcChar8) -> *mut FcChar8;

}
