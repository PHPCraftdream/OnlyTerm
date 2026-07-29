//! Slightly higher level helper for fontconfig
#![allow(clippy::mutex_atomic)]

use anyhow::{anyhow, ensure, Error};
use config::{FontStretch, FontWeight};
pub use fontconfig::*;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};
use std::{fmt, ptr};

pub const FC_CHARCELL: i32 = 110;
pub const FC_MONO: i32 = 100;
pub const FC_DUAL: i32 = 90;

pub struct FontSet {
    fonts: *mut FcFontSet,
}

impl Drop for FontSet {
    fn drop(&mut self) {
        // SAFETY: `self.fonts` is a valid owned `FcFontSet` obtained from
        // `FcFontList`/`FcFontSort` and destroyed exactly once here on drop.
        unsafe {
            FcFontSetDestroy(self.fonts);
        }
    }
}

impl fmt::Debug for FontSet {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.debug_list().entries(self.iter()).finish()
    }
}

pub struct FontSetIter<'a> {
    set: &'a FontSet,
    position: isize,
}

impl<'a> Iterator for FontSetIter<'a> {
    type Item = Pattern;

    fn next(&mut self) -> Option<Self::Item> {
        // SAFETY: `self.position` is kept `< nfont` by the bounds check below,
        // and `self.set.fonts` is a valid `FcFontSet` borrowed for the iterator's
        // lifetime. `FcPatternReference` adopts the returned pattern so the
        // `Pattern` wrapper owns an independent reference count.
        unsafe {
            if self.position == (*self.set.fonts).nfont as isize {
                None
            } else {
                let pat = *(*self.set.fonts)
                    .fonts
                    .offset(self.position)
                    .as_mut()
                    .unwrap();
                FcPatternReference(pat);
                self.position += 1;
                Some(Pattern { pat })
            }
        }
    }
}

impl FontSet {
    pub fn iter(&self) -> FontSetIter<'_> {
        FontSetIter {
            set: self,
            position: 0,
        }
    }
}

#[repr(C)]
pub enum MatchKind {
    Pattern = FcMatchPattern as isize,
}

pub struct FcResultWrap(FcResult);

impl FcResultWrap {
    pub fn succeeded(&self) -> bool {
        self.0 == FcResultMatch
    }

    pub fn as_err(&self) -> Error {
        // the compiler thinks we defined these globals, when all
        // we did was import them from elsewhere
        match self.0 {
            fontconfig::FcResultMatch => anyhow!("FcResultMatch"),
            fontconfig::FcResultNoMatch => anyhow!("FcResultNoMatch"),
            fontconfig::FcResultTypeMismatch => anyhow!("FcResultTypeMismatch"),
            fontconfig::FcResultNoId => anyhow!("FcResultNoId"),
            fontconfig::FcResultOutOfMemory => anyhow!("FcResultOutOfMemory"),
            _ => anyhow!("FcResult holds invalid value {}", self.0),
        }
    }

    pub fn result<T>(&self, t: T) -> Result<T, Error> {
        #[allow(non_upper_case_globals)]
        match self.0 {
            FcResultMatch => Ok(t),
            _ => Err(self.as_err()),
        }
    }
}

pub struct CharSet {
    cset: *mut FcCharSet,
}

pub struct CharSetRef<'a> {
    cset: *mut FcCharSet,
    phantom: std::marker::PhantomData<&'a FcCharSet>,
}

