//! Primitive scalar types, `FcType`/`FcResult`/`FcEndian` enums, and the
//! `FcValue`/`FcMatrix` value-carrying structs, together with the weight,
//! slant and width constants that populate `FcValue`s of type
//! `FcTypeInteger`. Grouped together because they form the low-level
//! vocabulary that every other module (patterns, font sets, ...) builds on.
use libc::*;

pub type FcChar8 = c_uchar;
pub type FcChar16 = c_ushort;
pub type FcChar32 = c_uint;
pub type FcBool = c_int;

pub type enum__FcType = c_uint;
pub const FcTypeVoid: u32 = 0_u32;
pub const FcTypeInteger: u32 = 1_u32;
pub const FcTypeDouble: u32 = 2_u32;
pub const FcTypeString: u32 = 3_u32;
pub const FcTypeBool: u32 = 4_u32;
pub const FcTypeMatrix: u32 = 5_u32;
pub const FcTypeCharSet: u32 = 6_u32;
pub const FcTypeFTFace: u32 = 7_u32;
pub const FcTypeLangSet: u32 = 8_u32;

pub type FcType = enum__FcType;

pub const FC_WEIGHT_THIN: c_int = 0;
pub const FC_WEIGHT_EXTRALIGHT: c_int = 40;
pub const FC_WEIGHT_ULTRALIGHT: c_int = FC_WEIGHT_EXTRALIGHT;
pub const FC_WEIGHT_LIGHT: c_int = 50;
pub const FC_WEIGHT_BOOK: c_int = 75;
pub const FC_WEIGHT_REGULAR: c_int = 80;
pub const FC_WEIGHT_NORMAL: c_int = FC_WEIGHT_REGULAR;
pub const FC_WEIGHT_MEDIUM: c_int = 100;
pub const FC_WEIGHT_DEMIBOLD: c_int = 180;
pub const FC_WEIGHT_SEMIBOLD: c_int = FC_WEIGHT_DEMIBOLD;
pub const FC_WEIGHT_BOLD: c_int = 200;
pub const FC_WEIGHT_EXTRABOLD: c_int = 205;
pub const FC_WEIGHT_ULTRABOLD: c_int = FC_WEIGHT_EXTRABOLD;
pub const FC_WEIGHT_BLACK: c_int = 210;
pub const FC_WEIGHT_HEAVY: c_int = FC_WEIGHT_BLACK;
pub const FC_WEIGHT_EXTRABLACK: c_int = 215;
pub const FC_WEIGHT_ULTRABLACK: c_int = FC_WEIGHT_EXTRABLACK;

pub const FC_SLANT_ROMAN: c_int = 0;
pub const FC_SLANT_ITALIC: c_int = 100;
pub const FC_SLANT_OBLIQUE: c_int = 110;

pub const FC_WIDTH_ULTRACONDENSED: c_int = 50;
pub const FC_WIDTH_EXTRACONDENSED: c_int = 63;
pub const FC_WIDTH_CONDENSED: c_int = 75;
pub const FC_WIDTH_SEMICONDENSED: c_int = 87;
pub const FC_WIDTH_NORMAL: c_int = 100;
pub const FC_WIDTH_SEMIEXPANDED: c_int = 113;
pub const FC_WIDTH_EXPANDED: c_int = 125;
pub const FC_WIDTH_EXTRAEXPANDED: c_int = 150;
pub const FC_WIDTH_ULTRAEXPANDED: c_int = 200;

#[repr(C)]
#[derive(Copy, Clone)]
pub struct struct__FcMatrix {
    pub xx: c_double,
    pub xy: c_double,
    pub yx: c_double,
    pub yy: c_double,
}

pub type FcMatrix = struct__FcMatrix;

pub type enum__FcResult = c_uint;
pub const FcResultMatch: u32 = 0_u32;
pub const FcResultNoMatch: u32 = 1_u32;
pub const FcResultTypeMismatch: u32 = 2_u32;
pub const FcResultNoId: u32 = 3_u32;
pub const FcResultOutOfMemory: u32 = 4_u32;

pub type FcResult = enum__FcResult;

#[repr(C)]
#[allow(missing_copy_implementations)]
pub struct struct__FcValue {
    pub _type: FcType,
    pub u: union_unnamed1,
}

pub type FcValue = struct__FcValue;

pub type union_unnamed1 = c_void;

pub type FcEndian = c_uint;
pub const FcEndianBig: u32 = 0_u32;
pub const FcEndianLittle: u32 = 1_u32;
