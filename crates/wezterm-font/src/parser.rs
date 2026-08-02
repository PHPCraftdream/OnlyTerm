use crate::locator::{FontDataHandle, FontDataSource, FontOrigin};
use crate::shaper::GlyphInfo;
use crate::swash_metrics::SwashFontInfo;
use config::{FontAttributes, FontStyle, FreeTypeLoadFlags, FreeTypeLoadTarget};
pub use config::{FontStretch, FontWeight};
use rangeset::RangeSet;
use std::cmp::Ordering;
use std::sync::{Arc, Mutex};

#[derive(Debug)]
pub enum MaybeShaped {
    Resolved(GlyphInfo),
    Unresolved { raw: String, slice_start: usize },
}

#[derive(Debug, Clone)]
pub struct FontPaletteInfo {
    pub name: String,
    pub palette_index: usize,
    pub usable_with_light_bg: bool,
    pub usable_with_dark_bg: bool,
}

/// Represents a parsed font
pub struct ParsedFont {
    names: Names,
    weight: FontWeight,
    stretch: FontStretch,
    style: FontStyle,
    cap_height: Option<f64>,
    pub handle: FontDataHandle,
    coverage: Mutex<RangeSet<u32>>,
    pub synthesize_italic: bool,
    pub synthesize_bold: bool,
    pub synthesize_dim: bool,
    pub assume_emoji_presentation: bool,
    pub pixel_sizes: Vec<u16>,
    pub is_built_in_fallback: bool,
    pub palettes: Vec<FontPaletteInfo>,

    pub harfbuzz_features: Option<Vec<String>>,
    pub freetype_load_target: Option<FreeTypeLoadTarget>,
    pub freetype_render_target: Option<FreeTypeLoadTarget>,
    pub freetype_load_flags: Option<FreeTypeLoadFlags>,
    pub scale: Option<f64>,
}

impl std::fmt::Debug for ParsedFont {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
        fmt.debug_struct("ParsedFont")
            .field("names", &self.names)
            .field("weight", &self.weight)
            .field("stretch", &self.stretch)
            .field("style", &self.style)
            .field("handle", &self.handle)
            .field("cap_height", &self.cap_height)
            .field("synthesize_italic", &self.synthesize_italic)
            .field("synthesize_bold", &self.synthesize_bold)
            .field("synthesize_dim", &self.synthesize_dim)
            .field("assume_emoji_presentation", &self.assume_emoji_presentation)
            .field("pixel_sizes", &self.pixel_sizes)
            .field("harfbuzz_features", &self.harfbuzz_features)
            .field("freetype_load_target", &self.freetype_load_target)
            .field("freetype_render_target", &self.freetype_render_target)
            .field("freetype_load_flags", &self.freetype_load_flags)
            .field("scale", &self.scale)
            .finish()
    }
}

impl Clone for ParsedFont {
    fn clone(&self) -> Self {
        Self {
            names: self.names.clone(),
            weight: self.weight,
            stretch: self.stretch,
            style: self.style,
            synthesize_italic: self.synthesize_italic,
            synthesize_bold: self.synthesize_bold,
            synthesize_dim: self.synthesize_dim,
            assume_emoji_presentation: self.assume_emoji_presentation,
            handle: self.handle.clone(),
            cap_height: self.cap_height,
            coverage: Mutex::new(self.coverage.lock().unwrap().clone()),
            pixel_sizes: self.pixel_sizes.clone(),
            harfbuzz_features: self.harfbuzz_features.clone(),
            freetype_load_target: self.freetype_load_target,
            freetype_render_target: self.freetype_render_target,
            freetype_load_flags: self.freetype_load_flags,
            is_built_in_fallback: self.is_built_in_fallback,
            scale: self.scale,
            palettes: self.palettes.clone(),
        }
    }
}

impl Eq for ParsedFont {}

impl PartialEq for ParsedFont {
    fn eq(&self, rhs: &Self) -> bool {
        self.stretch == rhs.stretch
            && self.weight == rhs.weight
            && self.style == rhs.style
            && self.names == rhs.names
    }
}

impl Ord for ParsedFont {
    fn cmp(&self, rhs: &Self) -> Ordering {
        match self.names.family.cmp(&rhs.names.family) {
            o @ Ordering::Less | o @ Ordering::Greater => o,
            Ordering::Equal => match self.stretch.cmp(&rhs.stretch) {
                o @ Ordering::Less | o @ Ordering::Greater => o,
                Ordering::Equal => match self.weight.cmp(&rhs.weight) {
                    o @ Ordering::Less | o @ Ordering::Greater => o,
                    Ordering::Equal => match self.style.cmp(&rhs.style) {
                        o @ Ordering::Less | o @ Ordering::Greater => o,
                        Ordering::Equal => self.handle.cmp(&rhs.handle),
                    },
                },
            },
        }
    }
}

