#![cfg(windows)]

use crate::locator::{FontDataSource, FontLocator, FontOrigin};
use crate::parser::{best_matching_font, parse_and_collect_font_info, ParsedFont};
use config::{
    FontAttributes, FontStretch as WTFontStretch, FontStyle as WTFontStyle,
    FontWeight as WTFontWeight,
};
use dwrote::{FontDescriptor, FontStretch, FontStyle, FontWeight};
use std::borrow::Cow;
use std::collections::HashSet;
use std::sync::Arc;
use winapi::shared::windef::{HDC, HFONT};
use winapi::um::dwrite::*;
use winapi::um::winbase::MulDiv;
use winapi::um::wingdi::{
    CreateCompatibleDC, CreateFontIndirectW, DeleteDC, DeleteObject, GetDeviceCaps, GetFontData,
    SelectObject, FIXED_PITCH, GDI_ERROR, LF_FACESIZE, LOGFONTW, LOGPIXELSY, OUT_TT_ONLY_PRECIS,
};

/// A FontLocator implemented using the system font loading
/// functions provided by the font-loader crate.
pub struct GdiFontLocator {}

fn extract_raw_font_data(font: HFONT, name: &str) -> anyhow::Result<FontDataSource> {
    // SAFETY: We create and own the compatible device context returned by
    // `CreateCompatibleDC` and free it with `DeleteDC` before returning. Each
    // `GetFontData` call is made twice: first with a null buffer to query the
    // size, then with a `vec![0u8; size]` buffer of exactly that length, so the
    // write destination is always large enough for the bytes GDI returns. The
    // caller-owned `font` (HFONT) is only selected into the DC, never freed here.
    unsafe {
        let hdc = CreateCompatibleDC(std::ptr::null_mut());
        SelectObject(hdc, font as *mut _);

        // GetFontData can retrieve different parts of the font data.
        // We want to fetch the entire font file, but things are made
        // more complicated because the file may be a TTC file.
        // In that case, the full file data isn't full parsable
        // as a TTF so we need to ask specifically for the TTC file,
        // and then try to reverse engineer which element of the TTC
        // is the one we were looking for.

        // See if we can retrieve the ttc data as a first try
        let ttc_table = 0x66637474; // 'ttcf'

        let ttc_size = GetFontData(hdc, ttc_table, 0, std::ptr::null_mut(), 0);

        let data = if ttc_size > 0 && ttc_size != GDI_ERROR {
            let mut data = vec![0u8; ttc_size as usize];
            GetFontData(hdc, ttc_table, 0, data.as_mut_ptr() as *mut _, ttc_size);

            Ok(data)
        } else {
            // Otherwise: presumably a regular ttf

            let size = GetFontData(hdc, 0, 0, std::ptr::null_mut(), 0);
            match size {
                _ if size > 0 && size != GDI_ERROR => {
                    let mut data = vec![0u8; size as usize];
                    GetFontData(hdc, 0, 0, data.as_mut_ptr() as *mut _, size);
                    Ok(data)
                }
                _ => Err(anyhow::anyhow!("Failed to get font data")),
            }
        };
        DeleteDC(hdc);
        let data = data?;

        Ok(FontDataSource::Memory {
            data: Arc::new(data.into_boxed_slice()),
            name: name.to_string(),
        })
    }
}

fn extract_font_data(
    font: HFONT,
    attr: &FontAttributes,
    pixel_size: u16,
) -> anyhow::Result<ParsedFont> {
    let source = extract_raw_font_data(font, &attr.family)?;

    let mut font_info = vec![];
    parse_and_collect_font_info(&source, &mut font_info, FontOrigin::Gdi)?;
    let matches = ParsedFont::best_match(attr, pixel_size, font_info);

    match matches {
        Some(m) => Ok(m),
        None => anyhow::bail!("No font matching {:?} in {:?}", attr, source),
    }
}

