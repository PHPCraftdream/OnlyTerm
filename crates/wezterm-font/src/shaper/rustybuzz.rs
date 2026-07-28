//! `RustybuzzShaper`: an implementation of the `FontShaper` trait built on
//! the pure-Rust `rustybuzz` crate instead of the C/C++ `harfbuzz` library.
//!
//! This is part of the freetype+harfbuzz -> rustybuzz+swash migration
//! (see docs/plans/2026-07-23-freetype-harfbuzz-migration.md, phase H1).
//!
//! H0 (formerly `wezterm-font/examples/dump_shaping.rs`, a comparison tool
//! deleted in phase H4 once the harfbuzz crate it compared against was
//! removed - see git history if you need the original side-by-side
//! output) established that:
//! - `glyph_id`/`cluster` sequences from raw `rustybuzz::shape()` matched the
//!   old production `HarfbuzzShaper` 1:1 on latin text, ligatures (FiraCode-style)
//!   and emoji ZWJ sequences.
//! - `x_advance`/`y_advance`/`x_offset`/`y_offset` differed, because the old
//!   production harfbuzz path (`USE_OT_FUNCS=false`) delegated glyph metrics
//!   to FreeType via `hb_ft_font_create_referenced` + `hb_ft_font_set_load_flags`,
//!   which meant every advance/offset that came back from `hb_shape` had
//!   already been hinted (grid-fit) by FreeType's TrueType bytecode
//!   interpreter/autohinter and expressed in 26.6 fixed-point pixels.
//!
//!   rustybuzz has no equivalent of `hb_font_set_scale`/`hb_ft_font_*`: it
//!   always reports glyph positions in the font's raw design units
//!   (`unitsPerEm` space) with no hinting/grid-fitting applied at all. To be
//!   directly comparable we had to scale ourselves:
//!       advance_px = raw_units * (point_size * dpi / 72) / units_per_em
//!   (this is the same formula `SwashFontInfo::selected_font_size` uses to
//!   compute the nominal pixel height).
//!
//!   FreeType's hinting on top of that scaling mostly manifests as rounding
//!   to whole pixels (26.6 fixed-point, i.e. 1/64 pixel granularity, but for
//!   most well-behaved monospace fonts at typical terminal sizes the hinted
//!   advance lands on an integral pixel boundary so that every cell in the
//!   grid is exactly the same width). We approximate that by rounding the
//!   scaled advance to the nearest pixel. This is not a bit-exact
//!   replication of FreeType's hinter (that would require reimplementing
//!   the TrueType bytecode VM), but it reproduces the property that matters
//!   for terminal rendering: monospace glyphs get a consistent, integral
//!   pixel advance instead of accumulating sub-pixel drift. Offsets
//!   (x_offset/y_offset) are generally zero for simple text and small
//!   relative to cell width for mark positioning, so we scale them the same
//!   way but do not force them to integral pixels (FreeType doesn't either;
//!   only the *advance* is grid-fit, offsets follow the outline hinting and
//!   can be fractional).
use crate::parser::ParsedFont;
use crate::shaper::{FallbackIdx, FontMetrics, FontShaper, GlyphInfo, PresentationWidth};
use crate::units::*;
use anyhow::{anyhow, Context};
use config::ConfigHandle;
use finl_unicode::grapheme_clusters::Graphemes;
use log::error;
use ordered_float::NotNan;
use std::cell::{RefCell, RefMut};
use std::collections::HashMap;
use std::ops::Range;
use termwiz::cell::{unicode_column_width, Presentation};
use wezterm_bidi::Direction;

/// Owns the raw font bytes backing a `rustybuzz::Face<'static>`.
///
/// `rustybuzz::Face<'a>` borrows its input buffer, so we need something to
/// keep the bytes alive for exactly as long as the `Face` that references
/// them. We box the bytes (a stable heap allocation that never moves once
/// created) and hand `rustybuzz` a pointer with an unsafely-extended
/// `'static` lifetime; the `Face` and the owning `Box<[u8]>` are stored
/// side by side in this struct and dropped together, so the borrow is
/// always valid for as long as anyone can observe it.
struct OwnedRbFace {
    // Order matters for drop safety in spirit (though neither type's Drop
    // impl actually touches the other's memory): the face borrows from
    // `_data`, so we keep them paired in a single struct rather than ever
    // handing the `Face` out on its own.
    face: rustybuzz::Face<'static>,
    _data: Box<[u8]>,
}

impl OwnedRbFace {
    fn from_bytes(data: Box<[u8]>, face_index: u32) -> anyhow::Result<Self> {
        let slice: &[u8] = &data;
        // Safety: `data` is heap-allocated (`Box<[u8]>`) and its address is
        // stable; we keep `_data` alive alongside `face` for the lifetime of
        // this struct, and never expose `face` in a way that could outlive
        // `_data` (it's private and only accessed through methods on
        // `OwnedRbFace`). This extends the borrow from the true lifetime of
        // `slice` (tied to `data`, which we own) to `'static` purely so the
        // two fields can live in the same struct.
        let static_slice: &'static [u8] = unsafe { std::mem::transmute(slice) };
        let face = rustybuzz::Face::from_slice(static_slice, face_index)
            .ok_or_else(|| anyhow!("rustybuzz failed to parse font face"))?;
        Ok(Self { face, _data: data })
    }
}

#[derive(Clone, Debug)]
struct Info {
    cluster: usize,
    len: usize,
    codepoint: u32,
    x_advance: f64,
    y_advance: f64,
    x_offset: f64,
    y_offset: f64,
}

fn get_only_char(s: &str) -> Option<char> {
    let mut chars = s.chars();
    let first_char = chars.next()?;
    if chars.next().is_some() {
        None
    } else {
        Some(first_char)
    }
}

fn make_glyphinfo(text: &str, num_cells: u8, font_idx: usize, info: &Info) -> GlyphInfo {
    let is_space = text == " ";
    let only_char = get_only_char(text);
    GlyphInfo {
        #[cfg(any(debug_assertions, test))]
        text: text.into(),
        only_char,
        is_space,
        num_cells,
        font_idx,
        glyph_pos: info.codepoint,
        cluster: info.cluster as u32,
        x_advance: PixelLength::new(info.x_advance),
        y_advance: PixelLength::new(info.y_advance),
        x_offset: PixelLength::new(info.x_offset),
        y_offset: PixelLength::new(info.y_offset),
    }
}

/// If `handle` selects a named instance of a variable font (as
/// enumerated by `SwashFontInfo::instances`/`parse_and_collect_font_info`,
/// one `ParsedFont` per `fvar` named instance, with `handle.variation`
/// recording which one - see `parser.rs`), resolve that instance's
/// design-space (user-unit, not normalized) coordinates so we can apply
/// the equivalent variation to the rustybuzz/ttf-parser face (which has
/// no notion of "this face index selects a named instance" - it only
/// understands explicit axis tag/value pairs via `set_variation`).
///
/// `SwashInstance::user_values` (from `font.instances()`) already reports
/// each axis's coordinate in the same user-space units
/// `rustybuzz::Face::set_variation`/`ttf_parser::Face::set_variation`
/// expect (the same units as `VariationAxis::min_value`/`def_value`/
/// `max_value`, e.g. a `wght` value like `700.0`), so this only needs to
/// pair each value up with its axis tag - no fvar/avar
/// normalization-math is involved (unlike `SwashInstance::
/// normalized_coords`, which *is* normalized to -1.0..=1.0 and would
/// need denormalizing against axis min/default/max to get here).
fn variation_coords_for(
    ttf_face: &ttf_parser::Face,
    font_info: &crate::swash_metrics::SwashFontInfo,
    handle: &crate::parser::ParsedFont,
) -> Vec<rustybuzz::Variation> {
    let mut coords = vec![];
    if handle.handle.variation == 0 {
        return coords;
    }
    let vidx = (handle.handle.variation - 1) as usize;
    let instances = font_info.instances();
    let Some(instance) = instances.get(vidx) else {
        return coords;
    };

    for (axis, &value) in ttf_face
        .variation_axes()
        .into_iter()
        .zip(instance.user_values.iter())
    {
        coords.push(rustybuzz::Variation {
            tag: axis.tag,
            value,
        });
    }
    coords
}

struct FontPair {
    /// Parsing/metrics counterpart to `rb_face`, built on `swash` (see
    /// `swash_metrics.rs`) rather than FreeType - used by `metrics_for_idx`/
    /// `metrics` for cell/underline/cap-height metrics, and by
    /// `variation_coords_for` (via `ensure_rb_face`) to resolve named
    /// instances.
    font_info: crate::swash_metrics::SwashFontInfo,
    rb_face: RefCell<Option<OwnedRbFace>>,
    shaped_any: bool,
    presentation: Presentation,
    features: Vec<rustybuzz::Feature>,
    variations: RefCell<Option<Vec<rustybuzz::Variation>>>,
    last_size_and_dpi: RefCell<Option<(f64, u32)>>,
    units_per_em: RefCell<Option<f64>>,
}

#[derive(Debug, Hash, PartialEq, Eq, Clone)]
struct MetricsKey {
    font_idx: usize,
    size: NotNan<f64>,
    dpi: u32,
}

pub struct RustybuzzShaper {
    handles: Vec<ParsedFont>,
    fonts: Vec<RefCell<Option<FontPair>>>,
    metrics: RefCell<HashMap<MetricsKey, FontMetrics>>,
    features: Vec<rustybuzz::Feature>,
    lang: rustybuzz::Language,
}