impl PartialOrd for ParsedFont {
    fn partial_cmp(&self, rhs: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(rhs))
    }
}

#[derive(Debug, PartialEq, Eq, Clone, PartialOrd, Ord)]
pub struct Names {
    pub full_name: String,
    pub family: String,
    pub sub_family: Option<String>,
    pub postscript_name: Option<String>,
    pub aliases: Vec<String>,
}

/// Returns the sorted, deduplicated set of distinct family-name strings
/// found across a font's `TypographicFamily`/`Family` name records (i.e.
/// every language/platform variant of those two ids), used to populate
/// `Names::aliases` below. `SwashFontInfo::sfnt_names` already restricts
/// itself to the small set of name ids wezterm cares about (family/
/// subfamily/postscript, both typographic and legacy), so this just
/// filters further down to the family-ish ids and dedups the resulting
/// strings.
fn family_aliases_from_sfnt_names(font_info: &SwashFontInfo) -> Vec<String> {
    let mut result: Vec<String> = font_info
        .sfnt_names()
        .into_iter()
        .filter(|rec| {
            matches!(
                rec.id,
                swash::StringId::TypographicFamily | swash::StringId::Family
            )
        })
        .map(|rec| rec.name)
        .collect();
    result.sort();
    result.dedup();
    result
}

impl Names {
    /// Builds a `Names` from a parsed font face's name table.
    ///
    /// This used to go through FreeType's own name-table walk
    /// (`ftwrap::Face::get_sfnt_names`/`family_name`/`style_name`/
    /// `postscript_name`) because, per
    /// <https://github.com/wezterm/wezterm/issues/1761#issuecomment-1079150560>,
    /// FreeType's own built-in name accessors have a limited set of
    /// encodings and can return `?????` for some fonts. `swash`'s
    /// `localized_strings()`/`find_by_id` (backing
    /// [`SwashFontInfo::family_name`]/[`style_name`]/[`postscript_name`])
    /// independently walks the same `name` table with its own (Unicode +
    /// Mac Roman) decoder and prefers Unicode-encoded records, achieving
    /// the same goal by a different route -- see the module doc comment
    /// on `swash_metrics.rs` for the parity testing that established this
    /// is a safe replacement for the common (non-broken-encoding) case.
    pub fn from_face(font_info: &SwashFontInfo) -> Names {
        let family = font_info.family_name();
        let sub_family = font_info.style_name();
        let postscript_name = font_info.postscript_name();

        let full_name = if sub_family.is_empty() {
            family.to_string()
        } else {
            format!("{} {}", family, sub_family)
        };

        let mut aliases = family_aliases_from_sfnt_names(font_info);
        aliases.retain(|n| *n != full_name && *n != family);

        Names {
            full_name,
            family,
            sub_family: Some(sub_family),
            postscript_name: Some(postscript_name),
            aliases,
        }
    }
}

impl ParsedFont {
    pub fn from_locator(handle: &FontDataHandle) -> anyhow::Result<Self> {
        let font_info = SwashFontInfo::from_locator(&handle.source, handle.index)?;
        Self::from_face(&font_info, handle.clone())
    }

    pub fn aka(&self) -> String {
        if self.names.aliases.is_empty() {
            String::new()
        } else {
            format!("(AKA: {}) ", self.names.aliases.join(", "))
        }
    }

    /// Render as a ktav `FontAttributes` object literal, e.g.
    /// `{ family: Lucida Console, weight: Bold, stretch: SemiCondensed, style: Italic }`,
    /// suitable for pasting directly into a `font.font` array in a ktav config.
    pub fn ktav_name(&self) -> String {
        format!(
            "{{ family: {}, weight: {}, stretch: {}, style: {} }}",
            self.names.family, self.weight, self.stretch, self.style
        )
    }

