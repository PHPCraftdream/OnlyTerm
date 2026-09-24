//! A font-database to keep track of fonts that we've located

use crate::locator::{FontDataSource, FontOrigin};
use crate::parser::{load_built_in_fonts, parse_and_collect_font_info, ParsedFont};
use anyhow::Context;
use config::{Config, ConfigHandle, FontAttributes};
use rangeset::RangeSet;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub struct FontDatabase {
    by_full_name: HashMap<String, Vec<ParsedFont>>,
}

/// Value cached in `FONT_DIRS_CACHE`: the config generation, the resolved
/// `font_dirs` list, and the parsed database behind an `Arc` so repeated
/// construction within one generation is a cheap clone instead of a rescan.
type FontDirsCacheValue = Option<(usize, Vec<PathBuf>, Arc<FontDatabase>)>;

lazy_static::lazy_static! {
    // Enumerating and parsing every file under `font_dirs` is expensive
    // (hundreds of ms for a system fonts directory with hundreds of
    // files). `FontConfiguration::new` is called repeatedly during
    // startup (once to size the initial window before the mux window
    // exists, and again when `TermWindow::new_window` builds its own
    // `FontConfiguration`), and each call used to redo the full
    // directory walk from scratch even though the set of `font_dirs`
    // hadn't changed. Cache the resulting `FontDatabase` keyed by the
    // config generation and the font_dirs list, so that repeated
    // construction within one generation is a cheap Arc clone instead of
    // a second filesystem scan+parse. See task #312.
    static ref FONT_DIRS_CACHE: Mutex<FontDirsCacheValue> =
        Mutex::new(None);
    // The built-in font selection is compiled into the binary and never
    // varies with the user's configuration, so it only ever needs to be
    // parsed once per process.
    static ref BUILT_IN_CACHE: Mutex<Option<Arc<FontDatabase>>> = Mutex::new(None);
}

impl Default for FontDatabase {
    fn default() -> Self {
        Self::new()
    }
}

impl FontDatabase {
    pub fn new() -> Self {
        Self {
            by_full_name: HashMap::new(),
        }
    }

    fn load_font_info(&mut self, font_info: Vec<ParsedFont>) {
        for parsed in font_info {
            if let Some(path) = parsed.handle.path_str() {
                self.by_full_name
                    .entry(path.to_string())
                    .or_default()
                    .push(parsed.clone());
            }
            self.by_full_name
                .entry(parsed.names().full_name.clone())
                .or_default()
                .push(parsed);
        }
    }

    /// Build up the database from the fonts found in the configured font dirs
    /// and from the built-in selection of fonts
    pub fn with_font_dirs(config: &Config) -> anyhow::Result<Self> {
        let mut font_info = vec![];
        for path in &config.font_dirs {
            for entry in walkdir::WalkDir::new(path).into_iter() {
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(_) => continue,
                };

                let source = FontDataSource::OnDisk(entry.path().to_path_buf());
                parse_and_collect_font_info(&source, &mut font_info, FontOrigin::FontDirs)
                    .map_err(|err| {
                        log::trace!("failed to read {:?}: {:#}", source, err);
                        err
                    })
                    .ok();
            }
        }