/// Defensively clamp a byte range to `s`'s actual char boundaries before
/// slicing it. `range` is expected to already land on char boundaries (its
/// endpoints are meant to be real rustybuzz cluster starts, which are
/// themselves always valid boundaries) - but if some combination of cluster
/// merging/dedup logic ever computes a range that doesn't quite line up
/// (rather than tracking down every possible cause up front), rounding the
/// endpoints outward to the nearest real char boundary avoids a hard panic
/// from indexing into the middle of a multi-byte character. Widening (never
/// narrowing) means we may include a byte or two of extra, adjacent text in
/// the shaped substring in that edge case, which is a far better outcome
/// than crashing the whole renderer.
fn clamp_to_char_boundaries(s: &str, range: Range<usize>) -> Range<usize> {
    let len = s.len();
    let mut start = range.start.min(len);
    while start > 0 && !s.is_char_boundary(start) {
        start -= 1;
    }
    let mut end = range.end.min(len);
    while end < len && !s.is_char_boundary(end) {
        end += 1;
    }
    if end < start {
        end = start;
    }
    if start != range.start.min(len) || end != range.end.min(len) {
        log::warn!("clamp_to_char_boundaries adjusted {:?} -> {:?} in {:?}", range, start..end, s);
    }
    start..end
}

/// Make a string holding a set of unicode replacement
/// characters equal to the number of graphemes in the
/// original string.  That isn't perfect, but it should
/// be good enough to indicate that something isn't right.
fn make_question_string(s: &str) -> String {
    let len = Graphemes::new(s).count();
    let mut result = String::new();
    let c = if !is_question_string(s) {
        std::char::REPLACEMENT_CHARACTER
    } else {
        '?'
    };
    for _ in 0..len {
        result.push(c);
    }
    result
}

fn is_question_string(s: &str) -> bool {
    for c in s.chars() {
        if c != std::char::REPLACEMENT_CHARACTER {
            return false;
        }
    }
    true
}

/// Parse OpenType feature strings (the same syntax used for
/// `harfbuzz_features`/`config.harfbuzz_features`, e.g. `"calt=0"`,
/// `"+liga"`, `"zero"`) into `rustybuzz::Feature`s. rustybuzz implements
/// `FromStr` for `Feature` with the identical syntax that
/// `hb_feature_from_string` accepts, so this is a drop-in replacement for
/// `hbwrap::feature_from_string`.
fn rb_feature_from_string(s: &str) -> anyhow::Result<rustybuzz::Feature> {
    s.parse::<rustybuzz::Feature>()
        .map_err(|e| anyhow!("failed to parse harfbuzz feature {s:?}: {e}"))
}

impl RustybuzzShaper {
    pub fn new(config: &ConfigHandle, handles: &[ParsedFont]) -> anyhow::Result<Self> {
        let handles = handles.to_vec();
        let mut fonts = vec![];
        for _ in 0..handles.len() {
            fonts.push(RefCell::new(None));
        }

        let lang: rustybuzz::Language = "en".parse().map_err(|e| anyhow!("{e}"))?;

        let features: Vec<rustybuzz::Feature> = config
            .harfbuzz_features
            .iter()
            .filter_map(|s| rb_feature_from_string(s).ok())
            .collect();

        Ok(Self {
            fonts,
            handles,
            metrics: RefCell::new(HashMap::new()),
            features,
            lang,
        })
    }