    /// Render as a ktav `font: { font: [ ... ] }` block (the shape a `TextStyle`
    /// takes in a ktav config), with `##` comments describing where each entry
    /// was resolved from. Intended to be copy-pasted straight into a config file.
    pub fn ktav_fallback(handles: &[Self]) -> String {
        let mut code = "font: {\n    font: [\n".to_string();

        for p in handles {
            // Paths may contain backslashes (e.g. on Windows); that's fine
            // inside a `##` comment, which is never parsed as ktav syntax.
            code.push_str(&format!(
                "        ## {}\n",
                p.handle.diagnostic_string()
            ));
            if p.synthesize_italic {
                code.push_str("        ## Will synthesize italics\n");
            }
            if p.synthesize_bold {
                code.push_str("        ## Will synthesize bold\n");
            } else if p.synthesize_dim {
                code.push_str("        ## Will synthesize dim\n");
            }
            if p.assume_emoji_presentation {
                code.push_str("        ## Assumed to have Emoji Presentation\n");
            }
            if !p.pixel_sizes.is_empty() {
                code.push_str(&format!("        ## Pixel sizes: {:?}\n", p.pixel_sizes));
            }
            if !p.palettes.is_empty() {
                for pal in &p.palettes {
                    let mut info = format!(
                        "        ## Palette: {} {}",
                        pal.palette_index,
                        pal.name.to_string()
                    );
                    if pal.usable_with_light_bg {
                        info.push_str(" (with light bg)");
                    }
                    if pal.usable_with_dark_bg {
                        info.push_str(" (with dark bg)");
                    }
                    info.push('\n');
                    code.push_str(&info);
                }
            }
            for aka in &p.names.aliases {
                code.push_str(&format!("        ## AKA: {}\n", aka));
            }

            if p.weight == FontWeight::REGULAR
                && p.stretch == FontStretch::Normal
                && p.style == FontStyle::Normal
                && p.freetype_render_target.is_none()
                && p.freetype_load_target.is_none()
                && p.freetype_load_flags.is_none()
                && p.harfbuzz_features.is_none()
                && p.scale.is_none()
            {
                code.push_str(&format!("        {{ family: {} }}\n", p.names.family));
            } else {
                code.push_str(&format!("        {{ family: {}", p.names.family));
                if p.weight != FontWeight::REGULAR {
                    code.push_str(&format!(", weight: {}", p.weight));
                }
                if p.stretch != FontStretch::Normal {
                    code.push_str(&format!(", stretch: {}", p.stretch));
                }
                if p.style != FontStyle::Normal {
                    code.push_str(&format!(", style: {}", p.style));
                }
                if let Some(scale) = p.scale {
                    code.push_str(&format!(", scale: {}", scale));
                }
                if let Some(item) = p.freetype_load_flags {
                    code.push_str(&format!(", freetype_load_flags: {}", item.to_string()));
                }
                if let Some(item) = p.freetype_load_target {
                    code.push_str(&format!(", freetype_load_target: {:?}", item));
                }
                if let Some(item) = p.freetype_render_target {
                    code.push_str(&format!(", freetype_render_target: {:?}", item));
                }
                if let Some(feat) = &p.harfbuzz_features {
                    code.push_str(", harfbuzz_features: [");
                    for (idx, f) in feat.iter().enumerate() {
                        if idx > 0 {
                            code.push_str(", ");
                        }
                        code.push_str(f);
                    }
                    code.push(']');
                }
                code.push_str(" }\n")
            }
            code.push_str("\n");
        }
        code.push_str("    ]\n}");
        code
    }