impl<'a> CharSetRef<'a> {
    pub fn to_range_set(&self) -> rangeset::RangeSet<u32> {
        let mut coverage = rangeset::RangeSet::new();
        let mut next_base_code_point = FcChar32::default();
        const FC_CHARSET_MAP_SIZE: usize = 256 / 32;
        const FC_CHARSET_DONE: FcChar32 = FcChar32::MAX;
        let mut map = [FcChar32::default(); FC_CHARSET_MAP_SIZE];
        // SAFETY: `self.cset` is a valid `FcCharSet`; `map.as_mut_ptr()` points
        // at an `[FcChar32; 8]` buffer (`FC_CHARSET_MAP_SIZE`) sized for one
        // charset page, and `next_base_code_point` is a valid out-pointer.
        let mut base_code_point =
            unsafe { FcCharSetFirstPage(self.cset, map.as_mut_ptr(), &mut next_base_code_point) };
        let mut range_start = FcChar32::MAX;
        let mut code_point = FcChar32::MAX;
        while base_code_point != FC_CHARSET_DONE {
            for (i, mask) in map.iter().enumerate() {
                for j in 0..32 {
                    if mask & (1 << j) != 0 {
                        let new_code_point = base_code_point + (j + i * 32) as u32;
                        if new_code_point > 0 && new_code_point - 1 > code_point {
                            coverage.add_range_unchecked(range_start..code_point + 1);
                            range_start = new_code_point;
                        }
                        if range_start == FcChar32::MAX {
                            range_start = new_code_point;
                        }
                        code_point = new_code_point;
                    }
                }
            }
            // SAFETY: same preconditions as `FcCharSetFirstPage` above: a valid
            // `FcCharSet` and a sufficiently sized `map` buffer.
            base_code_point = unsafe {
                FcCharSetNextPage(self.cset, map.as_mut_ptr(), &mut next_base_code_point)
            };
        }
        if range_start != FcChar32::MAX {
            coverage.add_range_unchecked(range_start..code_point + 1);
        }
        coverage
    }
}

impl Drop for CharSet {
    fn drop(&mut self) {
        // SAFETY: `self.cset` is a valid owned `FcCharSet` from
        // `FcCharSetCreate`, destroyed exactly once here on drop.
        unsafe {
            FcCharSetDestroy(self.cset);
        }
    }
}

impl<'a> From<&'a CharSet> for CharSetRef<'a> {
    fn from(c: &'a CharSet) -> Self {
        Self {
            cset: c.cset,
            phantom: std::marker::PhantomData,
        }
    }
}

impl CharSet {
    pub fn new() -> anyhow::Result<Self> {
        // SAFETY: `FcCharSetCreate` returns a freshly allocated owned charset;
        // the null return is checked before it is wrapped.
        unsafe {
            let cset = FcCharSetCreate();
            ensure!(!cset.is_null(), "FcCharSetCreate failed");
            Ok(Self { cset })
        }
    }

    pub fn add(&mut self, c: char) -> anyhow::Result<()> {
        // SAFETY: `self.cset` is a valid owned `FcCharSet`.
        unsafe {
            ensure!(
                FcCharSetAddChar(self.cset, c as u32) != 0,
                "FcCharSetAddChar failed"
            );
            Ok(())
        }
    }
}

pub struct Pattern {
    pat: *mut FcPattern,
}

impl Pattern {
    pub fn new() -> Result<Pattern, Error> {
        // SAFETY: `FcPatternCreate` returns a freshly allocated owned pattern;
        // the null return is checked before it is wrapped.
        unsafe {
            let p = FcPatternCreate();
            ensure!(!p.is_null(), "FcPatternCreate failed");
            Ok(Pattern { pat: p })
        }
    }