/// Convert a rust string to a windows wide string
fn wide_string(s: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn load_font(font_attr: &FontAttributes, pixel_size: u16) -> anyhow::Result<ParsedFont> {
    let mut log_font = LOGFONTW {
        lfHeight: 0,
        lfWidth: 0,
        lfEscapement: 0,
        lfOrientation: 0,
        lfWeight: font_attr.weight.to_opentype_weight() as _,
        lfItalic: if font_attr.style != WTFontStyle::Normal {
            1
        } else {
            0
        },
        lfUnderline: 0,
        lfStrikeOut: 0,
        lfCharSet: 0,
        lfOutPrecision: OUT_TT_ONLY_PRECIS as u8,
        lfClipPrecision: 0,
        lfQuality: 0,
        lfPitchAndFamily: FIXED_PITCH as u8,
        lfFaceName: [0u16; 32],
    };

    let name = wide_string(&font_attr.family);
    if name.len() > LF_FACESIZE {
        anyhow::bail!(
            "family name {:?} is too large for LOGFONTW",
            font_attr.family
        );
    }
    for (i, &c) in name.iter().enumerate() {
        log_font.lfFaceName[i] = c;
    }

    // SAFETY: `log_font` is a fully-initialized `LOGFONTW`: its face name was
    // filled from `wide_string` (which appends a NUL terminator) and length-
    // checked against `LF_FACESIZE` above. `CreateFontIndirectW` returns an
    // owned HFONT that we release with `DeleteObject` after extracting its data.
    unsafe {
        let font = CreateFontIndirectW(&log_font);
        let result = extract_font_data(font, font_attr, pixel_size);
        DeleteObject(font as *mut _);
        result
    }
}

/// # Safety
/// `hdc` must be a valid device context handle (e.g. obtained from
/// `GetDC`/`CreateDC`); it is passed to `GetDeviceCaps` which reads through it.
pub unsafe fn parse_log_font(log_font: &LOGFONTW, hdc: HDC) -> anyhow::Result<(ParsedFont, f64)> {
    let name = String::from_utf16(&log_font.lfFaceName)?;
    // SAFETY: `log_font` is a caller-provided, fully-initialized `LOGFONTW` that
    // `CreateFontIndirectW` treats as read-only; the resulting owned HFONT is
    // freed via `DeleteObject`. `hdc` is caller-owned and only read by
    // `GetDeviceCaps`; `MulDiv` is pure integer arithmetic with no pointers.
    unsafe {
        let font = CreateFontIndirectW(log_font);
        let source = extract_raw_font_data(font, &name);
        DeleteObject(font as *mut _);
        let source = source?;

        let point_size = MulDiv(-log_font.lfHeight, 72, GetDeviceCaps(hdc, LOGPIXELSY)) as f64;
        let pixel_size = log_font.lfHeight.unsigned_abs() as u16;

        let mut attr = FontAttributes::new(&name);
        attr.weight = config::FontWeight::from_opentype_weight(log_font.lfWeight as u16);
        if log_font.lfItalic == 1 {
            attr.style = WTFontStyle::Italic;
        }

        let mut font_info = vec![];
        parse_and_collect_font_info(&source, &mut font_info, FontOrigin::Gdi)?;
        let matches = ParsedFont::best_match(&attr, pixel_size, font_info);

        match matches {
            Some(m) => Ok((m, point_size)),
            None => anyhow::bail!("No font matching {:?} in {:?}", attr, source),
        }
    }
}

fn attributes_to_descriptor(font_attr: &FontAttributes) -> FontDescriptor {
    FontDescriptor {
        family_name: font_attr.family.to_string(),
        weight: FontWeight::from_u32(font_attr.weight.to_opentype_weight() as u32),
        // `FontCollection::font_from_descriptor` (dwrote) only accepts an
        // *exact* match on weight/stretch/style -- it calls DirectWrite's
        // `GetFirstMatchingFont` (which would happily return the closest
        // available face) but then throws the result away unless it's a
        // precise match on all three axes. Hardcoding `Normal` here used to
        // mean that request always lost for any family whose actual stretch
        // isn't Normal (e.g. Lucida Console, which is SemiCondensed): every
        // single lookup fell through to the legacy GDI path
        // (`load_font`/`CreateFontIndirectW`) instead of the faster/more
        // capable DirectWrite one, even though the user never asked for a
        // non-default stretch. Passing through the actually-requested
        // stretch lets the exact-match path succeed whenever the font's
        // real stretch happens to match what was asked for (task #426
        // investigation).
        stretch: FontStretch::from_u32(font_attr.stretch.to_opentype_stretch() as u32),
        style: match font_attr.style {
            WTFontStyle::Italic => FontStyle::Italic,
            WTFontStyle::Oblique => FontStyle::Oblique,
            WTFontStyle::Normal => FontStyle::Normal,
        },
    }
}

fn handle_from_descriptor(
    attr: &FontAttributes,
    collection: &dwrote::FontCollection,
    descriptor: &FontDescriptor,
    pixel_size: u16,
) -> Option<ParsedFont> {
    // The non-`get_` spellings are the current dwrote API. The deprecated
    // ones they replace hid COM failures; these hand back an `HRESULT`.
    // Nothing here can act on the code itself, but logging it and treating
    // the font as absent beats a silent miss.
    let font = match collection.font_from_descriptor(descriptor) {
        Ok(font) => font?,
        Err(hr) => {
            log::warn!("font_from_descriptor({:?}) failed: {:?}", descriptor, hr);
            return None;
        }
    };
    let face = font.create_font_face();
    let files = match face.files() {
        Ok(files) => files,
        Err(hr) => {
            log::warn!("FontFace::files() failed: {:?}", hr);
            return None;
        }
    };
    for file in files {
        if let Ok(path) = file.font_file_path() {
            let family_name = font.family_name();

            log::debug!("{} -> {}", family_name, path.display());
            let source = FontDataSource::OnDisk(path);
            match best_matching_font(&source, attr, FontOrigin::DirectWrite, pixel_size) {
                Ok(Some(parsed)) => {
                    return Some(parsed);
                }
                Ok(None) => {}
                Err(err) => log::warn!("While parsing: {:?}: {:#}", source, err),
            }
        }
    }
    None
}

impl FontLocator for GdiFontLocator {
    fn load_fonts(
        &self,
        fonts_selection: &[FontAttributes],
        loaded: &mut HashSet<FontAttributes>,
        pixel_size: u16,
    ) -> anyhow::Result<Vec<ParsedFont>> {
        let mut fonts = Vec::new();
        let collection = dwrote::FontCollection::system();

        for font_attr in fonts_selection {
            let descriptor = attributes_to_descriptor(font_attr);

            fn try_handle(
                font_attr: &FontAttributes,
                parsed: ParsedFont,
                fonts: &mut Vec<ParsedFont>,
                loaded: &mut HashSet<FontAttributes>,
            ) -> bool {
                if parsed.matches_name(font_attr) {
                    fonts.push(parsed);
                    loaded.insert(font_attr.clone());
                    true
                } else {
                    log::debug!("parsed {:?} doesn't match {:?}", parsed, font_attr);
                    false
                }
            }

            match handle_from_descriptor(font_attr, &collection, &descriptor, pixel_size) {
                Some(handle) => {
                    log::debug!("Got {:?} from dwrote", handle);
                    if try_handle(font_attr, handle, &mut fonts, loaded) {
                        continue;
                    }
                }
                None => {
                    log::debug!("dwrote couldn't resolve {:?}", font_attr);
                }
            }

            match load_font(font_attr, pixel_size) {
                Ok(handle) => {
                    log::debug!("Got {:?} from gdi", handle);
                    try_handle(font_attr, handle, &mut fonts, loaded);
                }
                Err(err) => {
                    log::debug!("gdi couldn't resolve {:?} to a path: {:#}", font_attr, err);
                }
            }
        }

        Ok(fonts)
    }

    fn locate_fallback_for_codepoints(
        &self,
        codepoints: &[char],
    ) -> anyhow::Result<Vec<ParsedFont>> {
        let text: Vec<u16> = codepoints
            .iter()
            .map(|&c| c as u16)
            .chain(std::iter::once(0))
            .collect();

        let collection = dwrote::FontCollection::system();
        struct Source {
            locale: String,
            len: u32,
        }
        impl dwrote::TextAnalysisSourceMethods for Source {
            fn get_locale_name<'a>(&'a self, _: u32) -> (Cow<'a, str>, u32) {
                (Cow::Borrowed(&self.locale), self.len)
            }
            fn get_paragraph_reading_direction(&self) -> u32 {
                DWRITE_READING_DIRECTION_LEFT_TO_RIGHT
            }
        }

        let source = dwrote::TextAnalysisSource::from_text(
            Box::new(Source {
                locale: "".to_string(),
                len: codepoints.len() as u32,
            }),
            Cow::Borrowed(&text),
        );

        let mut handles = vec![];
        let mut resolved = HashSet::new();

        if let Some(fallback) = dwrote::FontFallback::get_system_fallback() {
            let mut start = 0usize;
            let mut len = codepoints.len();
            loop {
                let result = fallback.map_characters(
                    &source,
                    start as u32,
                    len as u32,
                    &collection,
                    None,
                    FontWeight::Regular,
                    FontStyle::Normal,
                    FontStretch::Normal,
                );

                if let Some(font) = result.mapped_font {
                    log::trace!(
                        "DirectWrite Suggested fallback: {} {}",
                        font.family_name(),
                        font.face_name()
                    );

                    let attr = FontAttributes {
                        weight: WTFontWeight::from_opentype_weight(font.weight().to_u32() as _),
                        stretch: WTFontStretch::from_opentype_stretch(font.stretch().to_u32() as _),
                        style: WTFontStyle::Normal,
                        family: font.family_name(),
                        is_fallback: true,
                        is_synthetic: true,
                        harfbuzz_features: None,
                        freetype_load_target: None,
                        freetype_render_target: None,
                        freetype_load_flags: None,
                        scale: None,
                        assume_emoji_presentation: None,
                    };

                    if !resolved.contains(&attr) {
                        resolved.insert(attr.clone());

                        let descriptor = attributes_to_descriptor(&attr);
                        if let Some(handle) = handle_from_descriptor(
                            &attr,
                            &collection,
                            &descriptor,
                            16, /* pixel_size: irrelevant really as we kinda want a scalable font for fallback */
                        ) {
                            handles.push(handle);
                        }
                    }
                }
                if result.mapped_length > 0 {
                    start += result.mapped_length
                } else {
                    break;
                }
                if start == codepoints.len() {
                    break;
                }
                len = codepoints.len() - start;
            }
        } else {
            log::error!("Unable to get system fallback from dwrote");
        }

        Ok(handles)
    }

    fn enumerate_all_fonts(&self) -> anyhow::Result<Vec<ParsedFont>> {
        let collection = dwrote::FontCollection::system();
        let mut fonts = vec![];
        let mut files = HashSet::new();
        for family in collection.families_iter() {
            let count = family.get_font_count();
            for idx in 0..count {
                // Current dwrote spellings; see `handle_from_descriptor`.
                // A family that fails to yield one of its fonts shouldn't
                // abort the whole enumeration -- skip it and carry on, so a
                // single broken installed font can't cost the user every
                // other one.
                let font = match family.font(idx) {
                    Ok(font) => font,
                    Err(hr) => {
                        log::warn!("FontFamily::font({}) failed: {:?}", idx, hr);
                        continue;
                    }
                };
                let face = font.create_font_face();
                let face_files = match face.files() {
                    Ok(files) => files,
                    Err(hr) => {
                        log::warn!("FontFace::files() failed: {:?}", hr);
                        continue;
                    }
                };
                for file in face_files {
                    if let Ok(path) = file.font_file_path() {
                        if files.contains(&path) {
                            continue;
                        }
                        files.insert(path.clone());

                        let source = FontDataSource::OnDisk(path);
                        if let Err(err) = parse_and_collect_font_info(
                            &source,
                            &mut fonts,
                            FontOrigin::DirectWrite,
                        ) {
                            log::warn!("While parsing: {:?}: {:#}", source, err);
                        }
                    }
                }
            }
        }
        fonts.sort();

        Ok(fonts)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use config::FontStretch as WTFontStretch;

    /// Task #426 investigation: `attributes_to_descriptor` used to hardcode
    /// `stretch: FontStretch::Normal` no matter what the caller actually
    /// asked for. `dwrote::FontCollection::font_from_descriptor` requires an
    /// *exact* match on weight/stretch/style (see its doc comment above),
    /// so that hardcoding silently defeated DirectWrite resolution for any
    /// family whose real stretch isn't Normal -- e.g. Lucida Console, which
    /// is SemiCondensed -- even when the user's configuration never asked
    /// for anything but the default stretch. Every such lookup fell through
    /// to the legacy GDI path instead of ever getting a chance to succeed
    /// via DirectWrite. This pins down that the descriptor now carries
    /// through the caller's actual stretch instead of a fixed default.
    #[test]
    fn descriptor_carries_requested_stretch() {
        for &stretch in &[
            WTFontStretch::UltraCondensed,
            WTFontStretch::ExtraCondensed,
            WTFontStretch::Condensed,
            WTFontStretch::SemiCondensed,
            WTFontStretch::Normal,
            WTFontStretch::SemiExpanded,
            WTFontStretch::Expanded,
            WTFontStretch::ExtraExpanded,
            WTFontStretch::UltraExpanded,
        ] {
            let attr = FontAttributes {
                stretch,
                ..FontAttributes::new("Lucida Console")
            };
            let descriptor = attributes_to_descriptor(&attr);
            assert_eq!(
                descriptor.stretch.to_u32(),
                stretch.to_opentype_stretch() as u32,
                "descriptor.stretch should mirror the requested attr.stretch \
                 ({:?}), not be hardcoded to Normal",
                stretch
            );
        }
    }

    /// Task #426 live sweep: call the *actual* `GdiFontLocator::load_fonts`
    /// entry point (not just `attributes_to_descriptor` in isolation) for
    /// `Lucida Console` at every pixel_size reachable via repeated
    /// Ctrl+=/Ctrl+- zooming (~10% steps) combined with common Windows
    /// display/text-scaling ratios (100%-250%) applied to a 14pt starting
    /// size, to find any pixel_size at which resolution actually fails.
    /// Skips on machines where Lucida Console isn't installed (e.g. CI
    /// images that don't ship it) rather than failing spuriously.
    #[test]
    fn load_fonts_sweep_pixel_sizes_for_lucida_console() {
        if !std::path::Path::new(r"C:\Windows\Fonts\lucon.ttf").is_file() {
            eprintln!("skipping: Lucida Console is not installed on this machine");
            return;
        }

        let locator = GdiFontLocator {};
        let attr = FontAttributes::new("Lucida Console");
        let mut failures = vec![];
        for pixel_size in 5u16..=80 {
            let mut loaded = HashSet::new();
            match locator.load_fonts(std::slice::from_ref(&attr), &mut loaded, pixel_size) {
                Ok(fonts) if !fonts.is_empty() => {}
                Ok(_) => failures.push((pixel_size, "empty result".to_string())),
                Err(e) => failures.push((pixel_size, format!("{:#}", e))),
            }
        }
        assert!(
            failures.is_empty(),
            "load_fonts failed to resolve Lucida Console at these pixel_sizes: {:?}",
            failures
        );
    }
}