    pub fn from_face(font_info: &SwashFontInfo, handle: FontDataHandle) -> anyhow::Result<Self> {
        let style = if font_info.is_italic() {
            FontStyle::Italic
        } else {
            FontStyle::Normal
        };
        // `handle.variation` selects a named instance the same way
        // `ftwrap::Face::variations()` used to (see
        // `parse_and_collect_font_info`/`SwashFontInfo::instances`
        // below): instance 0 means "the font's default (non-variable, or
        // variable-at-defaults) instance", matching FreeType's
        // `FT_Set_Named_Instance(face, 0)` convention.
        let instance_index = if handle.variation == 0 {
            None
        } else {
            Some((handle.variation - 1) as usize)
        };
        let (ot_weight, width) = font_info.weight_and_width(instance_index);
        let weight = FontWeight::from_opentype_weight(ot_weight);
        let stretch = FontStretch::from_opentype_stretch(width);
        let cap_height = font_info.cap_height_ratio();
        let pixel_sizes = font_info.pixel_sizes();

        let palettes = font_info
            .palettes()
            .into_iter()
            .map(|p| FontPaletteInfo {
                name: p.name.unwrap_or_default(),
                palette_index: p.index as usize,
                usable_with_light_bg: p.usable_with_light_bg,
                usable_with_dark_bg: p.usable_with_dark_bg,
            })
            .collect();

        let has_svg = font_info.has_svg();

        if has_svg {
            if config::configuration().ignore_svg_fonts {
                anyhow::bail!("skipping svg font because ignore_svg_fonts=true");
            }
        }

        let has_color = font_info.has_color();
        let assume_emoji_presentation = has_color;

        let names = Names::from_face(font_info);
        // Objectively gross, but freetype's italic property is very coarse grained.
        // fontconfig resorts to name matching, so we do too :-/
        let style = match style {
            FontStyle::Normal => {
                let lower = names.full_name.to_ascii_lowercase();
                if lower.contains("italic") || lower.contains("kursiv") {
                    FontStyle::Italic
                } else if lower.contains("oblique") {
                    FontStyle::Oblique
                } else {
                    FontStyle::Normal
                }
            }
            FontStyle::Italic => {
                let lower = names.full_name.to_ascii_lowercase();
                if lower.contains("oblique") {
                    FontStyle::Oblique
                } else {
                    FontStyle::Italic
                }
            }
            // Currently "impossible" because freetype only knows italic or normal
            FontStyle::Oblique => FontStyle::Oblique,
        };

        let weight = match weight {
            FontWeight::REGULAR => {
                let lower = names.full_name.to_lowercase();
                let mut weight = weight;
                for (label, candidate) in &[
                    ("extrablack", FontWeight::EXTRABLACK),
                    // must match after other black variants
                    ("black", FontWeight::BLACK),
                    ("extrabold", FontWeight::EXTRABOLD),
                    ("demibold", FontWeight::DEMIBOLD),
                    // must match after other bold variants
                    ("bold", FontWeight::BOLD),
                    ("medium", FontWeight::MEDIUM),
                    ("book", FontWeight::BOOK),
                    ("demilight", FontWeight::DEMILIGHT),
                    ("extralight", FontWeight::EXTRALIGHT),
                    // must match after other light variants
                    ("light", FontWeight::LIGHT),
                    ("thin", FontWeight::THIN),
                ] {
                    if lower.contains(label) {
                        weight = *candidate;
                        break;
                    }
                }
                weight
            }
            weight => weight,
        };

        let stretch = match stretch {
            FontStretch::Normal => {
                let lower = names.full_name.to_lowercase();
                let mut stretch = stretch;
                for (label, value) in &[
                    ("ultracondensed", FontStretch::UltraCondensed),
                    ("extracondensed", FontStretch::ExtraCondensed),
                    ("semicondensed", FontStretch::SemiCondensed),
                    // must match after other condensed variants
                    ("condensed", FontStretch::Condensed),
                    ("semiexpanded", FontStretch::SemiExpanded),
                    ("extraexpanded", FontStretch::ExtraExpanded),
                    ("ultraexpanded", FontStretch::UltraExpanded),
                    // must match after other expanded variants
                    ("expanded", FontStretch::Expanded),
                ] {
                    if lower.contains(label) {
                        stretch = *value;
                        break;
                    }
                }

                stretch
            }
            stretch => stretch,
        };

        Ok(Self {
            names,
            weight,
            stretch,
            style,
            synthesize_italic: false,
            synthesize_bold: false,
            synthesize_dim: false,
            is_built_in_fallback: false,
            assume_emoji_presentation,
            handle,
            coverage: Mutex::new(RangeSet::new()),
            cap_height,
            pixel_sizes,
            harfbuzz_features: None,
            freetype_render_target: None,
            freetype_load_target: None,
            freetype_load_flags: None,
            scale: None,
            palettes,
        })
    }

    /// Computes the intersection of the wanted set of codepoints with
    /// the set of codepoints covered by this font entry.
    /// Computes the codepoint coverage for this font entry if we haven't
    /// already done so.
    pub fn coverage_intersection(&self, wanted: &RangeSet<u32>) -> anyhow::Result<RangeSet<u32>> {
        let mut cov = self.coverage.lock().unwrap();
        if cov.is_empty() {
            let t = std::time::Instant::now();
            let font_info = SwashFontInfo::from_locator(&self.handle.source, self.handle.index)?;
            *cov = font_info.compute_coverage();
            let elapsed = t.elapsed();
            metrics::histogram!("font.compute.codepoint.coverage").record(elapsed);
            log::debug!(
                "{} codepoint coverage computed in {:?}",
                self.names.full_name,
                elapsed
            );
        }
        Ok(wanted.intersection(&cov))
    }

    /// Returns the human-readable glyph name (e.g. a PostScript/AGL glyph
    /// name such as `"A"` or `"uni25CF"`) for the given glyph id, if the
    /// font provides one. Diagnostic-only (used by `wezterm-gui`'s `ls-fonts`/
    /// text-shaping debug CLI output); equivalent to the removed
    /// `ftwrap::Face::get_glyph_name` (`FT_Get_Glyph_Name`), backed by
    /// `ttf_parser::Face::glyph_name` instead (reads the same `post`/CFF
    /// charstring glyph-name data FreeType did, just without a C library
    /// in the loop).
    pub fn glyph_name(&self, glyph_pos: u32) -> Option<String> {
        let data = self.handle.source.load_data().ok()?;
        let face = ttf_parser::Face::parse(&data, self.handle.index).ok()?;
        face.glyph_name(ttf_parser::GlyphId(glyph_pos as u16))
            .map(|s| s.to_string())
    }