        let mut db = Self::new();
        db.load_font_info(font_info);
        Ok(db)
    }

    /// Same as `with_font_dirs`, but memoizes the result keyed by the config
    /// generation and the `font_dirs` list, so that repeated calls within one
    /// generation reuse the previous scan+parse instead of re-walking the
    /// filesystem and re-parsing every font file. This matters because
    /// `FontConfiguration::new` is invoked more than once during startup
    /// (see task #312) with the same `font_dirs`, and a scan of a system
    /// fonts directory can take the better part of a second.
    ///
    /// The generation is part of the key even though it has no bearing on
    /// what a scan would find, because "install a font, then reload the
    /// config to pick it up" is a workflow people rely on: `font_dirs` is
    /// unchanged in that case, so keying on it alone would keep serving the
    /// stale database and make the newly installed font permanently
    /// invisible.
    pub fn with_font_dirs_cached(config: &ConfigHandle) -> anyhow::Result<Arc<Self>> {
        let generation = config.generation();
        let mut cache = FONT_DIRS_CACHE.lock().unwrap();
        if let Some((cached_generation, dirs, db)) = cache.as_ref() {
            if *cached_generation == generation && dirs == &config.font_dirs {
                return Ok(Arc::clone(db));
            }
        }
        let db = Arc::new(Self::with_font_dirs(config)?);
        cache.replace((generation, config.font_dirs.clone(), Arc::clone(&db)));
        Ok(db)
    }

    pub fn list_available(&self) -> Vec<ParsedFont> {
        let mut fonts = vec![];
        for parsed_list in self.by_full_name.values() {
            for parsed in parsed_list {
                fonts.push(parsed.clone());
            }
        }
        fonts
    }

    pub fn with_built_in() -> anyhow::Result<Self> {
        let mut font_info = vec![];
        load_built_in_fonts(&mut font_info)?;
        let mut db = Self::new();
        db.load_font_info(font_info);
        Ok(db)
    }

    /// Same as `with_built_in`, but memoized for the lifetime of the
    /// process: the built-in font selection is compiled into the binary
    /// and can never change, so there's no need to re-parse it every time
    /// a `FontConfiguration` is constructed.
    pub fn with_built_in_cached() -> anyhow::Result<Arc<Self>> {
        let mut cache = BUILT_IN_CACHE.lock().unwrap();
        if let Some(db) = cache.as_ref() {
            return Ok(Arc::clone(db));
        }
        let db = Arc::new(Self::with_built_in()?);
        cache.replace(Arc::clone(&db));
        Ok(db)
    }

    pub fn resolve_multiple(
        &self,
        fonts: &[FontAttributes],
        handles: &mut Vec<ParsedFont>,
        loaded: &mut HashSet<FontAttributes>,
        pixel_size: u16,
    ) {
        for attr in fonts {
            if let Some(handle) = self.resolve(attr, pixel_size) {
                handles.push(handle.clone().synthesize(attr));
                loaded.insert(attr.clone());
            }
        }
    }

    /// Equivalent to FontLocator::locate_fallback_for_codepoints
    pub fn locate_fallback_for_codepoints(
        &self,
        codepoints: &[char],
    ) -> anyhow::Result<Vec<ParsedFont>> {
        let mut wanted_range = RangeSet::new();
        for &c in codepoints {
            wanted_range.add(c as u32);
        }

        let mut matches = vec![];

        for parsed_list in self.by_full_name.values() {
            for parsed in parsed_list {
                if parsed.names().family == "Last Resort High-Efficiency" {
                    continue;
                }
                let covered = parsed
                    .coverage_intersection(&wanted_range)
                    .with_context(|| format!("coverage_interaction for {:?}", parsed))?;
                if !covered.is_empty() {
                    matches.push(parsed.clone());
                }
            }
        }

        Ok(matches)
    }

    pub fn candidates(&self, font_attr: &FontAttributes) -> Vec<&ParsedFont> {
        let mut fonts = vec![];
        for parsed_list in self.by_full_name.values() {
            for parsed in parsed_list {
                if parsed.matches_name(font_attr) {
                    fonts.push(parsed);
                }
            }
        }
        fonts
    }

    pub fn resolve(&self, font_attr: &FontAttributes, pixel_size: u16) -> Option<&ParsedFont> {
        let mut candidates = vec![];
        for parsed_list in self.by_full_name.values() {
            for parsed in parsed_list {
                if parsed.matches_name(font_attr) {
                    candidates.push(parsed);
                }
            }
        }

        if let Some(idx) = ParsedFont::best_matching_index(font_attr, &candidates, pixel_size) {
            return candidates.get(idx).copied();
        }

        None
    }
}