    fn load_fallback(
        &self,
        font_idx: FallbackIdx,
        _dpi: u32,
    ) -> anyhow::Result<Option<RefMut<'_, FontPair>>> {
        if font_idx >= self.handles.len() {
            return Ok(None);
        }
        match self.fonts.get(font_idx) {
            None => Ok(None),
            Some(opt_pair) => {
                let mut opt_pair = opt_pair.borrow_mut();
                if opt_pair.is_none() {
                    let handle = &self.handles[font_idx];
                    log::trace!("rustybuzz shaper wants {} {:?}", font_idx, handle);
                    let font_info = crate::swash_metrics::SwashFontInfo::from_locator(
                        &handle.handle.source,
                        handle.handle.index,
                    )?;

                    let features = match &handle.harfbuzz_features {
                        Some(features) => features
                            .iter()
                            .filter_map(|s| rb_feature_from_string(s).ok())
                            .collect(),
                        None => self.features.clone(),
                    };

                    *opt_pair = Some(FontPair {
                        font_info,
                        rb_face: RefCell::new(None),
                        shaped_any: false,
                        presentation: if handle.assume_emoji_presentation {
                            Presentation::Emoji
                        } else {
                            Presentation::Text
                        },
                        features,
                        variations: RefCell::new(None),
                        last_size_and_dpi: RefCell::new(None),
                        units_per_em: RefCell::new(None),
                    });
                }

                Ok(Some(RefMut::map(opt_pair, |opt_pair| {
                    opt_pair.as_mut().unwrap()
                })))
            }
        }
    }

    /// Ensure that `pair.rb_face` holds a parsed rustybuzz face (loading the
    /// raw font bytes and applying any variation coordinates on first use),
    /// and return the font's `unitsPerEm`.
    fn ensure_rb_face(&self, font_idx: FallbackIdx, pair: &FontPair) -> anyhow::Result<f64> {
        if let Some(upem) = *pair.units_per_em.borrow() {
            return Ok(upem);
        }

        let handle = &self.handles[font_idx];
        let data = handle.handle.source.load_data().with_context(|| {
            format!("loading raw font bytes for rustybuzz face {:?}", handle.handle)
        })?;
        // `handle.handle.index` is a plain collection index (see
        // `parse_and_collect_font_info`/`ParsedFont::from_face`, which
        // keep the named-instance selector entirely in the separate
        // `variation` field rather than bit-packing it into `index` the
        // way FreeType's own face-index convention used to require).
        let face_index = handle.handle.index;

        let mut owned = OwnedRbFace::from_bytes(data.clone().into_owned().into_boxed_slice(), face_index)?;

        // Resolve named-instance coordinates (if any) using the same raw
        // bytes, via a transient `ttf_parser::Face` purely to read
        // `variation_axes()` - see `variation_coords_for`'s doc comment
        // for why this needs ttf_parser rather than rustybuzz's own
        // (variation-application-only) API.
        let variations = {
            let mut cached = pair.variations.borrow_mut();
            if cached.is_none() {
                let computed = match ttf_parser::Face::parse(&data, face_index) {
                    Ok(ttf_face) => variation_coords_for(&ttf_face, &pair.font_info, handle),
                    Err(_) => vec![],
                };
                cached.replace(computed);
            }
            cached.clone().unwrap_or_default()
        };

        for variation in &variations {
            owned.face.set_variation(variation.tag, variation.value);
        }

        let upem = owned.face.units_per_em() as f64;
        pair.rb_face.borrow_mut().replace(owned);
        pair.units_per_em.borrow_mut().replace(upem);
        Ok(upem)
    }

    fn do_shape(
        &self,
        mut font_idx: FallbackIdx,
        s: &str,
        font_size: f64,
        dpi: u32,
        no_glyphs: &mut Vec<char>,
        presentation: Option<Presentation>,
        direction: Direction,
        range: Range<usize>,
        presentation_width: Option<&PresentationWidth>,
    ) -> anyhow::Result<Vec<GlyphInfo>> {
        let mut buf = rustybuzz::UnicodeBuffer::new();
        // We deliberately omit setting the script and leave it to rustybuzz
        // to infer from the buffer contents, mirroring HarfbuzzShaper (see
        // the comment in shaper/harfbuzz.rs referencing #1474/#1573).
        buf.set_direction(match direction {
            Direction::LeftToRight => rustybuzz::Direction::LeftToRight,
            Direction::RightToLeft => rustybuzz::Direction::RightToLeft,
        });
        buf.set_language(self.lang.clone());
        buf.push_str(&s[range.clone()]);
        buf.guess_segment_properties();
        buf.set_cluster_level(rustybuzz::BufferClusterLevel::MonotoneGraphemes);

        let shaped_any;
        let mut no_more_fallbacks = false;

        let glyph_buffer;
        let scale;

        loop {
            // A fallback candidate's backing file may be unreadable (eg. a
            // Windows Store / MSIX-packaged font living under an
            // ACL-protected `C:\Program Files\WindowsApps\...` path that
            // denies access outside of the owning app container - see
            // https://github.com/wezterm/wezterm/issues/7963) or otherwise
            // fail to parse. Any such error here must NOT be allowed to
            // propagate out of `do_shape` via `?`: that would abort shaping
            // for the whole text run (and, if triggered repeatedly by
            // something like a CLI spinner animation re-triggering fallback
            // resolution on every tick, spam the render loop with hard
            // errors). Instead we log a warning and treat this candidate as
            // unusable, advancing to the next fallback font in the list,
            // exactly as we already do when a candidate's presentation
            // (text vs emoji) doesn't match.
            let loaded = match self.load_fallback(font_idx, dpi) {
                Ok(loaded) => loaded,
                Err(err) => {
                    log::warn!(
                        "Failed to load fallback font candidate at index {font_idx} \
                         ({:?}): {:#}; skipping to next fallback candidate",
                        self.handles.get(font_idx),
                        err
                    );
                    font_idx += 1;
                    continue;
                }
            };
            match loaded {
                Some(pair) => {
                    if let Some(p) = presentation {
                        if pair.presentation != p {
                            log::trace!(
                                "wanted presentation is {p:?} != font \
                                     presentation {:?} so skip \
                                     font_idx={font_idx}",
                                pair.presentation
                            );
                            font_idx += 1;
                            continue;
                        }
                    }
                    let point_size = font_size * self.handles[font_idx].scale.unwrap_or(1.);

                    let units_per_em = match self.ensure_rb_face(font_idx, &pair) {
                        Ok(upem) => upem,
                        Err(err) => {
                            log::warn!(
                                "Failed to parse fallback font candidate at index {font_idx} \
                                 ({:?}): {:#}; skipping to next fallback candidate",
                                self.handles.get(font_idx),
                                err
                            );
                            // Drop the half-initialized pair so we don't
                            // keep retrying a known-bad candidate on every
                            // future shape call for this font_idx.
                            drop(pair);
                            if let Some(opt_pair) = self.fonts.get(font_idx) {
                                opt_pair.borrow_mut().take();
                            }
                            font_idx += 1;
                            continue;
                        }
                    };

                    if *pair.last_size_and_dpi.borrow() != Some((point_size, dpi)) {
                        pair.last_size_and_dpi.borrow_mut().replace((point_size, dpi));
                    }

                    let pixel_size = point_size * dpi as f64 / 72.0;
                    scale = pixel_size / units_per_em;

                    shaped_any = pair.shaped_any;

                    let rb_face = pair.rb_face.borrow();
                    let face = &rb_face.as_ref().unwrap().face;
                    glyph_buffer = rustybuzz::shape(face, pair.features.as_slice(), buf);
                    log::trace!(
                        "rustybuzz shaped font_idx={} {:?} presentation={presentation:?}",
                        font_idx,
                        &s[range.start..range.end],
                    );
                    break;
                }
                None => {
                    for c in s.chars() {
                        no_glyphs.push(c);
                    }

                    if presentation.is_some() {
                        log::debug!(
                            "Ran out of fallback options, retry shape with no presentation"
                        );
                        return self.do_shape(
                            0,
                            s,
                            font_size,
                            dpi,
                            no_glyphs,
                            None,
                            direction,
                            range,
                            presentation_width,
                        );
                    }

                    no_more_fallbacks = true;
                    font_idx = 0;
                    continue;
                }
            }
        }

        let rb_infos = glyph_buffer.glyph_infos();
        let positions = glyph_buffer.glyph_positions();

        let mut cluster = Vec::with_capacity(s.len());
        let mut info_clusters: Vec<Vec<Info>> = Vec::with_capacity(s.len());

        // See the lengthy comment in shaper/harfbuzz.rs::do_shape for the
        // rationale behind this cluster-resolution dance; the logic here is
        // a straight port, operating on rustybuzz's GlyphInfo/GlyphPosition
        // instead of harfbuzz's.
        let mut cluster_resolver = ClusterResolver {
            presentation_width,
            ..Default::default()
        };

        cluster_resolver.build(rb_infos, s, &range);
        log::debug!("cluster_resolver: {cluster_resolver:#?}");

        // Round scaled advances to the nearest whole pixel to approximate
        // FreeType's grid-fit hinting (see the module doc comment for the
        // rationale). Offsets are left as fractional pixels, matching the
        // fact that FreeType only grid-fits the advance width/height, not
        // mark/attachment offsets.
        let scaled_advance = |raw: i32| -> f64 { (raw as f64 * scale).round() };
        let scaled_offset = |raw: i32| -> f64 { raw as f64 * scale };

        let info_iter = rb_infos.iter().zip(positions.iter()).peekable();
        for (info, pos) in info_iter {
            // rustybuzz reports `cluster` as a byte offset into whatever
            // was actually pushed into its buffer (`&s[range]`), i.e.
            // 0-based *relative to `range.start`* -- not an absolute
            // offset into the full `s`. `ClusterResolver` stores/looks up
            // absolute offsets (matching how it slices `s` directly), so
            // this must be converted before use.
            let cluster_info = match cluster_resolver.get_mut(info.cluster as usize + range.start) {
                Some(i) => i,
                None => panic!(
                    "expected cluster info.cluster {} to be in cluster_resolver",
                    info.cluster
                ),
            };
            let len = cluster_info.byte_len;

            let mut info = Info {
                cluster: cluster_info.start,
                len,
                codepoint: info.glyph_id,
                x_advance: scaled_advance(pos.x_advance),
                y_advance: scaled_advance(pos.y_advance),
                x_offset: scaled_offset(pos.x_offset),
                y_offset: scaled_offset(pos.y_offset),
            };
            log::debug!("rb info.cluster {} -> {info:?}", info.cluster);

            if info.codepoint == 0 && !no_more_fallbacks {
                cluster_info.incomplete = true;
            }

            if let Some(ref mut cluster) = info_clusters.last_mut() {
                if info.codepoint == 0 && !no_more_fallbacks {
                    let prior = cluster.last_mut().unwrap();
                    if prior.codepoint == 0 || prior.cluster == info.cluster {
                        if prior.cluster + prior.len == info.cluster {
                            prior.len += info.len;
                            continue;
                        } else if info.cluster + info.len == prior.cluster {
                            std::mem::swap(&mut info, prior);
                            prior.len += info.len;
                            continue;
                        } else if info.cluster + info.len == prior.cluster + prior.len {
                            continue;
                        }
                    }
                }

                if cluster.last().unwrap().cluster == info.cluster {
                    cluster.push(info);
                    continue;
                }
            }
            info_clusters.push(vec![info]);
        }
        log::debug!("font_idx={font_idx} info_clusters: {:#?}", info_clusters);

        let mut direct_clusters = 0;

        for infos in &info_clusters {
            let cluster_info = cluster_resolver
                .get(infos[0].cluster)
                .expect("assigned above");
            let sub_range =
                clamp_to_char_boundaries(s, cluster_info.start..cluster_info.start + cluster_info.byte_len);
            let substr = &s[sub_range.clone()];

            if cluster_info.incomplete && font_idx + 1 < self.handles.len() {
                // `infos` may hold several sub-entries whose individual
                // `cluster`/`len` came out of the merge dance above with
                // inconsistent, overlapping values (this happens when
                // several adjacent multi-codepoint graphemes -- eg: a run
                // of consonant+niqqud pairs -- are ALL unresolved in this
                // font and get folded into one group). Using just
                // `infos[0]`'s own range understated the true extent and
                // silently dropped whichever graphemes weren't covered by
                // it, so span min/max across every sub-entry instead.
                let recurse_start = infos.iter().map(|i| i.cluster).min().unwrap();
                let recurse_end = infos.iter().map(|i| i.cluster + i.len).max().unwrap();

                let mut shape = match self.do_shape(
                    font_idx + 1,
                    s,
                    font_size,
                    dpi,
                    no_glyphs,
                    presentation,
                    direction,
                    recurse_start..recurse_end,
                    presentation_width,
                ) {
                    Ok(shape) => Ok(shape),
                    Err(e) => {
                        error!("{:?} for {:?}", e, substr);
                        self.do_shape(
                            0,
                            &make_question_string(substr),
                            font_size,
                            dpi,
                            no_glyphs,
                            presentation,
                            direction,
                            sub_range,
                            presentation_width,
                        )
                    }
                }?;

                cluster.append(&mut shape);
                continue;
            }

            // Either this grapheme fully resolved in `font_idx`, or it
            // didn't and there's no further fallback font to try. In the
            // latter case `infos` can still contain a mix of resolved and
            // unresolved glyphs -- eg: a base consonant that shaped fine
            // in this font alongside a combining mark attached to the
            // same grapheme that didn't (this font has no glyph for it).
            // Give unresolved glyphs zero advance/width instead of
            // whatever default .notdef advance the font/shaper assigned
            // them, so a missing diacritic just silently disappears
            // instead of injecting an extra blank cell next to (and
            // discarding the shaping of) the base letter it belongs to.
            let total_width: f64 = infos
                .iter()
                .map(|info| if info.codepoint == 0 { 0. } else { info.x_advance })
                .sum();
            let mut remaining_cells = cluster_info.cell_width;

            for info in infos.iter() {
                if info.codepoint == 0 {
                    // `substr` spans the whole grapheme (eg: base letter +
                    // combining mark), not just this specific unresolved
                    // glyph -- our cluster resolution only tracks
                    // grapheme-level byte ranges, so this may also report
                    // chars that resolved fine elsewhere in `infos`. Still
                    // better than the previous behavior of dumping the
                    // entire remaining string on fallback exhaustion.
                    no_glyphs.extend(substr.chars());
                    let mut zeroed = info.clone();
                    zeroed.x_advance = 0.;
                    zeroed.y_advance = 0.;
                    let glyph = make_glyphinfo(substr, 0, font_idx, &zeroed);
                    cluster.push(glyph);
                    direct_clusters += 1;
                    continue;
                }

                let weighted_cell_width = if total_width == 0. {
                    1
                } else {
                    (cluster_info.cell_width as f64 * info.x_advance / total_width).ceil() as u8
                };
                let weighted_cell_width = weighted_cell_width.min(remaining_cells);
                remaining_cells = remaining_cells.saturating_sub(weighted_cell_width);

                let glyph = make_glyphinfo(substr, weighted_cell_width, font_idx, info);

                cluster.push(glyph);
                direct_clusters += 1;
            }
        }

        if !shaped_any {
            if let Some(opt_pair) = self.fonts.get(font_idx) {
                if direct_clusters == 0 {
                    log::trace!(
                        "Shaper didn't resolve glyphs from {:?}, so unload it",
                        self.handles[font_idx]
                    );
                    opt_pair.borrow_mut().take();
                } else if let Some(pair) = &mut *opt_pair.borrow_mut() {
                    pair.shaped_any = true;
                }
            }
        }

        Ok(cluster)
    }
}