    pub fn names(&self) -> &Names {
        &self.names
    }

    pub fn weight(&self) -> FontWeight {
        self.weight
    }

    pub fn stretch(&self) -> FontStretch {
        self.stretch
    }

    pub fn style(&self) -> FontStyle {
        self.style
    }

    pub fn matches_name(&self, attr: &FontAttributes) -> bool {
        if attr.family == self.names.family {
            return true;
        }
        if let Some(path) = self.handle.path_str() {
            if attr.family == path {
                return true;
            }
        }
        self.matches_full_or_ps_name(attr) || self.matches_alias(attr)
    }

    pub fn matches_alias(&self, attr: &FontAttributes) -> bool {
        for a in &self.names.aliases {
            if *a == attr.family {
                return true;
            }
        }
        false
    }

    pub fn matches_full_or_ps_name(&self, attr: &FontAttributes) -> bool {
        if attr.family == self.names.full_name {
            return true;
        }
        if let Some(ps) = self.names.postscript_name.as_ref() {
            if attr.family == *ps {
                return true;
            }
        }
        false
    }

    /// Perform CSS Fonts Level 3 font matching.
    /// This implementation is derived from the `find_best_match` function
    /// in the font-kit crate which is
    /// Copyright © 2018 The Pathfinder Project Developers.
    /// https://drafts.csswg.org/css-fonts-3/#font-style-matching says
    pub fn best_matching_index<P: std::ops::Deref<Target = Self> + std::fmt::Debug>(
        attr: &FontAttributes,
        fonts: &[P],
        pixel_size: u16,
    ) -> Option<usize> {
        if fonts.is_empty() {
            return None;
        }

        let mut candidates: Vec<usize> = (0..fonts.len()).collect();

        // First, filter by stretch
        let stretch_value = attr.stretch.to_opentype_stretch();
        let stretch = if candidates
            .iter()
            .any(|&idx| fonts[idx].stretch == attr.stretch)
        {
            attr.stretch
        } else if attr.stretch <= FontStretch::Normal {
            // Find the closest stretch, looking at narrower first before
            // looking at wider candidates
            match candidates
                .iter()
                .filter(|&&idx| fonts[idx].stretch < attr.stretch)
                .min_by_key(|&&idx| stretch_value - fonts[idx].stretch.to_opentype_stretch())
            {
                Some(&idx) => fonts[idx].stretch,
                None => {
                    let idx = *candidates.iter().min_by_key(|&&idx| {
                        fonts[idx].stretch.to_opentype_stretch() - stretch_value
                    })?;
                    fonts[idx].stretch
                }
            }
        } else {
            // Look at wider values, then narrower values
            match candidates
                .iter()
                .filter(|&&idx| fonts[idx].stretch > attr.stretch)
                .min_by_key(|&&idx| fonts[idx].stretch.to_opentype_stretch() - stretch_value)
            {
                Some(&idx) => fonts[idx].stretch,
                None => {
                    let idx = *candidates.iter().min_by_key(|&&idx| {
                        stretch_value - fonts[idx].stretch.to_opentype_stretch()
                    })?;
                    fonts[idx].stretch
                }
            }
        };

        // Reduce to matching stretches
        candidates.retain(|&idx| fonts[idx].stretch == stretch);

        // Now match style: italics.
        let styles = match attr.style {
            FontStyle::Normal => [FontStyle::Normal, FontStyle::Italic, FontStyle::Oblique],
            FontStyle::Italic => [FontStyle::Italic, FontStyle::Oblique, FontStyle::Normal],
            FontStyle::Oblique => [FontStyle::Oblique, FontStyle::Italic, FontStyle::Normal],
        };
        let style = *styles
            .iter()
            .find(|&&style| candidates.iter().any(|&idx| fonts[idx].style == style))?;

        // Reduce to matching italics
        candidates.retain(|&idx| fonts[idx].style == style);

        // And now match by font weight
        let query_weight = attr.weight.to_opentype_weight();
        let weight = if candidates
            .iter()
            .any(|&idx| fonts[idx].weight == attr.weight)
        {
            // Exact match for the requested weight
            attr.weight
        } else if attr.weight == FontWeight::REGULAR
            && candidates
                .iter()
                .any(|&idx| fonts[idx].weight == FontWeight::MEDIUM)
        {
            // https://drafts.csswg.org/css-fonts-3/#font-style-matching says
            // that if they want weight=400 and we don't have 400,
            // look at weight 500 first
            FontWeight::MEDIUM
        } else if attr.weight == FontWeight::MEDIUM
            && candidates
                .iter()
                .any(|&idx| fonts[idx].weight == FontWeight::REGULAR)
        {
            // Similarly, look at regular before Medium if they wanted
            // Medium and we didn't have it
            FontWeight::REGULAR
        } else if attr.weight <= FontWeight::MEDIUM {
            // Find best lighter weight, else best heavier weight
            match candidates
                .iter()
                .filter(|&&idx| fonts[idx].weight <= attr.weight)
                .min_by_key(|&&idx| query_weight - fonts[idx].weight.to_opentype_weight())
            {
                Some(&idx) => fonts[idx].weight,
                None => {
                    let idx = *candidates.iter().min_by_key(|&&idx| {
                        fonts[idx].weight.to_opentype_weight() - query_weight
                    })?;
                    fonts[idx].weight
                }
            }
        } else {
            // Find best heavier weight, else best lighter weight
            match candidates
                .iter()
                .filter(|&&idx| fonts[idx].weight >= attr.weight)
                .min_by_key(|&&idx| fonts[idx].weight.to_opentype_weight() - query_weight)
            {
                Some(&idx) => fonts[idx].weight,
                None => {
                    let idx = *candidates.iter().min_by_key(|&&idx| {
                        query_weight - fonts[idx].weight.to_opentype_weight()
                    })?;
                    fonts[idx].weight
                }
            }
        };

        // Reduce to matching weight
        candidates.retain(|&idx| fonts[idx].weight == weight);

        // Check for best matching pixel strike, but only if all
        // candidates have pixel strikes
        if candidates
            .iter()
            .all(|&idx| !fonts[idx].pixel_sizes.is_empty())
        {
            if let Some((_distance, idx)) = candidates
                .iter()
                .map(|&idx| {
                    let distance = fonts[idx]
                        .pixel_sizes
                        .iter()
                        .map(|&size| ((pixel_size as i32) - (size as i32)).abs())
                        .min()
                        .unwrap_or(i32::MAX);
                    (distance, idx)
                })
                .min()
            {
                return Some(idx);
            }
        }

        // The first one in this set is our best match
        candidates.into_iter().next()
    }