    pub fn get_charset<'a>(&'a self) -> anyhow::Result<CharSetRef<'a>> {
        let mut c = ptr::null_mut();
        // SAFETY: `self.pat` is a valid `FcPattern`; the key is a NUL-terminated
        // C string literal and `&mut c` is a properly typed `*mut FcCharSet`
        // out-pointer.
        unsafe {
            FcPatternGetCharSet(self.pat, b"charset\0".as_ptr() as *const c_char, 0, &mut c);
        }
        ensure!(!c.is_null(), "pattern has no charset");
        Ok(CharSetRef {
            cset: c,
            phantom: std::marker::PhantomData,
        })
    }

    pub fn add_charset(&mut self, charset: &CharSet) -> anyhow::Result<()> {
        // SAFETY: `self.pat` and `charset.cset` are valid owned objects; the key
        // is a NUL-terminated C string literal.
        unsafe {
            ensure!(
                FcPatternAddCharSet(
                    self.pat,
                    b"charset\0".as_ptr() as *const c_char,
                    charset.cset
                ) != 0,
                "failed to add charset property"
            );
            Ok(())
        }
    }

    pub fn charset_intersect_count(&self, charset: &CharSet) -> anyhow::Result<u32> {
        // SAFETY: `self.pat` and `charset.cset` are valid objects; the key is a
        // NUL-terminated C string literal.
        unsafe {
            let mut c = ptr::null_mut();
            FcPatternGetCharSet(self.pat, b"charset\0".as_ptr() as *const c_char, 0, &mut c);
            ensure!(!c.is_null(), "pattern has no charset");
            Ok(FcCharSetIntersectCount(c, charset.cset))
        }
    }

    pub fn add_string(&mut self, key: &str, value: &str) -> Result<(), Error> {
        let key = CString::new(key)?;
        let value = CString::new(value)?;
        // SAFETY: `self.pat` is a valid pattern; `key` and `value` are `CString`s
        // (NUL-terminated and guaranteed free of interior NUL bytes).
        unsafe {
            ensure!(
                FcPatternAddString(self.pat, key.as_ptr(), value.as_ptr() as *const u8) != 0,
                "failed to add string property {:?} -> {:?}",
                key,
                value
            );
            Ok(())
        }
    }

    #[allow(dead_code)]
    pub fn add_double(&mut self, key: &str, value: f64) -> Result<(), Error> {
        let key = CString::new(key)?;
        // SAFETY: `self.pat` is a valid pattern; `key` is a NUL-terminated
        // `CString`.
        unsafe {
            ensure!(
                FcPatternAddDouble(self.pat, key.as_ptr(), value) != 0,
                "failed to set double property {:?} -> {}",
                key,
                value
            );
            Ok(())
        }
    }

    pub fn add_integer(&mut self, key: &str, value: i32) -> Result<(), Error> {
        let key = CString::new(key)?;
        // SAFETY: `self.pat` is a valid pattern; `key` is a NUL-terminated
        // `CString`.
        unsafe {
            ensure!(
                FcPatternAddInteger(self.pat, key.as_ptr(), value) != 0,
                "failed to set integer property {:?} -> {}",
                key,
                value
            );
            Ok(())
        }
    }

    pub fn family(&mut self, family: &str) -> Result<(), Error> {
        self.add_string("family", family)
    }

    pub fn monospace(&mut self) -> Result<(), Error> {
        self.add_integer("spacing", FC_MONO)
    }

    pub fn dual(&mut self) -> Result<(), Error> {
        self.add_integer("spacing", FC_DUAL)
    }

    pub fn delete_property(&mut self, key: &str) -> Result<bool, Error> {
        let key = CString::new(key)?;
        // SAFETY: `self.pat` is a valid pattern; `key` is a NUL-terminated
        // `CString`.
        unsafe { Ok(FcPatternDel(self.pat, key.as_ptr()) != 0) }
    }

    pub fn format(&self, fmt: &str) -> Result<String, Error> {
        let fmt = CString::new(fmt)?;
        // SAFETY: `self.pat` is valid and `fmt` is a NUL-terminated `CString`.
        // The returned `FcChar8*` string is copied into an owned Rust `String`
        // before `FcStrFree` releases it.
        unsafe {
            let s = FcPatternFormat(self.pat, fmt.as_ptr() as *const u8);
            ensure!(!s.is_null(), "failed to format pattern");

            let res = CStr::from_ptr(s as *const c_char)
                .to_string_lossy()
                .into_owned();
            FcStrFree(s);
            Ok(res)
        }
    }

    pub fn render_prepare(&self, pat: &Pattern) -> Result<Pattern, Error> {
        // SAFETY: `self.pat` and `pat.pat` are valid owned patterns; the null
        // config pointer selects the default config. The result is null-checked.
        unsafe {
            let pat = FcFontRenderPrepare(ptr::null_mut(), self.pat, pat.pat);
            ensure!(!pat.is_null(), "failed to prepare pattern");
            Ok(Pattern { pat })
        }
    }

    pub fn config_substitute(&mut self, match_kind: MatchKind) -> Result<(), Error> {
        // SAFETY: `self.pat` is a valid owned pattern. `match_kind` is a
        // `#[repr(C)]` fieldless enum, so casting it to `FcMatchKind` (`c_uint`)
        // is a sound same-representation conversion. The null config pointer
        // selects the default fontconfig configuration.
        unsafe {
            ensure!(
                FcConfigSubstitute(ptr::null_mut(), self.pat, match_kind as FcMatchKind) != 0,
                "FcConfigSubstitute failed"
            );
            Ok(())
        }
    }

    pub fn default_substitute(&mut self) {
        // SAFETY: `self.pat` is a valid owned `FcPattern`.
        unsafe {
            FcDefaultSubstitute(self.pat);
        }
    }

    pub fn list(&self) -> anyhow::Result<FontSet> {
        // SAFETY: `self.pat` is a valid pattern. The object set (`oset`) is
        // created here, used for the query, and freed with `FcObjectSetDestroy`
        // in the same block; all key arguments are NUL-terminated byte literals.
        unsafe {
            // This defines the fields that are retrieved
            let oset = FcObjectSetCreate();
            ensure!(!oset.is_null(), "FcObjectSetCreate failed");
            FcObjectSetAdd(oset, b"family\0".as_ptr() as *const c_char);
            FcObjectSetAdd(oset, b"file\0".as_ptr() as *const c_char);
            FcObjectSetAdd(oset, b"index\0".as_ptr() as *const c_char);
            FcObjectSetAdd(oset, b"spacing\0".as_ptr() as *const c_char);
            FcObjectSetAdd(oset, b"charset\0".as_ptr() as *const c_char);

            let fonts = FcFontList(ptr::null_mut(), self.pat, oset);
            let result = if !fonts.is_null() {
                Ok(FontSet { fonts })
            } else {
                Err(anyhow!("FcFontList failed"))
            };
            FcObjectSetDestroy(oset);
            result
        }
    }

    pub fn get_best_match(&self) -> Result<Self, Error> {
        // SAFETY: `self.pat` is valid; `&mut res.0` is a properly typed
        // `*mut FcResult` out-pointer. The null config selects the default config.
        unsafe {
            let mut res = FcResultWrap(0);
            let best = FcFontMatch(ptr::null_mut(), self.pat, &mut res.0 as *mut _);

            if !res.succeeded() {
                Err(res.as_err())
            } else {
                Ok(Pattern { pat: best })
            }
        }
    }

    pub fn sort(&self, trim: bool) -> Result<FontSet, Error> {
        // SAFETY: `self.pat` is valid; `&mut res.0` is a valid `*mut FcResult`
        // out-pointer and the null `FcCharSet**` argument is permitted by the API.
        unsafe {
            let mut res = FcResultWrap(0);
            let fonts = FcFontSort(
                ptr::null_mut(),
                self.pat,
                if trim { 1 } else { 0 },
                ptr::null_mut(),
                &mut res.0 as *mut _,
            );

            res.result(FontSet { fonts })
        }
    }

    pub fn get_file(&self) -> Result<String, Error> {
        self.get_string("file")
    }

    #[allow(dead_code)]
    pub fn get_double(&self, key: &str) -> Result<f64, Error> {
        // SAFETY: `self.pat` is valid; `key` is a NUL-terminated `CString` and
        // `&mut fval` is a properly typed `*mut f64` out-pointer.
        unsafe {
            let key = CString::new(key)?;
            let mut fval: f64 = 0.0;
            let res = FcResultWrap(FcPatternGetDouble(
                self.pat,
                key.as_ptr(),
                0,
                &mut fval as *mut _,
            ));
            if !res.succeeded() {
                Err(res.as_err())
            } else {
                Ok(fval)
            }
        }
    }

    pub fn get_integer(&self, key: &str) -> Result<c_int, Error> {
        // SAFETY: `self.pat` is valid; `key` is a NUL-terminated `CString` and
        // `&mut ival` is a properly typed `*mut c_int` out-pointer.
        unsafe {
            let key = CString::new(key)?;
            let mut ival: c_int = 0;
            let res = FcResultWrap(FcPatternGetInteger(
                self.pat,
                key.as_ptr(),
                0,
                &mut ival as *mut _,
            ));
            if !res.succeeded() {
                Err(res.as_err())
            } else {
                Ok(ival)
            }
        }
    }

    pub fn get_string(&self, key: &str) -> Result<String, Error> {
        // SAFETY: `self.pat` is valid; `key` is a NUL-terminated `CString`.
        // `&mut ptr` is a valid `*mut *mut FcChar8` out-pointer; the borrowed
        // string is copied out via `to_string_lossy` and is never freed by us.
        unsafe {
            let key = CString::new(key)?;
            let mut ptr: *mut u8 = ptr::null_mut();
            let res = FcResultWrap(FcPatternGetString(
                self.pat,
                key.as_ptr(),
                0,
                &mut ptr as *mut *mut u8,
            ));
            if !res.succeeded() {
                Err(res.as_err())
            } else {
                Ok(CStr::from_ptr(ptr as *const c_char)
                    .to_string_lossy()
                    .into_owned())
            }
        }
    }
}

