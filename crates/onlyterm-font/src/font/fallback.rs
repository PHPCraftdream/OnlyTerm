use crate::db::FontDatabase;
use crate::locator::FontLocator;
use crate::parser::ParsedFont;
use config::ConfigHandle;
use onlyterm_toast_notification::ToastNotification;
use rangeset::RangeSet;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

lazy_static::lazy_static! {
    static ref LAST_WARNING: Mutex<Option<(Instant, usize)>> = Mutex::new(None);
}

pub(super) struct FallbackResolveInfo {
    pub(super) no_glyphs: Vec<char>,
    pub(super) pending: Arc<Mutex<Vec<ParsedFont>>>,
    pub(super) completion: Box<dyn FnOnce(Vec<char>) + Send>,
    pub(super) font_dirs: Arc<FontDatabase>,
    pub(super) built_in: Arc<FontDatabase>,
    pub(super) locator: Arc<dyn FontLocator + Send + Sync>,
    pub(super) config: ConfigHandle,
}

impl FallbackResolveInfo {
    pub(super) fn process(self) {
        let fallback_str = self.no_glyphs.iter().collect::<String>();
        let requested_glyphs = self.no_glyphs.clone();
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

        // Sort candidates deterministically before the greedy reduction
        // below picks winners. Unlike the old code, this always runs (not
        // just when `sort_fallback_fonts_by_coverage` is on) because two of
        // its three keys are correctness fixes, not a quality preference:
        //
        // 1. (opt-in via `sort_fallback_fonts_by_coverage`) descending
        //    coverage of `wanted` -- a font that covers more of what we
        //    still need is more useful, so prefer it first.
        // 2. (always) prefer a non-emoji-presentation font on a tie.
        //    `compute_coverage` (cmap enumeration) and the shaper's real
        //    per-codepoint glyph lookup can disagree for color/emoji fonts:
        //    a codepoint's cmap entry can point at a glyph that is only
        //    meaningful as part of a ZWJ/ligature sequence (resolved via
        //    GSUB during real shaping), not as a standalone character, so
        //    `compute_coverage` sees a real (non-zero) glyph id while the
        //    shaper renders `.notdef` for it in isolation. Observed
        //    concretely with the bundled NotoColorEmoji.ttf claiming
        //    coverage of U+2702 (SCISSORS, a Dingbat, not an emoji) that it
        //    cannot actually render standalone, competing with the
        //    correctly-covering Noto Sans Symbols 2. Deprioritizing
        //    emoji-presentation fonts on ties avoids the class of bug, not
        //    just this one codepoint.
        // 3. (always) the font's own name, as a final tie-break. The three
        //    `locate_fallback_for_codepoints` sources above walk a
        //    `HashMap`, whose iteration order is randomized per-process, so
        //    without this the previous keys being equal would still leave
        //    the winner picked non-deterministically from one run to the
        //    next (this is what made the underlying bug intermittent
        //    rather than consistently wrong, and is why this whole sort
        //    cannot be left conditional on the opt-in coverage preference).
        extra_handles.sort_by(|a, b| {
            let mut ordering = std::cmp::Ordering::Equal;
            if self.config.sort_fallback_fonts_by_coverage {
                let a_cov = a
                    .coverage_intersection(&wanted)
                    .map(|r| r.len())
                    .unwrap_or(0);
                let b_cov = b
                    .coverage_intersection(&wanted)
                    .map(|r| r.len())
                    .unwrap_or(0);
                ordering = b_cov.cmp(&a_cov);
            }
            ordering
                .then_with(|| {
                    a.assume_emoji_presentation
                        .cmp(&b.assume_emoji_presentation)
                })
                .then_with(|| a.names().full_name.cmp(&b.names().full_name))
        });
        log::trace!(
            "Fallback fonts that match {} after sorting are: {:#?}",
            fallback_str.escape_unicode(),
            extra_handles
        );

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
            (self.completion)(requested_glyphs);
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

                // If font_dirs is configured but search_font_dirs_for_fallback
                // is off, those fonts are scanned into a database that is
                // then never consulted for fallback resolution, which looks
                // exactly like "onlyterm can't find my fonts" from the
                // outside. Call that out explicitly so the fix (flip the
                // flag) is obvious instead of requiring a fresh
                // investigation each time.
                let font_dirs_hint = if !self.config.font_dirs.is_empty()
                    && !self.config.search_font_dirs_for_fallback
                {
                    format!(
                        "\nfont_dirs is configured ({} directories) but \
                         search_font_dirs_for_fallback is false, so those fonts\n\
                         are not being searched as fallback candidates. Set\n\
                         search_font_dirs_for_fallback = true in your config to use them.\n",
                        self.config.font_dirs.len()
                    )
                } else {
                    String::new()
                };

                log::warn!(
                    "No fonts contain glyphs for these codepoints: {}.\n\
                     Placeholder glyphs are being displayed instead.\n\
                     You may wish to install additional fonts, or adjust your\n\
                     configuration so that it can find them.{}\n\
                     {} has more information about configuring fonts.\n\
                     Set warn_about_missing_glyphs=false to suppress this message.",
                    fallback_str.escape_unicode(),
                    font_dirs_hint,
                    url,
                );

                ToastNotification {
                    title: "Font problem".to_string(),
                    message: format!(
                        "No fonts contain glyphs for these codepoints: {}.\n\
                            Placeholder glyphs are being displayed instead.\n\
                            You may wish to install additional fonts, or adjust\n\
                            your configuration so that it can find them.{}\n\
                            Set warn_about_missing_glyphs=false to suppress this\n\
                            message.",
                        fallback_str.escape_unicode(),
                        font_dirs_hint,
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