    pub fn best_match(
        attr: &FontAttributes,
        pixel_size: u16,
        mut fonts: Vec<Self>,
    ) -> Option<Self> {
        let refs: Vec<&Self> = fonts.iter().collect();
        let idx = Self::best_matching_index(attr, &refs, pixel_size)?;
        fonts.drain(idx..=idx).next().map(|p| p.synthesize(attr))
    }

    /// Update self to reflect whether the rasterizer might need to synthesize
    /// italic for this font.
    pub fn synthesize(mut self, attr: &FontAttributes) -> Self {
        self.harfbuzz_features = attr.harfbuzz_features.clone();
        self.freetype_render_target = attr.freetype_render_target;
        self.freetype_load_target = attr.freetype_load_target;
        self.freetype_load_flags = attr.freetype_load_flags;
        self.scale = attr.scale.map(|f| *f);

        self.synthesize_italic = self.style == FontStyle::Normal && attr.style != FontStyle::Normal;
        self.synthesize_bold = attr.weight >= FontWeight::DEMIBOLD
            && attr.weight > self.weight
            && self.weight <= FontWeight::REGULAR;
        self.synthesize_dim = attr.weight < FontWeight::REGULAR
            && attr.weight < self.weight
            && self.weight >= FontWeight::REGULAR;

        match attr.assume_emoji_presentation {
            Some(assume) => {
                self.assume_emoji_presentation = assume;
            }
            None => {
                // If they explicitly list an emoji font, assume that they
                // want it to be used for emoji presentation.
                // We match on "moji" rather than "emoji" as there are
                // emoji fonts that are moji rather than emoji :-/
                // This heuristic is awful, TBH.
                if !self.is_built_in_fallback
                    && !attr.is_synthetic
                    && self.names.full_name.to_ascii_lowercase().contains("moji")
                {
                    self.assume_emoji_presentation = true;
                }
            }
        }

        self
    }
}