impl FontShaper for RustybuzzShaper {
    fn shape(
        &self,
        text: &str,
        size: f64,
        dpi: u32,
        no_glyphs: &mut Vec<char>,
        presentation: Option<Presentation>,
        direction: Direction,
        range: Option<Range<usize>>,
        presentation_width: Option<&PresentationWidth>,
    ) -> anyhow::Result<Vec<GlyphInfo>> {
        let range = range.unwrap_or_else(|| 0..text.len());

        log::trace!(
            "rustybuzz shape {range:?} `{}` with presentation={presentation:?}",
            text.escape_debug()
        );
        let start = std::time::Instant::now();
        let result = self.do_shape(
            0,
            text,
            size,
            dpi,
            no_glyphs,
            presentation,
            direction,
            range,
            presentation_width,
        );
        metrics::histogram!("shape.rustybuzz").record(start.elapsed());
        result
    }

    fn metrics_for_idx(&self, font_idx: usize, size: f64, dpi: u32) -> anyhow::Result<FontMetrics> {
        let pair = self
            .load_fallback(font_idx, dpi)?
            .ok_or_else(|| anyhow!("metrics_for_idx: there is no font with idx={font_idx}!?"))?;

        let key = MetricsKey {
            font_idx,
            size: NotNan::new(size).unwrap(),
            dpi,
        };
        if let Some(metrics) = self.metrics.borrow().get(&key) {
            return Ok(*metrics);
        }

        let scale = self.handles[font_idx].scale.unwrap_or(1.);

        // `SwashFontInfo::selected_font_size` is the swash-based
        // equivalent of `ftwrap::Face::set_font_size`: rustybuzz/
        // ttf-parser doesn't have a "cell metrics" helper equivalent, and
        // reproducing FreeType's exact hinted cell metrics here matters for
        // grid alignment (this is the same reasoning as HarfbuzzShaper's
        // metrics_for_idx, and is unaffected by the shaping-engine swap).
        let selected_size = pair.font_info.selected_font_size(size * scale, dpi);
        let mut metrics = FontMetrics {
            cell_height: PixelLength::new(selected_size.height),
            cell_width: PixelLength::new(selected_size.width),
            descender: PixelLength::new(selected_size.descender),
            underline_thickness: PixelLength::new(selected_size.underline_thickness),
            underline_position: PixelLength::new(selected_size.underline_position),
            cap_height_ratio: selected_size.cap_height_to_height_ratio,
            cap_height: selected_size.cap_height.map(PixelLength::new),
            is_scaled: selected_size.is_scaled,
            presentation: pair.presentation,
            force_y_adjust: PixelLength::new(0.),
        };

        if scale != 1.0 && metrics.is_scaled {
            let diff = metrics.descender - (metrics.descender / scale);
            metrics.force_y_adjust = diff;
        }

        self.metrics.borrow_mut().insert(key, metrics);

        log::trace!(
            "rustybuzz metrics_for_idx={}, size={}, dpi={} -> {:?}",
            font_idx,
            size,
            dpi,
            metrics
        );

        Ok(metrics)
    }

    fn metrics(&self, size: f64, dpi: u32) -> anyhow::Result<FontMetrics> {
        let theoretical_height = size * dpi as f64 / 72.0;
        let mut metrics_idx = 0;
        log::trace!(
            "rustybuzz compute metrics across these handles for size={}, dpi={},
             theoretical pixel height {}: {:?}",
            size,
            dpi,
            theoretical_height,
            self.handles
        );
        while let Ok(Some(pair)) = self.load_fallback(metrics_idx, dpi) {
            let selected_size = pair
                .font_info
                .selected_font_size(size * self.handles[metrics_idx].scale.unwrap_or(1.), dpi);
            let diff = (theoretical_height - selected_size.height).abs();
            let factor = diff / theoretical_height;
            if factor < 2.0 {
                break;
            }

            if metrics_idx + 1 >= self.handles.len() {
                log::warn!(
                    "rustybuzz metrics: I wanted to skip idx {} because diff={} factor={} \
                    theoretical_height={} cell_height={}, but there are no more \
                    fallback fonts. Metrics will likely be crazy.",
                    metrics_idx,
                    diff,
                    factor,
                    theoretical_height,
                    selected_size.height
                );
                break;
            }

            metrics_idx += 1;
        }

        self.metrics_for_idx(metrics_idx, size, dpi)
    }
}

#[derive(Debug)]
struct ClusterInfo {
    start: usize,
    byte_len: usize,
    cell_width: u8,
    incomplete: bool,
}

#[derive(Default, Debug)]
struct ClusterResolver<'a> {
    map: HashMap<usize, ClusterInfo>,
    presentation_width: Option<&'a PresentationWidth<'a>>,
    start_by_cell_idx: HashMap<usize, usize>,
}

impl<'a> ClusterResolver<'a> {
    pub fn build(&mut self, rb_infos: &[rustybuzz::GlyphInfo], s: &str, range: &Range<usize>) {
        #[derive(PartialOrd, Ord, Eq, PartialEq, Copy, Clone)]
        struct Item {
            cell_idx: Option<usize>,
            start: usize,
        }

        let mut map = HashMap::new();

        for info in rb_infos.iter() {
            // See the comment at the `get_mut` call site in `do_shape`:
            // rustybuzz's `cluster` is relative to `range.start` (the
            // start of whatever substring was actually shaped), so it
            // must be converted to an absolute offset into `s` before
            // it's used to slice `s` or to look up `PresentationWidth`
            // (which indexes by absolute byte offset into the full
            // cluster text).
            let start = info.cluster as usize + range.start;

            let cell_idx = match self.presentation_width {
                Some(pw) => {
                    let cell_idx = pw.byte_to_cell_idx(start);

                    let entry = self.start_by_cell_idx.entry(cell_idx).or_insert(start);
                    *entry = (*entry).min(start);

                    Some(cell_idx)
                }
                None => None,
            };

            map.entry(start).or_insert_with(|| Item { start, cell_idx });
        }

        let mut cluster_starts: Vec<Item> = map.into_values().collect();
        // Must sort by byte position, not the derived `Ord` (which
        // compares `cell_idx` first since it's declared first): walking
        // this vector assumes consecutive entries are consecutive byte
        // ranges (`next_start - start` below). That coincided with
        // sorting by `cell_idx` as long as cell_idx only ever increased
        // with byte position, which no longer holds once a line can
        // contain a right-to-left phrase whose cells were reordered
        // in `cluster.text` relative to their original cell index.
        cluster_starts.sort_by_key(|item| item.start);

        cluster_starts.dedup_by(|a, b| match (a.cell_idx, b.cell_idx) {
            (Some(a), Some(b)) => a == b,
            _ => false,
        });

        let mut iter = cluster_starts.iter().peekable();
        while let Some(item) = iter.next().copied() {
            let start = item.start;
            let next_start = iter.peek().map(|&&s| s.start).unwrap_or(range.end);
            let byte_len = next_start - start;
            let cell_width = match self.presentation_width {
                Some(p) => p.num_cells(start..next_start),
                None => unicode_column_width(&s[start..next_start], None) as u8,
            };
            self.map.entry(start).or_insert_with(|| ClusterInfo {
                start,
                byte_len,
                cell_width,
                incomplete: false,
            });
        }
    }

    pub fn get_mut(&mut self, start: usize) -> Option<&mut ClusterInfo> {
        match self.presentation_width {
            Some(pw) => {
                let cell_idx = pw.byte_to_cell_idx(start);
                let actual_start = self.start_by_cell_idx.get(&cell_idx)?;
                self.map.get_mut(&actual_start)
            }
            None => self.map.get_mut(&start),
        }
    }

