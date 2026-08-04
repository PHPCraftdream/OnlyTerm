use crate::locator::{FontDataHandle, FontDataSource, FontOrigin};
use crate::swash_metrics::SwashFontInfo;
use super::ParsedFont;
use std::sync::Arc;

/// Backing bytes for a single scan of `parse_and_collect_font_info`: either
/// a memory-mapped on-disk file (the fast path -- see the comment on
/// [`parse_and_collect_font_info`]) or an owned, fully-read buffer (used
/// for `BuiltIn`/`Memory` sources, which have no file to map, and as a
/// fallback if mapping the file fails).
enum ScanBytes {
    Mapped(Arc<memmap2::Mmap>),
    Owned(Box<[u8]>),
}

impl std::ops::Deref for ScanBytes {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        match self {
            ScanBytes::Mapped(m) => m,
            ScanBytes::Owned(b) => b,
        }
    }
}

impl ScanBytes {
    fn swash_font_info(&self, index: u32) -> anyhow::Result<SwashFontInfo> {
        match self {
            ScanBytes::Mapped(m) => SwashFontInfo::from_mmap(Arc::clone(m), index),
            ScanBytes::Owned(b) => SwashFontInfo::from_data(b.clone(), index),
        }
    }
}

pub(crate) fn parse_and_collect_font_info(
    source: &FontDataSource,
    font_info: &mut Vec<ParsedFont>,
    origin: FontOrigin,
) -> anyhow::Result<()> {
    // Enumerating a font directory (`ls-fonts --list-system`,
    // `FontDatabase::with_font_dirs`) only needs a `ttf_parser::fonts_in_collection`
    // sized peek at the header plus a handful of tables (`name`, `head`,
    // `OS/2`, `CPAL`, and a cmap walk for coverage) out of each file -- but
    // large CJK system font collections routinely run into the tens of
    // megabytes (e.g. `mingliub.ttc` at ~37MB). Reading the whole file with
    // `std::fs::read` (`FontDataSource::load_data`) forces every page to be
    // read off disk up front even though the parser only ever dereferences
    // a small fraction of them; measured across a real `C:/Windows/Fonts`
    // directory (537 files), the large-file `load_data` calls alone
    // accounted for roughly a quarter of the whole directory scan (task
    // #319 investigation notes).
    //
    // Memory-mapping the file instead defers the actual disk I/O to the OS's
    // page fault handler, so only the byte ranges the parser (swash/
    // ttf_parser, both of which just index into a `&[u8]`) actually touches
    // get read from disk. `BuiltIn`/`Memory` sources have no file to map, so
    // they keep going through `load_data` (which is a cheap `Cow::Borrowed`
    // for them anyway, not a disk read).
    //
    // SAFETY (of the `unsafe` mmap creation below): `memmap2::Mmap::map`
    // is `unsafe` because the OS mapping aliases the file's contents
    // directly -- if another process truncates or overwrites the file
    // while it is mapped, dereferencing the map is formally UB (it can
    // also raise SIGBUS on Unix for a truncation past EOF). This is
    // considered acceptable here because: (1) this path is read-only and
    // best-effort already -- `with_font_dirs`'s caller discards individual
    // per-file errors and simply skips a file it can't parse, the same
    // tolerance a torn/renamed-mid-scan file would hit; (2) the scan
    // reads system/user font directories, which are not expected to be
    // rewritten concurrently with a terminal starting up; and (3) on
    // failure to create the mapping at all (e.g. permission issues, or a
    // FAT/network filesystem that disallows mmap) we fall back to a
    // regular full read below rather than erroring out, so the *number* of
    // fonts discovered cannot regress even if mapping silently isn't
    // available. A concurrent modification during the (much smaller)
    // window where pages are actually touched can't be ruled out in
    // principle, but is not meaningfully more likely than the same race
    // already being possible against `std::fs::read` reading a file that's
    // mid-write (which just produces truncated/torn *content*, not UB) --
    // the difference here is one of theoretical soundness, not practical
    // exposure, for a diagnostic/enumeration path that already tolerates
    // per-file failures.
    let data: ScanBytes = match source {
        FontDataSource::OnDisk(path) => match std::fs::File::open(path) {
            Ok(file) => {
                // SAFETY: see the comment above this match.
                match unsafe { memmap2::Mmap::map(&file) } {
                    Ok(mmap) => ScanBytes::Mapped(Arc::new(mmap)),
                    Err(_) => ScanBytes::Owned(source.load_data()?.into_owned().into_boxed_slice()),
                }
            }
            Err(_) => ScanBytes::Owned(source.load_data()?.into_owned().into_boxed_slice()),
        },
        FontDataSource::BuiltIn { .. } | FontDataSource::Memory { .. } => {
            ScanBytes::Owned(source.load_data()?.into_owned().into_boxed_slice())
        }
    };

    // `ttf_parser::fonts_in_collection` mirrors what
    // `ftwrap::Library::query_num_faces` used to get via
    // `FT_Open_Face(.., face_index=-1, ..)` (a documented FreeType
    // convention for "just tell me how many faces are in this file
    // without loading one") - `None` means "not a collection", i.e.
    // exactly 1 face, matching how a non-TTC `num_faces` is always 1. It
    // only reads the first few bytes of `data` (magic + `numFonts`), so
    // this is cheap regardless of whether `data` is mapped or owned.
    let num_faces = ttf_parser::fonts_in_collection(&data).unwrap_or(1);

    fn load_one(
        source: &FontDataSource,
        data: &ScanBytes,
        index: u32,
        font_info: &mut Vec<ParsedFont>,
        origin: &FontOrigin,
    ) -> anyhow::Result<()> {
        let locator = FontDataHandle {
            source: source.clone(),
            index,
            variation: 0,
            origin: origin.clone(),
            coverage: None,
        };

        // Reuses the same backing bytes (an `Arc::clone` of the mmap, or a
        // memcpy of the owned buffer -- either way, no additional I/O)
        // rather than going through `SwashFontInfo::from_locator`, which
        // would re-read the file from disk for every sub-face.
        let font_ref_info = data.swash_font_info(index)?;
        let instances = font_ref_info.instances();
        if !instances.is_empty() {
            // Named-instance (variable font) enumeration: mirrors
            // `ftwrap::Face::variations()`, which built one `ParsedFont`
            // per `fvar` named instance by temporarily selecting it via
            // `FT_Set_Named_Instance` on the shared FT face. `handle.variation`
            // (1-indexed, 0 reserved for "no variation selected") records
            // which instance was used so that later per-glyph consumers
            // (`RustybuzzShaper::variation_coords_for` and friends) can
            // re-select the same coordinates.
            for (idx, _instance) in instances.iter().enumerate() {
                let variation_locator = FontDataHandle {
                    variation: (idx + 1) as u32,
                    ..locator.clone()
                };
                match ParsedFont::from_face(&font_ref_info, variation_locator) {
                    Ok(parsed) => font_info.push(parsed),
                    Err(err) => log::trace!(
                        "error while parsing {:?} index {} instance {}: {}",
                        source,
                        index,
                        idx,
                        err
                    ),
                }
            }
        } else {
            let parsed = ParsedFont::from_face(&font_ref_info, locator)?;
            font_info.push(parsed);
        }
        Ok(())
    }

    for index in 0..num_faces {
        if let Err(err) = load_one(source, &data, index, font_info, &origin) {
            log::trace!("error while parsing {:?} index {}: {}", source, index, err);
        }
    }

    Ok(())
}