/// In case the user has a broken configuration, or no configuration,
/// we bundle JetBrains Mono, Noto Color Emoji and Cascadia Mono (for
/// monospace Hebrew consonants) to act as reasonably sane fallback fonts.
/// This function loads those.
pub(crate) fn load_built_in_fonts(font_info: &mut Vec<ParsedFont>) -> anyhow::Result<()> {
    #[allow(unused_macros)]
    macro_rules! font {
        ($font:literal) => {
            (include_bytes!($font) as &'static [u8], $font)
        };
    }

    let built_ins: &[&[(&[u8], &str)]] = &[
        #[cfg(any(test, feature = "vendor-jetbrains"))]
        &[
            font!("../../../assets/fonts/JetBrainsMono-BoldItalic.ttf"),
            font!("../../../assets/fonts/JetBrainsMono-Bold.ttf"),
            font!("../../../assets/fonts/JetBrainsMono-ExtraBoldItalic.ttf"),
            font!("../../../assets/fonts/JetBrainsMono-ExtraBold.ttf"),
            font!("../../../assets/fonts/JetBrainsMono-ExtraLightItalic.ttf"),
            font!("../../../assets/fonts/JetBrainsMono-ExtraLight.ttf"),
            font!("../../../assets/fonts/JetBrainsMono-Italic.ttf"),
            font!("../../../assets/fonts/JetBrainsMono-LightItalic.ttf"),
            font!("../../../assets/fonts/JetBrainsMono-Light.ttf"),
            font!("../../../assets/fonts/JetBrainsMono-MediumItalic.ttf"),
            font!("../../../assets/fonts/JetBrainsMono-Medium.ttf"),
            font!("../../../assets/fonts/JetBrainsMono-Regular.ttf"),
            font!("../../../assets/fonts/JetBrainsMono-SemiBoldItalic.ttf"),
            font!("../../../assets/fonts/JetBrainsMono-SemiBold.ttf"),
            font!("../../../assets/fonts/JetBrainsMono-ThinItalic.ttf"),
            font!("../../../assets/fonts/JetBrainsMono-Thin.ttf"),
        ],
        #[cfg(any(test, feature = "vendor-roboto"))]
        &[
            font!("../../../assets/fonts/Roboto-Black.ttf"),
            font!("../../../assets/fonts/Roboto-BlackItalic.ttf"),
            font!("../../../assets/fonts/Roboto-Bold.ttf"),
            font!("../../../assets/fonts/Roboto-BoldItalic.ttf"),
            font!("../../../assets/fonts/Roboto-Italic.ttf"),
            font!("../../../assets/fonts/Roboto-Light.ttf"),
            font!("../../../assets/fonts/Roboto-LightItalic.ttf"),
            font!("../../../assets/fonts/Roboto-Medium.ttf"),
            font!("../../../assets/fonts/Roboto-MediumItalic.ttf"),
            font!("../../../assets/fonts/Roboto-Regular.ttf"),
            font!("../../../assets/fonts/Roboto-Thin.ttf"),
            font!("../../../assets/fonts/Roboto-ThinItalic.ttf"),
        ],
        #[cfg(any(test, feature = "vendor-noto-emoji"))]
        &[font!("../../../assets/fonts/NotoColorEmoji.ttf")],
        #[cfg(any(test, feature = "vendor-hebrew"))]
        &[
            font!("../../../assets/fonts/CascadiaMono-Regular.ttf"),
            font!("../../../assets/fonts/CascadiaMono-Bold.ttf"),
        ],
        #[cfg(any(test, feature = "vendor-nerd-font-symbols"))]
        &[font!("../../../assets/fonts/SymbolsNerdFontMono-Regular.ttf")],
    ];
    for bundle in built_ins {
        for (data, name) in bundle.iter() {
            let locator = FontDataHandle {
                source: FontDataSource::BuiltIn { data, name },
                index: 0,
                variation: 0,
                origin: FontOrigin::BuiltIn,
                coverage: None,
            };
            let mut parsed = ParsedFont::from_locator(&locator)?;
            parsed.is_built_in_fallback = true;
            font_info.push(parsed);
        }
    }

    Ok(())
}

pub fn best_matching_font(
    source: &FontDataSource,
    font_attr: &FontAttributes,
    origin: FontOrigin,
    pixel_size: u16,
) -> anyhow::Result<Option<ParsedFont>> {
    let mut font_info = vec![];
    parse_and_collect_font_info(source, &mut font_info, origin)?;
    font_info.retain(|font| font.matches_name(font_attr));
    Ok(ParsedFont::best_match(font_attr, pixel_size, font_info))
}

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
        if let Err(err) = load_one(&source, &data, index, font_info, &origin) {
            log::trace!("error while parsing {:?} index {}: {}", source, index, err);
        }
    }

    Ok(())
}