    pub fn get(&self, start: usize) -> Option<&ClusterInfo> {
        match self.presentation_width {
            Some(pw) => {
                let cell_idx = pw.byte_to_cell_idx(start);
                let actual_start = self.start_by_cell_idx.get(&cell_idx)?;
                self.map.get(&actual_start)
            }
            None => self.map.get(&start),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::locator::{FontDataHandle, FontDataSource};
    use crate::FontDatabase;
    use config::FontAttributes;

    fn hebrew_fallback_handle() -> ParsedFont {
        let db = FontDatabase::with_built_in().unwrap();
        db.resolve(
            &FontAttributes {
                family: "Cascadia Mono".into(),
                stretch: Default::default(),
                weight: Default::default(),
                is_fallback: false,
                is_synthetic: false,
                style: Default::default(),
                freetype_load_flags: None,
                freetype_load_target: None,
                freetype_render_target: None,
                harfbuzz_features: None,
                scale: None,
                assume_emoji_presentation: None,
            },
            14,
        )
        .unwrap()
        .clone()
    }

    /// Mirrors the real default font stack's shape: a primary font with no
    /// Hebrew coverage (Lucida Console isn't available in this Linux/CI
    /// build environment, so JetBrains Mono stands in for "primary font
    /// without Hebrew glyphs") followed by the bundled Hebrew fallback, so
    /// Hebrew codepoints only resolve after at least one no-glyphs/
    /// "incomplete" pass through a font that can't shape them.
    fn primary_then_hebrew_fallback_handles() -> Vec<ParsedFont> {
        vec![jetbrains_handle(), hebrew_fallback_handle()]
    }

    /// Same idea, but using the actual default primary font
    /// (`default_font_style` on Windows), which -- unlike JetBrains Mono --
    /// may have partial native Hebrew coverage (eg: base consonants but not
    /// niqqud combining marks), producing a different pattern of
    /// direct-vs-"incomplete" glyphs within the same rustybuzz cluster than
    /// a font with zero Hebrew coverage at all.
    #[cfg(windows)]
    fn lucida_then_hebrew_fallback_handles() -> Vec<ParsedFont> {
        let lucida = ParsedFont::from_locator(&FontDataHandle {
            source: FontDataSource::OnDisk(std::path::PathBuf::from(
                "C:\\Windows\\Fonts\\lucon.ttf",
            )),
            index: 0,
            variation: 0,
            origin: crate::locator::FontOrigin::FontDirs,
            coverage: None,
        })
        .expect("C:\\Windows\\Fonts\\lucon.ttf (Lucida Console) must be present on Windows CI");

        fn built_in(family: &str) -> ParsedFont {
            let db = FontDatabase::with_built_in().unwrap();
            db.resolve(
                &FontAttributes {
                    family: family.into(),
                    stretch: Default::default(),
                    weight: Default::default(),
                    is_fallback: true,
                    is_synthetic: false,
                    style: Default::default(),
                    freetype_load_flags: None,
                    freetype_load_target: None,
                    freetype_render_target: None,
                    harfbuzz_features: None,
                    scale: None,
                    assume_emoji_presentation: None,
                },
                14,
            )
            .unwrap()
            .clone()
        }

        // Exact real default order: primary, JetBrains fallback, Noto Color
        // Emoji, Cascadia Mono (Hebrew), Symbols Nerd Font Mono (see
        // `TextStyle::font_with_fallback`).
        vec![
            lucida,
            jetbrains_handle(),
            built_in("Noto Color Emoji"),
            hebrew_fallback_handle(),
            built_in("Symbols Nerd Font Mono"),
        ]
    }

    /// Regression test for a real bug: mixed Hebrew/Latin/punctuation text
    /// on one line (eg: "shalom, world" style output with an embedded
    /// dash/quote) rendered with duplicated punctuation and cells drawn in
    /// the wrong place. Root cause: `CellCluster::make_cluster_with_bidi`
    /// used `ReorderedRun::range` (a `min..max+1` numeric envelope) to walk
    /// a run's codepoints, but that envelope isn't guaranteed to contain
    /// *only* this run's codepoints when multiple runs are interleaved on
    /// the same line -- it can overlap with a neighboring run, visiting
    /// (and rendering) the same character twice. Fixed by using
    /// `ReorderedRun::indices` (the exact, deduplicated set of codepoints
    /// for this run) instead, sorted ascending to recover logical order.
    ///
    /// This asserts the fundamental invariant that must hold no matter how
    /// bidi resolution splits a line into clusters: every original cell is
    /// covered by exactly one resolved cluster, never zero and never two.
    /// Exact reproduction captured from a live warning log: the defensive
    /// `clamp_to_char_boundaries` guard fired for real, with
    /// `text=",םלועל "` (a `CellCluster::text`, part of a longer Hebrew
    /// phrase), reporting "adjusted 0..2 -> 0..3" -- meaning
    /// `ClusterResolver` computed a byte range that cut the Hebrew letter
    /// 'ם' (a 2-byte UTF-8 character occupying bytes 1..3) in half. This
    /// pins down the *exact* input/font-stack combination that triggers
    /// the underlying byte-range miscalculation, for use as a base to find
    /// the true root cause (this only proves the clamp saves us from a
    /// crash, not that the resulting glyphs/positions are actually
    /// correct).
    #[test]
    fn reproduces_the_captured_clamp_warning_input() {
        let _ = env_logger::Builder::new()
            .is_test(true)
            .filter_level(log::LevelFilter::Debug)
            .try_init();

        let config = config::configuration();
        let shaper =
            RustybuzzShaper::new(&config, &primary_then_hebrew_fallback_handles()).unwrap();

        let mut no_glyphs = vec![];
        shaper
            .shape(
                ",םלועל ",
                14.,
                72,
                &mut no_glyphs,
                None,
                Direction::RightToLeft,
                None,
                None,
            )
            .unwrap();
    }

    #[test]
    fn bidi_clusters_do_not_duplicate_or_drop_cells() {
        use termwiz::cell::CellAttributes;
        use termwiz::surface::Line;
        use wezterm_bidi::ParagraphDirectionHint;

        for text in [
            "שלום, עולם! Hello, world",
            "ברוך ה' — Благословен вовеки",
            "На иврите: אמן ואמן, לעולם — Благословен",
            "י ואת נ ודבלמ",
        ] {
            let line = Line::from_text(text, &CellAttributes::default(), 0, None);
            let total_cells = line.len();
            let clusters = line.cluster(Some(ParagraphDirectionHint::AutoLeftToRight));

            // A cluster's cells no longer need to be a contiguous
            // `first_cell_idx..first_cell_idx+width` range now that a
            // Hebrew phrase can be reordered within its cluster (only the
            // *set* of covered cells, via `byte_to_cell_idx`, needs to
            // partition the line exactly). `byte_to_cell_idx` is the
            // authoritative per-byte mapping actually used to position
            // glyphs at render time.
            let mut coverage = vec![0u32; total_cells];
            for cluster in &clusters {
                // Dedup within the cluster first: a niqqud/base pair is
                // two chars sharing one cell, which must count once, not
                // once per char.
                let mut cluster_cells: Vec<usize> = cluster
                    .text
                    .char_indices()
                    .map(|(byte_idx, _)| cluster.byte_to_cell_idx(byte_idx))
                    .collect();
                cluster_cells.sort_unstable();
                cluster_cells.dedup();
                for cell_idx in cluster_cells {
                    assert!(
                        cell_idx < total_cells,
                        "text={text:?}: cluster {cluster:?} covers out-of-range cell {cell_idx}"
                    );
                    coverage[cell_idx] += 1;
                }
            }
            for (cell_idx, count) in coverage.iter().enumerate() {
                assert_eq!(
                    *count, 1,
                    "text={text:?}: cell {cell_idx} covered {count} times (want exactly 1); clusters={clusters:#?}"
                );
            }
        }
    }

    /// Regression reproduction for a real crash: rendering Hebrew text with
    /// niqqud (vowel points, which combine into the same terminal cell as
    /// their base letter) through the real `Line` -> `CellCluster` -> shaper
    /// pipeline, with bidi enabled (as it now is by default), panicked with
    /// "byte index N is not a char boundary" inside `ClusterResolver`
    /// (`do_shape`, around the `let substr = &s[sub_range.clone()];` line).
    #[test]
    fn bidi_multi_word_hebrew_phrase_cluster_order() {
        // Diagnostic: for a multi-word, uniform-attrs Hebrew phrase, how
        // many clusters does `Line::cluster()` produce and in what order?
        // If it stays as ONE cluster, the shaper reorders inter-word RTL
        // layout correctly on its own. If it gets split into several
        // clusters (eg: by the whitespace force-break heuristic), the
        // clusters themselves need to be in VISUAL (reversed) order for
        // RTL, since crossing a cluster boundary means the shaper can't
        // reorder across it.
        use termwiz::cell::CellAttributes;
        use termwiz::surface::Line;
        use wezterm_bidi::ParagraphDirectionHint;

        let text = "שלום עליכם עליכם שלום";
        let line = Line::from_text(text, &CellAttributes::default(), 0, None);
        let clusters = line.cluster(Some(ParagraphDirectionHint::AutoLeftToRight));
        eprintln!("{} cluster(s) for {:?}", clusters.len(), text);
        for c in &clusters {
            eprintln!(
                "  text={:?} width={} first_cell_idx={} direction={:?}",
                c.text, c.width, c.first_cell_idx, c.direction
            );
        }
    }

    #[test]
    fn unresolved_mark_does_not_discard_its_base_letter() {
        // Regression test: hiriq (U+05B4) is not covered by Cascadia Mono,
        // and there's no secondary Hebrew fallback font behind it. Before
        // the fix, a grapheme where the base letter resolved but its
        // combining mark didn't (both share one rustybuzz "cluster" under
        // `MonotoneGraphemes`) was entirely discarded and re-shaped as two
        // separate notdef glyphs once fallback fonts were exhausted --
        // losing the base letter's real glyph and injecting an extra
        // full-width blank cell for the mark. Now the base letter's glyph
        // must survive and the unresolved mark must claim zero cells.
        let _ = env_logger::Builder::new()
            .is_test(true)
            .filter_level(log::LevelFilter::Warn)
            .try_init();

        let text = "\u{5d4}\u{5b4}\u{5d9}\u{5d0}"; // he + hiriq + yod + alef ("הִיא")
        let config = config::configuration();
        let shaper = RustybuzzShaper::new(&config, &primary_then_hebrew_fallback_handles()).unwrap();
        let mut no_glyphs = vec![];
        let info = shaper
            .shape(text, 14., 72, &mut no_glyphs, None, Direction::RightToLeft, None, None)
            .unwrap();

        let total_cells: usize = info.iter().map(|i| i.num_cells as usize).sum();
        assert_eq!(
            total_cells, 3,
            "he+yod+alef should claim 3 cells total (hiriq is unresolved and \
             must claim 0), got {total_cells}: {info:#?}"
        );

        let he_resolved = info
            .iter()
            .any(|i| i.font_idx == 1 && i.glyph_pos != 0 && i.num_cells == 1);
        assert!(
            he_resolved,
            "the base letter he (U+05D4) should keep its real, resolved \
             Cascadia Mono glyph even though the hiriq mark attached to \
             the same grapheme has no glyph in that font: {info:#?}"
        );
    }

    #[test]
    fn bidi_multi_word_hebrew_phrase_shapes_with_correct_cell_widths() {
        // Reproduction attempt using the REAL current default font stack
        // (JetBrains Mono primary -- has zero Hebrew coverage -- falling
        // back to the bundled Cascadia Mono) for a full multi-word
        // Hebrew phrase, checking that every shaped glyph's num_cells adds
        // up to exactly the cluster's width (no glyph should claim 0 cells
        // or more cells than are left, which would show up as glued-together
        // or overly-wide gaps on screen).
        let _ = env_logger::Builder::new()
            .is_test(true)
            .filter_level(log::LevelFilter::Warn)
            .try_init();

        use termwiz::cell::CellAttributes;
        use termwiz::surface::Line;
        use wezterm_bidi::ParagraphDirectionHint;

        let text = "שלום עליכם עליכם שלום";
        let line = Line::from_text(text, &CellAttributes::default(), 0, None);
        let clusters = line.cluster(Some(ParagraphDirectionHint::AutoLeftToRight));

        let config = config::configuration();
        let shaper =
            RustybuzzShaper::new(&config, &primary_then_hebrew_fallback_handles()).unwrap();

        for cluster in &clusters {
            let presentation_width = PresentationWidth::with_cluster(cluster);
            let mut no_glyphs = vec![];
            let info = shaper
                .shape(
                    &cluster.text,
                    14.,
                    72,
                    &mut no_glyphs,
                    Some(cluster.presentation),
                    cluster.direction,
                    None,
                    Some(&presentation_width),
                )
                .unwrap();
            let total_cells: usize = info.iter().map(|i| i.num_cells as usize).sum();
            eprintln!(
                "cluster width={} total_shaped_cells={} no_glyphs={:?}",
                cluster.width, total_cells, no_glyphs
            );
            for i in &info {
                eprintln!(
                    "  glyph_pos={} num_cells={} x_advance={:.2} cluster={} only_char={:?}",
                    i.glyph_pos, i.num_cells, i.x_advance.get(), i.cluster, i.only_char
                );
            }
            assert_eq!(
                total_cells, cluster.width,
                "shaped glyphs' num_cells sum ({total_cells}) doesn't match cluster width ({}) for {:?}",
                cluster.width, cluster.text
            );
        }
    }

    #[test]
    fn bidi_cluster_widths_per_char_attrs() {
        // Diagnostic (not a hard assertion yet): does giving each Hebrew
        // character DIFFERENT cell attributes -- as a chatty/streaming CLI
        // like Claude Code plausibly does per-token/per-color-span -- cause
        // `Line::cluster()` to split what should be one contiguous Hebrew
        // word into several tiny independent bidi "paragraphs", each
        // auto-detecting its own direction independently and losing the
        // surrounding context? This inspects cluster count/width/
        // first_cell_idx directly, without going through the shaper at all.
        use termwiz::cell::{Cell, CellAttributes};
        use termwiz::surface::Line;
        use wezterm_bidi::ParagraphDirectionHint;

        let text = "שלום";

        let uniform = Line::from_text(text, &CellAttributes::default(), 0, None);
        let uniform_clusters = uniform.cluster(Some(ParagraphDirectionHint::AutoLeftToRight));
        eprintln!("uniform attrs: {} cluster(s)", uniform_clusters.len());
        for c in &uniform_clusters {
            eprintln!(
                "  text={:?} width={} first_cell_idx={} direction={:?}",
                c.text, c.width, c.first_cell_idx, c.direction
            );
        }

        let mut varied = Line::new(0);
        for (idx, c) in text.chars().enumerate() {
            let mut attrs = CellAttributes::default();
            // Alternate foreground color per character, mimicking
            // per-character/per-token styling.
            attrs.set_foreground(termwiz::color::ColorAttribute::PaletteIndex(
                (idx % 2) as u8,
            ));
            varied.set_cell(idx, Cell::new(c, attrs), 0);
        }
        let varied_clusters = varied.cluster(Some(ParagraphDirectionHint::AutoLeftToRight));
        eprintln!("varied attrs: {} cluster(s)", varied_clusters.len());
        for c in &varied_clusters {
            eprintln!(
                "  text={:?} width={} first_cell_idx={} direction={:?}",
                c.text, c.width, c.first_cell_idx, c.direction
            );
        }
    }

    #[test]
    fn hebrew_phrase_reverses_in_place_without_touching_neighbors() {
        // Regression test for the simplified (non-UAX#9) rendering
        // model: a terminal ties cursor movement, selection and shell
        // line-editing to each character's typed/logical column, so
        // instead of running the full Bidi Algorithm (which
        // right-justifies RTL-based paragraphs and can sweep a stray
        // dash or number into the wrong end of the line), only the
        // Hebrew letters themselves get reversed relative to each other,
        // exactly where they were typed. Brackets/digits/Latin text
        // never move and are never mirrored, since they never change
        // position relative to the rest of the line.
        use termwiz::cell::CellAttributes;
        use termwiz::surface::Line;
        use wezterm_bidi::ParagraphDirectionHint;

        for (text, want) in [
            ("(שלום)", "(םולש)"),
            ("שלום עולם", "םלוע םולש"),
            // The geresh stays bonded to its letter (moves with it) but
            // the pair itself still reverses along with the rest of the
            // phrase, same as any other letter -- reading the resulting
            // "'א קרפ" span right-to-left recovers "פרק א'" exactly.
            ("פרק א' — Chapter", "'א קרפ — Chapter"),
        ] {
            let line = Line::from_text(text, &CellAttributes::default(), 0, None);
            let clusters = line.cluster(Some(ParagraphDirectionHint::AutoLeftToRight));
            let joined: String = clusters.iter().map(|c| c.text.as_str()).collect();
            assert_eq!(joined, want, "input {text:?}");
        }
    }

    #[test]
    fn punctuation_inside_hebrew_phrase_moves_with_the_phrase() {
        // A comma/question mark *between* two Hebrew words punctuates the
        // Hebrew, so it has to travel with it when the phrase is reversed
        // (this is Unicode rule UAX #9 N1: a neutral run surrounded by
        // right-to-left text becomes right-to-left too). Quotes/brackets
        // wrapping the whole phrase have non-Hebrew on their far side, so
        // they are *not* part of the phrase and must stay put -- which is
        // what keeps the line growing left-to-right from column 0 with
        // the Hebrew half still ahead of its Russian translation.
        //
        // Each case is written as (before, phrase, after) and the
        // expectation is built as `before + reverse(phrase) + after`:
        // reversing is by definition what "reads right-to-left" means, so
        // this states the intent without restating the algorithm.
        use termwiz::cell::CellAttributes;
        use termwiz::surface::Line;
        use wezterm_bidi::ParagraphDirectionHint;

        for (before, phrase, after) in [
            // The reported case: quoted Hebrew, then its quoted Russian
            // translation. The comma is inside the phrase and moves; the
            // quotes and the ` / ` separator do not.
            (
                "\"",
                "אם אין אני לי, מי לי",
                "\" / \"Если не я за себя, то кто за меня\"",
            ),
            ("«", "כל ישראל ערבים זה בזה", "» / «Весь Израиль в ответе»"),
            ("(", "איזהו עשיר", ") (кто богат?)"),
            // A closing ASCII apostrophe is a quote, not a geresh: it
            // must stay outside the phrase it closes rather than being
            // dragged to the far side of it.
            ("'", "דע לפני מי אתה עומד", "' / 'знай, перед кем ты стоишь'"),
        ] {
            let text = format!("{before}{phrase}{after}");
            let want = format!(
                "{before}{}{after}",
                phrase.chars().rev().collect::<String>()
            );
            let line = Line::from_text(&text, &CellAttributes::default(), 0, None);
            let clusters = line.cluster(Some(ParagraphDirectionHint::AutoLeftToRight));
            let joined: String = clusters.iter().map(|c| c.text.as_str()).collect();
            assert_eq!(joined, want, "input {text:?}");
        }
    }

    #[test]
    fn hebrew_phrase_touching_wrap_boundary_is_left_unreversed() {
        // Regression test: a physical row only ever sees its own cells,
        // so a Hebrew phrase touching the first/last cell might actually
        // be an incomplete fragment of a longer phrase that continues on
        // the row before/after it (the line wrapped there). Reversing an
        // incomplete fragment produces worse results (a bracket ending up
        // on the wrong side) than leaving it as typed, so
        // `cluster_with_wrap_context` must leave an edge-touching phrase
        // untouched when the wrap topology says it might be incomplete.
        use termwiz::cell::CellAttributes;
        use termwiz::surface::Line;
        use wezterm_bidi::ParagraphDirectionHint;

        let text = "שלום עולם";
        let line = Line::from_text(text, &CellAttributes::default(), 0, None);
        let hint = Some(ParagraphDirectionHint::AutoLeftToRight);

        // Baseline: with no wrap context, the whole phrase reverses.
        let normal: String = line
            .cluster(hint)
            .iter()
            .map(|c| c.text.as_str())
            .collect::<Vec<_>>()
            .join("");
        assert_eq!(normal, "םלוע םולש");

        // This row is the tail of a wrapped phrase (its first cell might
        // continue a run from the row above) -- since the phrase touches
        // cell 0, it must be left exactly as typed.
        let as_continuation: String = line
            .cluster_with_wrap_context(hint, true)
            .iter()
            .map(|c| c.text.as_str())
            .collect::<Vec<_>>()
            .join("");
        assert_eq!(as_continuation, text);
    }

    #[test]
    fn diag_quoted_hebrew_then_russian_char_by_char() {
        // Diagnostic: build the same line two ways -- via `Line::from_text`
        // (grapheme-aware, used by `render_line`/most tests) and via
        // per-character `set_cell` (mimicking how the real terminal builds
        // a line one printed character at a time from PTY bytes) -- and
        // compare the resulting cluster order, to check whether the two
        // construction paths actually produce the same `CellCluster`s for
        // a line reported to render differently in the two contexts.
        use termwiz::cell::{Cell, CellAttributes};
        use termwiz::surface::Line;
        use wezterm_bidi::ParagraphDirectionHint;

        let text = "\"אם אין אני לי, מי לי\" / \"Если не я за себя, то кто за меня\"";
        let hint = Some(ParagraphDirectionHint::AutoLeftToRight);

        let from_text = Line::from_text(text, &CellAttributes::default(), 0, None);
        let joined_from_text: String = from_text
            .cluster(hint)
            .iter()
            .map(|c| c.text.as_str())
            .collect::<Vec<_>>()
            .join("");

        let mut char_by_char = Line::new(0);
        for (idx, c) in text.chars().enumerate() {
            char_by_char.set_cell(idx, Cell::new(c, CellAttributes::default()), 0);
        }
        let joined_char_by_char: String = char_by_char
            .cluster(hint)
            .iter()
            .map(|c| c.text.as_str())
            .collect::<Vec<_>>()
            .join("");

        eprintln!("from_text:     {joined_from_text:?}");
        eprintln!("char_by_char:  {joined_char_by_char:?}");
        assert_eq!(joined_from_text, joined_char_by_char);
    }

    #[test]
    fn diag_mixed_lang_quote_boundary() {
        // Diagnostic: a Russian translation wrapped in guillemets, an em
        // dash, and the Hebrew original -- with the Russian+punctuation
        // portion given one set of attrs and the Hebrew portion another
        // (mimicking a chatty CLI's per-language color styling), the way
        // a real user reported broken quote mirroring/positioning.
        use termwiz::cell::{Cell, CellAttributes};
        use termwiz::surface::Line;
        use wezterm_bidi::ParagraphDirectionHint;

        let ru = "«Если не я за себя, то кто?» — ";
        let he = "אם אין אני לי";
        let mut line = Line::new(0);
        let mut idx = 0;
        let mut ru_attrs = CellAttributes::default();
        ru_attrs.set_foreground(termwiz::color::ColorAttribute::PaletteIndex(1));
        for c in ru.chars() {
            line.set_cell(idx, Cell::new(c, ru_attrs.clone()), 0);
            idx += 1;
        }
        let mut he_attrs = CellAttributes::default();
        he_attrs.set_foreground(termwiz::color::ColorAttribute::PaletteIndex(2));
        for c in he.chars() {
            line.set_cell(idx, Cell::new(c, he_attrs.clone()), 0);
            idx += 1;
        }

        let clusters = line.cluster(Some(ParagraphDirectionHint::AutoLeftToRight));
        eprintln!("{} cluster(s):", clusters.len());
        for c in &clusters {
            eprintln!(
                "  text={:?} width={} first_cell_idx={} direction={:?}",
                c.text, c.width, c.first_cell_idx, c.direction
            );
        }
    }

    #[test]
    fn shapes_hebrew_text_with_niqqud_under_bidi() {
        let _ = env_logger::Builder::new()
            .is_test(true)
            .filter_level(log::LevelFilter::Debug)
            .try_init();

        use termwiz::cell::CellAttributes;
        use termwiz::surface::Line;
        use wezterm_bidi::ParagraphDirectionHint;

        // "shalom" with niqqud: each vowel point combines into the same
        // grapheme cluster (and thus the same terminal cell) as the
        // preceding consonant.
        let combined = Line::from_text("שָׁלוֹם", &CellAttributes::default(), 0, None);

        // Same text, but with every niqqud mark placed in its OWN cell
        // instead of being grouped into the preceding consonant's grapheme
        // cluster -- simulating what happens if the base letter and its
        // combining mark get printed via separate `print()`/flush cycles
        // (eg: an SGR/color escape between them, as a chatty program like
        // Claude Code emits per-character/per-word highlighting) instead of
        // arriving as one already-composed string handed to
        // `Line::from_text`.
        let mut split = Line::new(0);
        for (idx, c) in "שָׁלוֹם".chars().enumerate() {
            split.set_cell(idx, termwiz::cell::Cell::new(c, CellAttributes::default()), 0);
        }

        // Neither JetBrains Mono nor Lucida Console has ANY Hebrew coverage
        // (confirmed separately), so a Latin prefix ahead of the Hebrew word
        // forces the Hebrew span to resolve via recursive fallback
        // (`do_shape(font_idx + 1, ...)`) starting at a NON-ZERO byte offset
        // -- exercising the "incomplete cluster" recursion path with
        // `range.start != 0`, which combined/split (pure Hebrew, always
        // starting at byte 0) never did.
        let prefixed = Line::from_text("echo שָׁלוֹם", &CellAttributes::default(), 0, None);

        for (label, line) in [("combined", &combined), ("split", &split), ("prefixed", &prefixed)] {
            let clusters = line.cluster(Some(ParagraphDirectionHint::AutoLeftToRight));

            let config = config::configuration();
            let shaper =
                RustybuzzShaper::new(&config, &primary_then_hebrew_fallback_handles()).unwrap();

            for cluster in &clusters {
                let presentation_width = PresentationWidth::with_cluster(cluster);
                let mut no_glyphs = vec![];
                shaper
                    .shape(
                        &cluster.text,
                        14.,
                        72,
                        &mut no_glyphs,
                        Some(cluster.presentation),
                        cluster.direction,
                        None,
                        Some(&presentation_width),
                    )
                    .unwrap_or_else(|e| panic!("label={label:?} cluster={cluster:?}: {e:?}"));
            }

            #[cfg(windows)]
            {
                let shaper =
                    RustybuzzShaper::new(&config, &lucida_then_hebrew_fallback_handles()).unwrap();
                for cluster in &clusters {
                    let presentation_width = PresentationWidth::with_cluster(cluster);
                    let mut no_glyphs = vec![];
                    shaper
                        .shape(
                            &cluster.text,
                            14.,
                            72,
                            &mut no_glyphs,
                            Some(cluster.presentation),
                            cluster.direction,
                            None,
                            Some(&presentation_width),
                        )
                        .unwrap_or_else(|e| {
                            panic!("[lucida] label={label:?} cluster={cluster:?}: {e:?}")
                        });
                }
            }
        }
    }

    fn jetbrains_handle() -> ParsedFont {
        let db = FontDatabase::with_built_in().unwrap();
        db.resolve(
            &FontAttributes {
                family: "JetBrains Mono".into(),
                stretch: Default::default(),
                weight: Default::default(),
                is_fallback: false,
                is_synthetic: false,
                style: Default::default(),
                freetype_load_flags: None,
                freetype_load_target: None,
                freetype_render_target: None,
                harfbuzz_features: None,
                scale: None,
                assume_emoji_presentation: None,
            },
            14,
        )
        .unwrap()
        .clone()
    }

    /// One shaped glyph's regression-relevant fields, used by
    /// `assert_shape_matches_baseline` below.
    #[derive(Debug, Clone, Copy, PartialEq)]
    struct GlyphBaseline {
        glyph_pos: u32,
        cluster: u32,
        x_advance: f64,
    }

    /// Shapes `text` with `RustybuzzShaper` (size=10, dpi=72, JetBrains
    /// Mono) and asserts the result matches a hardcoded baseline exactly
    /// for `glyph_pos`/`cluster`, and within `eps` pixels for `x_advance`
    /// (not bit-exact against the baseline capture, to tolerate
    /// float-rounding jitter across platforms/toolchains rather than
    /// requiring the environment that captured the baseline).
    ///
    /// This replaces a former harfbuzz-vs-rustybuzz parity comparison
    /// (see the module doc comment on the H0-established guarantee): now
    /// that the `harfbuzz` crate/`HarfbuzzShaper` have been removed
    /// (phase H4), there is no live oracle to compare against, so this
    /// instead pins down the current `RustybuzzShaper` output as a
    /// regression baseline (captured by actually running the shaper, not
    /// guessed) -- it will still catch a shaping regression from a
    /// rustybuzz/ttf-parser upgrade or a refactor of `do_shape`, just not
    /// a *divergence from harfbuzz* (which H0/H1 already established was
    /// zero for glyph_id/cluster, and small/tolerance-bounded for
    /// x_advance, before this crate was removed).
    fn assert_shape_matches_baseline(text: &str, eps: f64, expected: &[GlyphBaseline]) {
        let config = config::configuration();
        let handle = jetbrains_handle();
        let rb_shaper = RustybuzzShaper::new(&config, &[handle]).unwrap();

        let mut no_glyphs = vec![];
        let info = rb_shaper
            .shape(
                text,
                10.,
                72,
                &mut no_glyphs,
                None,
                Direction::LeftToRight,
                None,
                None,
            )
            .unwrap();
        assert!(no_glyphs.is_empty(), "{:?}", no_glyphs);

        assert_eq!(
            expected.len(),
            info.len(),
            "glyph count mismatch for {text:?}: expected={expected:#?} actual={info:#?}"
        );

        for (want, got) in expected.iter().zip(info.iter()) {
            assert_eq!(
                want.glyph_pos, got.glyph_pos,
                "glyph_id mismatch for {text:?}: want={want:?} got={got:?}"
            );
            assert_eq!(
                want.cluster, got.cluster,
                "cluster mismatch for {text:?}: want={want:?} got={got:?}"
            );
            assert!(
                (want.x_advance - got.x_advance.get()).abs() <= eps,
                "x_advance mismatch beyond eps={eps} for {text:?}: want={want:?} got={got:?}"
            );
        }
    }

    #[test]
    fn parity_simple_latin() {
        let _ = env_logger::Builder::new()
            .is_test(true)
            .filter_level(log::LevelFilter::Trace)
            .try_init();
        // Baselines captured from a real `RustybuzzShaper::shape` run
        // against JetBrainsMono-Regular.ttf at size=10, dpi=72 (see
        // `assert_shape_matches_baseline`'s doc comment for why these are
        // hardcoded rather than compared live against harfbuzz).
        assert_shape_matches_baseline(
            "abc",
            1.0,
            &[
                GlyphBaseline { glyph_pos: 189, cluster: 0, x_advance: 6.0 },
                GlyphBaseline { glyph_pos: 214, cluster: 1, x_advance: 6.0 },
                GlyphBaseline { glyph_pos: 215, cluster: 2, x_advance: 6.0 },
            ],
        );
        assert_shape_matches_baseline(
            "x x",
            1.0,
            &[
                GlyphBaseline { glyph_pos: 367, cluster: 0, x_advance: 6.0 },
                GlyphBaseline { glyph_pos: 958, cluster: 1, x_advance: 6.0 },
                GlyphBaseline { glyph_pos: 367, cluster: 2, x_advance: 6.0 },
            ],
        );
        assert_shape_matches_baseline(
            "x\u{3000}x",
            1.0,
            &[
                GlyphBaseline { glyph_pos: 367, cluster: 0, x_advance: 6.0 },
                GlyphBaseline { glyph_pos: 958, cluster: 1, x_advance: 10.0 },
                GlyphBaseline { glyph_pos: 367, cluster: 4, x_advance: 6.0 },
            ],
        );
    }

    #[test]
    fn parity_ligatures() {
        let _ = env_logger::Builder::new()
            .is_test(true)
            .filter_level(log::LevelFilter::Trace)
            .try_init();
        // JetBrains Mono applies contextual (`calt`) substitution to
        // `<-`/`<--` (each character gets a different glyph id than its
        // standalone form, e.g. `<`'s glyph_pos changes from 1052 to
        // 1742 once followed by `-`), exercising the same
        // feature-driven substitution path a former `HarfbuzzShaper`
        // comparison test covered (see `assert_shape_matches_baseline`'s
        // doc comment) -- note this does not collapse into a single
        // merged glyph per sequence at this size/config (each character
        // keeps its own glyph and cluster), so the baselines below have
        // one entry per input character, not one per ligated sequence.
        // Baselines captured from a real shaper run, same as
        // `parity_simple_latin`.
        assert_shape_matches_baseline(
            "<",
            1.0,
            &[GlyphBaseline { glyph_pos: 1052, cluster: 0, x_advance: 6.0 }],
        );
        assert_shape_matches_baseline(
            "<-",
            1.0,
            &[
                GlyphBaseline { glyph_pos: 1742, cluster: 0, x_advance: 6.0 },
                GlyphBaseline { glyph_pos: 1588, cluster: 1, x_advance: 6.0 },
            ],
        );
        assert_shape_matches_baseline(
            "<--",
            1.0,
            &[
                GlyphBaseline { glyph_pos: 1742, cluster: 0, x_advance: 6.0 },
                GlyphBaseline { glyph_pos: 1742, cluster: 1, x_advance: 6.0 },
                GlyphBaseline { glyph_pos: 1589, cluster: 2, x_advance: 6.0 },
            ],
        );
    }

    #[test]
    fn shape_basic() {
        let _ = env_logger::Builder::new()
            .is_test(true)
            .filter_level(log::LevelFilter::Trace)
            .try_init();

        let config = config::configuration();
        let shaper = RustybuzzShaper::new(&config, &[jetbrains_handle()]).unwrap();
        let mut no_glyphs = vec![];
        let info = shaper
            .shape(
                "abc",
                10.,
                72,
                &mut no_glyphs,
                None,
                Direction::LeftToRight,
                None,
                None,
            )
            .unwrap();
        assert!(no_glyphs.is_empty(), "{:?}", no_glyphs);
        assert_eq!(info.len(), 3);
        assert_eq!(info[0].only_char, Some('a'));
        assert_eq!(info[1].only_char, Some('b'));
        assert_eq!(info[2].only_char, Some('c'));
        assert_eq!(info[0].cluster, 0);
        assert_eq!(info[1].cluster, 1);
        assert_eq!(info[2].cluster, 2);
    }

    /// Regression coverage for
    /// <https://github.com/wezterm/wezterm/issues/7963>: a fallback font
    /// candidate whose backing file cannot be opened (originally reported
    /// as a Windows Store / MSIX font living under an ACL-protected
    /// `C:\Program Files\WindowsApps\...` path, denying access with "Access
    /// is denied. (os error 5)") must not abort shaping for the whole text
    /// run. The old `HarfbuzzShaper::load_fallback` (removed along with the
    /// rest of the harfbuzz shaper in the freetype/harfbuzz -> rustybuzz/
    /// swash migration) panicked in this situation; that panic could
    /// escalate to a fatal crash (STATUS_FATAL_APP_EXIT) if a caught panic
    /// unwind triggered a second panic, e.g. from a CLI spinner animation
    /// re-triggering fallback resolution on every tick.
    ///
    /// We don't attempt to reproduce real Windows ACL denial here (fragile
    /// and platform-specific); instead we point a fallback candidate's
    /// `FontDataSource::OnDisk` at a path that does not exist at all. From
    /// `RustybuzzShaper::load_fallback`'s point of view this produces the
    /// same shape of failure as an ACL-Denied open: `std::fs::read` (inside
    /// `FontDataSource::load_data`, called by
    /// `SwashFontInfo::from_locator`) returns an `Err`, and any IO error
    /// there must be handled identically regardless of its underlying
    /// `io::ErrorKind` (`NotFound`, `PermissionDenied`, etc.) -- the
    /// resolver has no business special-casing one IO error kind over
    /// another; all of them mean "this candidate is unusable, move on".
    ///
    /// The fallback list here has the broken candidate at index 0 and a
    /// real, working font (JetBrains Mono) at index 1. If the resolver
    /// still worked the old (buggy) way -- propagating the open/parse
    /// error out of `do_shape` via `?` -- this test would fail with
    /// `shape(..).unwrap()` panicking on the propagated `Err`. With the
    /// fix, `shape` logs a warning for the broken candidate and moves on
    /// to shape successfully against font_idx=1.
    #[test]
    fn fallback_skips_unreadable_candidate() {
        let _ = env_logger::Builder::new()
            .is_test(true)
            .filter_level(log::LevelFilter::Trace)
            .try_init();

        let config = config::configuration();

        let unreadable_handle = ParsedFont::from_locator(&FontDataHandle {
            source: FontDataSource::OnDisk(std::path::PathBuf::from(
                "/this/path/does/not/exist/wezterm-issue-7963-fallback-test.ttf",
            )),
            index: 0,
            variation: 0,
            origin: crate::locator::FontOrigin::FontDirs,
            coverage: None,
        });
        // `ParsedFont::from_locator` itself may already fail to build a
        // `ParsedFont` for a nonexistent path (it needs to peek at the file
        // to extract names/metrics) -- either way we want a `ParsedFont`
        // value to put in the handles list, because the real-world bug is
        // about a *resolved* fallback candidate (one that made it into the
        // handles list, e.g. because font enumeration read it from a
        // directory listing without opening it) whose file later can't be
        // opened when the shaper actually tries to load it. So if
        // constructing it from a bogus path fails up front, fall back to
        // building one from the real JetBrains Mono font and then
        // rewriting its `handle.source` to the bogus path -- this forges
        // exactly the "resolved candidate, unreadable file" scenario
        // `load_fallback` must tolerate.
        let mut broken = unreadable_handle.unwrap_or_else(|_| jetbrains_handle());
        broken.handle.source = FontDataSource::OnDisk(std::path::PathBuf::from(
            "/this/path/does/not/exist/wezterm-issue-7963-fallback-test.ttf",
        ));

        let working = jetbrains_handle();

        let shaper = RustybuzzShaper::new(&config, &[broken, working]).unwrap();

        let mut no_glyphs = vec![];
        let info = shaper
            .shape(
                "abc",
                10.,
                72,
                &mut no_glyphs,
                None,
                Direction::LeftToRight,
                None,
                None,
            )
            .expect(
                "shape() must gracefully skip an unreadable fallback candidate \
                 instead of propagating its IO/parse error (see #7963)",
            );

        assert!(no_glyphs.is_empty(), "{:?}", no_glyphs);
        assert_eq!(info.len(), 3);
        assert_eq!(info[0].only_char, Some('a'));
        assert_eq!(info[1].only_char, Some('b'));
        assert_eq!(info[2].only_char, Some('c'));
        for glyph in &info {
            assert_eq!(
                glyph.font_idx, 1,
                "expected glyphs to be shaped from the working fallback \
                 candidate (font_idx=1), not the unreadable one: {:?}",
                info
            );
        }
    }
}