impl Drop for Pattern {
    fn drop(&mut self) {
        // SAFETY: `self.pat` is a valid owned `FcPattern` from
        // `FcPatternCreate`, destroyed exactly once here on drop.
        unsafe {
            FcPatternDestroy(self.pat);
        }
    }
}

impl fmt::Debug for Pattern {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        // unsafe{FcPatternPrint(self.pat);}
        fmt.write_str(
            &self
                .format("Pattern(%{+family,style,weight,width,slant,spacing,file,index,charset,fontformat{%{=unparse}}})")
                .unwrap(),
        )
    }
}

pub fn to_fc_weight(weight: FontWeight) -> c_int {
    if weight >= FontWeight::EXTRABLACK {
        FC_WEIGHT_EXTRABLACK
    } else if weight >= FontWeight::BLACK {
        FC_WEIGHT_BLACK
    } else if weight >= FontWeight::EXTRABOLD {
        FC_WEIGHT_EXTRABOLD
    } else if weight >= FontWeight::BOLD {
        FC_WEIGHT_BOLD
    } else if weight >= FontWeight::DEMIBOLD {
        FC_WEIGHT_DEMIBOLD
    } else if weight >= FontWeight::MEDIUM {
        FC_WEIGHT_MEDIUM
    } else if weight >= FontWeight::REGULAR {
        FC_WEIGHT_REGULAR
    } else if weight >= FontWeight::BOOK {
        FC_WEIGHT_BOOK
    } else if weight >= FontWeight::LIGHT {
        FC_WEIGHT_LIGHT
    } else if weight >= FontWeight::EXTRALIGHT {
        FC_WEIGHT_EXTRALIGHT
    } else {
        FC_WEIGHT_THIN
    }
}

pub fn to_fc_width(stretch: FontStretch) -> c_int {
    match stretch {
        FontStretch::UltraCondensed => FC_WIDTH_ULTRACONDENSED,
        FontStretch::ExtraCondensed => FC_WIDTH_EXTRACONDENSED,
        FontStretch::Condensed => FC_WIDTH_CONDENSED,
        FontStretch::SemiCondensed => FC_WIDTH_SEMICONDENSED,
        FontStretch::Normal => FC_WIDTH_NORMAL,
        FontStretch::SemiExpanded => FC_WIDTH_SEMIEXPANDED,
        FontStretch::Expanded => FC_WIDTH_EXPANDED,
        FontStretch::ExtraExpanded => FC_WIDTH_EXTRAEXPANDED,
        FontStretch::UltraExpanded => FC_WIDTH_ULTRAEXPANDED,
    }
}
