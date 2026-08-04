use crate::db::FontDatabase;
use crate::locator::FontLocator;
use crate::parser::ParsedFont;
use config::ConfigHandle;
use rangeset::RangeSet;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use wezterm_toast_notification::ToastNotification;

lazy_static::lazy_static! {
    static ref LAST_WARNING: Mutex<Option<(Instant, usize)>> = Mutex::new(None);
}

pub(super) struct FallbackResolveInfo {
    pub(super) no_glyphs: Vec<char>,
    pub(super) pending: Arc<Mutex<Vec<ParsedFont>>>,
    pub(super) completion: Box<dyn FnOnce() + Send>,
    pub(super) font_dirs: Arc<FontDatabase>,
    pub(super) built_in: Arc<FontDatabase>,
    pub(super) locator: Arc<dyn FontLocator + Send + Sync>,
    pub(super) config: ConfigHandle,
}

impl FallbackResolveInfo {
    pub(super) fn process(self) {
        let fallback_str = self.no_glyphs.iter().collect::<String>();
        let mut extra_handles = vec![];

        log::trace!(
            "Looking for {} in fallback fonts",
            fallback_str.escape_unicode()
        );

        match self.locator.locate_fallback_for_codepoints(&self.no_glyphs) {
            Ok(ref mut handles) => extra_handles.append(handles),
            Err(err) => log::error!(
                "Error: {:#} while resolving fallback for {} from font-locator",
                err,
                fallback_str.escape_unicode()
            ),
        }

        if self.config.search_font_dirs_for_fallback {
            match self
                .font_dirs
                .locate_fallback_for_codepoints(&self.no_glyphs)
            {
                Ok(ref mut handles) => extra_handles.append(handles),
                Err(err) => log::error!(
                    "Error: {:#} while resolving fallback for {} from font_dirs",
                    err,
                    fallback_str.escape_unicode()
                ),
            }
        }

        match self
            .built_in
            .locate_fallback_for_codepoints(&self.no_glyphs)
        {
            Ok(ref mut handles) => extra_handles.append(handles),
            Err(err) => log::error!(
                "Error: {:#} while resolving fallback for {} for built-in fonts",
                err,
                fallback_str.escape_unicode()
            ),
        }

        let mut wanted = RangeSet::new();
        for c in self.no_glyphs {
            wanted.add(c as u32);
        }
        log::trace!(
            "Fallback fonts that match {} before sorting are: {:#?}",
            fallback_str.escape_unicode(),
            extra_handles
        );

        if wanted.len() > 1 && self.config.sort_fallback_fonts_by_coverage {
            // Sort by ascending coverage
            extra_handles.sort_by_cached_key(|p| {
                p.coverage_intersection(&wanted)
                    .map(|r| r.len())
                    .unwrap_or(0)
            });
            // Re-arrange to descending coverage
            extra_handles.reverse();
            log::trace!(
                "Fallback fonts that match {} after sorting are: {:#?}",
                fallback_str.escape_unicode(),
                extra_handles
            );
        }

        // iteratively reduce to just the fonts that we need
        extra_handles.retain(|p| match p.coverage_intersection(&wanted) {
            Ok(cov) if cov.is_empty() => false,
            Ok(cov) => {
                // Remove the matches from the set, so that we avoid
                // picking up multiple fonts for the same glyphs
                wanted = wanted.difference(&cov);
                true
            }
            Err(_) => false,
        });

        if !extra_handles.is_empty() {
            let mut pending = self.pending.lock().unwrap();
            pending.append(&mut extra_handles);
            (self.completion)();
        }

        if !wanted.is_empty() {
            // There were some glyphs we couldn't resolve!
            let fallback_str = wanted
                .iter_values()
                .map(|c| std::char::from_u32(c).unwrap_or(' '))
                .collect::<String>();

            let current_gen = self.config.generation();
            let show_warning = self.config.warn_about_missing_glyphs
                && LAST_WARNING
                    .lock()
                    .unwrap()
                    .map(|(instant, generation)| {
                        generation != current_gen
                            || instant.elapsed() > Duration::from_secs(60 * 60)
                    })
                    .unwrap_or(true);

            if show_warning {
                LAST_WARNING
                    .lock()
                    .unwrap()
                    .replace((Instant::now(), self.config.generation()));
                let url = "https://wezterm.org/config/fonts.html";
                log::warn!(
                    "No fonts contain glyphs for these codepoints: {}.\n\
                     Placeholder glyphs are being displayed instead.\n\
                     You may wish to install additional fonts, or adjust your\n\
                     configuration so that it can find them.\n\
                     {} has more information about configuring fonts.\n\
                     Set warn_about_missing_glyphs=false to suppress this message.",
                    fallback_str.escape_unicode(),
                    url,
                );

                ToastNotification {
                    title: "Font problem".to_string(),
                    message: format!(
                        "No fonts contain glyphs for these codepoints: {}.\n\
                            Placeholder glyphs are being displayed instead.\n\
                            You may wish to install additional fonts, or adjust\n\
                            your configuration so that it can find them.\n\
                            Set warn_about_missing_glyphs=false to suppress this\n\
                            message.",
                        fallback_str.escape_unicode()
                    ),
                    url: Some(url.to_string()),
                    timeout: Some(Duration::from_secs(15)),
                }
                .show();
            } else {
                log::debug!(
                    "No fonts contain glyphs for these codepoints: {}",
                    fallback_str.escape_unicode()
                );
            }
        }
    }
}