#[cfg(test)]
mod perf1_test {
    //! Regression coverage for PERF1: `parse_and_collect_font_info` used
    //! to call `SwashFontInfo::from_locator` once per sub-face of a
    //! `.ttc`/`.otc` collection, and `from_locator` re-reads the *entire*
    //! file off disk every time it's called -- so a collection with N
    //! sub-faces was read off disk N+1 times over (once just to learn the
    //! face count via `fonts_in_collection`, then once again per
    //! sub-face), even though every sub-face lives in the same file. For
    //! large CJK system font collections (e.g. `mingliub.ttc` at ~37MB
    //! with 3 faces, or `Sitka*.ttc` with 6 faces each) that made
    //! `enumerate_all_fonts`/`ls-fonts --list-system` measurably slower
    //! than it needs to be. The fix reads the file once and reuses the
    //! in-memory bytes (a cheap memcpy, not I/O) for every sub-face via
    //! `SwashFontInfo::from_data`.
    use super::*;
    use crate::locator::FontOrigin;
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    /// A real (not hand-rolled) 2-face TrueType Collection built with
    /// `fonttools`'s `TTCollection` writer from
    /// `assets/fonts/JetBrainsMono-{Regular,Bold}.ttf`, checked in purely
    /// for this regression test (there was no `.ttc` fixture in the repo
    /// before this).
    fn ttc_fixture_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("testdata")
            .join("JetBrainsMono-RegularBold.ttc")
    }

    /// Sanity check that multi-face enumeration still produces one
    /// `ParsedFont` per sub-face (i.e. the read-once refactor didn't
    /// silently break the loop over `num_faces`).
    #[test]
    fn ttc_enumeration_yields_one_entry_per_face() {
        let source = FontDataSource::OnDisk(ttc_fixture_path());
        let mut font_info = vec![];
        parse_and_collect_font_info(&source, &mut font_info, FontOrigin::FontDirs).unwrap();

        assert_eq!(
            font_info.len(),
            2,
            "expected exactly one ParsedFont per sub-face of the 2-face ttc fixture, got {:?}",
            font_info
                .iter()
                .map(|f| f.names().full_name.clone())
                .collect::<Vec<_>>()
        );

        let mut full_names: Vec<&str> = font_info
            .iter()
            .map(|f| f.names().full_name.as_str())
            .collect();
        full_names.sort();
        assert_eq!(
            full_names,
            vec!["JetBrains Mono Bold", "JetBrains Mono Regular"]
        );
    }

    /// `SwashFontInfo::from_data` (fed already-loaded bytes) must produce
    /// results identical to `SwashFontInfo::from_locator` (which loads the
    /// bytes itself) for the same face -- i.e. splitting "load the bytes"
    /// out of `from_locator` must be a pure refactor with no behavior
    /// change.
    #[test]
    fn from_data_matches_from_locator() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("assets/fonts/JetBrainsMono-Regular.ttf");
        let source = FontDataSource::OnDisk(path);

        let via_locator = SwashFontInfo::from_locator(&source, 0).unwrap();
        let raw = source.load_data().unwrap().into_owned().into_boxed_slice();
        let via_data = SwashFontInfo::from_data(raw, 0).unwrap();

        assert_eq!(via_locator.family_name(), via_data.family_name());
        assert_eq!(via_locator.style_name(), via_data.style_name());
        assert_eq!(via_locator.units_per_em(), via_data.units_per_em());
        assert_eq!(via_locator.num_glyphs(), via_data.num_glyphs());
        assert_eq!(
            via_locator.weight_and_width(None),
            via_data.weight_and_width(None)
        );
    }

    /// Coarse wall-clock regression guard: enumerating the 2-face ttc
    /// fixture (~475KB total) should be well within a generous upper
    /// bound. This is not a tight perf assertion (machine/CI speed
    /// varies enormously) -- it exists to catch a *gross* regression like
    /// accidentally reintroducing an O(num_faces) full-file re-read (or
    /// worse, an O(num_faces^2) pattern) for collections, which is
    /// exactly the bug this change fixes. The bound is generous enough
    /// (2s for a ~475KB file processed twice) that it should never be
    /// flaky on any real CI/dev machine, while still being tight enough
    /// to catch a reintroduced per-face disk re-read of a much larger
    /// real-world collection (which this test's small fixture can't
    /// directly reproduce the magnitude of, but a correctness-preserving
    /// refactor should not care about fixture size).
    #[test]
    fn ttc_enumeration_completes_quickly() {
        let source = FontDataSource::OnDisk(ttc_fixture_path());

        let start = Instant::now();
        for _ in 0..10 {
            let mut font_info = vec![];
            parse_and_collect_font_info(&source, &mut font_info, FontOrigin::FontDirs).unwrap();
            assert_eq!(font_info.len(), 2);
        }
        let elapsed = start.elapsed();

        assert!(
            elapsed < Duration::from_secs(2),
            "enumerating the 2-face ttc fixture 10 times took {:?}, expected well under 2s \
             (possible regression: re-reading the file from disk per sub-face instead of once)",
            elapsed
        );
    }
}