#[cfg(test)]
#[cfg(windows)]
mod stack_overflow_repro {
    //! Task #422: a real, deterministic `EXCEPTION_STACK_OVERFLOW` was
    //! captured via minidump-stackwalk from two independent crash dumps
    //! in `C:\CrashDumps\OnlyTerm\`, both with an identical crash address
    //! and both after 5-7s of uptime, with the configuration
    //! `font_locator: ConfigDirsOnly`, `font_dirs: [C:/Windows/Fonts]`,
    //! `search_font_dirs_for_fallback: true`. The reported crash frame was
    //! inside `FontDatabase::with_font_dirs` (this file), which is what
    //! originally pointed the investigation at font parsing.
    //!
    //! Investigation summary (see the config-crate regression test added
    //! alongside this one, `crates/config/src/config.rs`'s
    //! `stack_overflow_repro` module, for the actual root cause): running
    //! `with_font_dirs` against the real `C:/Windows/Fonts` directory
    //! (537 files, including COLR/TTC/SVG-in-OT fonts) on a thread with a
    //! deliberately small stack does reproduce an overflow, but bisecting
    //! it down (by shrinking the font-file set, and by isolating each
    //! `ttf_parser`/`swash`-backed accessor individually) showed that no
    //! font file or font-parsing call is responsible: every
    //! `ttf_parser`/`SwashFontInfo` accessor, including on the specific
    //! file the overflow was bisected down to
    //! (`EmojiOneColor-SVGinOT.ttf`), completes fine even on a 64KiB
    //! stack. The actual trigger was `config::configuration()`'s *first*
    //! call in the process lazily initializing the global `CONFIG`
    //! static via `Config::default()` -- which is unrelated to font
    //! parsing and just happened to be reachable from
    //! `ParsedFont::from_face`'s `ignore_svg_fonts` check the moment a
    //! font file was being parsed. `Config::default()` goes through the
    //! `FromDynamic` derive macro (`crates/onlyterm-dynamic/derive`),
    //! which for a ~226-field struct generates one large, non-recursive
    //! `from_dynamic` function body; in an unoptimized/debug build this
    //! measured as needing more stack headroom than a constrained (256KiB)
    //! stack provides, though comfortably under the 1MiB Windows default.
    //! This test keeps a basic sanity check that a real
    //! `with_font_dirs` scan of the system fonts directory succeeds; the
    //! `Config::default()` stack-usage regression coverage lives in the
    //! `config` crate, closer to the actual root cause.
    use super::*;
    use std::path::Path;

    #[test]
    fn with_font_dirs_over_real_windows_fonts_directory_does_not_overflow() {
        let fonts_dir = Path::new(r"C:\Windows\Fonts");
        if !fonts_dir.is_dir() {
            eprintln!("skipping: {:?} does not exist on this machine", fonts_dir);
            return;
        }

        let config = Config {
            font_dirs: vec![fonts_dir.to_path_buf()],
            ..Config::default()
        };

        // A small (but not absurd) stack makes any unbounded/deep
        // recursion in the parsing path fail fast and cheaply instead of
        // silently succeeding just because the test thread happens to
        // have several MB of headroom. 1MiB matches the real Windows
        // default main-thread stack size (see the module doc comment for
        // why this test no longer uses a smaller stack: below 1MiB, the
        // dominant cost is `Config::default()`'s own stack usage, not
        // anything in this crate, and that's covered separately in the
        // `config` crate).
        let handle = std::thread::Builder::new()
            .name("font-dirs-repro".into())
            .stack_size(1024 * 1024)
            .spawn(move || {
                let db = FontDatabase::with_font_dirs(&config)
                    .expect("with_font_dirs should not error for a valid directory");
                db.list_available().len()
            })
            .expect("failed to spawn repro thread");

        let count = handle
            .join()
            .expect("with_font_dirs panicked or the thread crashed (possible stack overflow)");
        eprintln!(
            "with_font_dirs over {:?} parsed {} font entries without crashing",
            fonts_dir, count
        );
    }
}
