//! `RustybuzzShaper`: an implementation of the `FontShaper` trait built on
//! the pure-Rust `rustybuzz` crate instead of the C/C++ `harfbuzz` library.
//!
//! This is part of the freetype+harfbuzz -> rustybuzz+swash migration
//! (see docs/plans/2026-07-23-freetype-harfbuzz-migration.md, phase H1).
//!
//! H0 (formerly `onlyterm-font/examples/dump_shaping.rs`, a comparison tool
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
//!   advance_px = raw_units * (point_size * dpi / 72) / units_per_em
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
use crate::locator::SharedFontData;
use crate::parser::ParsedFont;
use crate::shaper::{FallbackIdx, FontMetrics, FontShaper, GlyphInfo, PresentationWidth};
use crate::units::*;
use anyhow::{anyhow, Context};
use config::ConfigHandle;
use finl_unicode::grapheme_clusters::Graphemes;
use log::error;
use onlyterm_bidi::Direction;
use ordered_float::NotNan;
use std::cell::{RefCell, RefMut};
use std::collections::HashMap;
use std::ops::Range;
use termwiz::cell::{unicode_column_width, Presentation};

/// Keeps shared immutable raw storage alive for a `rustybuzz::Face<'static>`.
///
/// `rustybuzz::Face<'a>` borrows its input buffer, so we need something to
/// keep the bytes alive for exactly as long as the `Face` that references
/// them. We hand `rustybuzz` a pointer with an unsafely-extended `'static`
/// lifetime; the face and its shared backing are stored side by side, so the
/// borrow is valid for as long as anyone can observe it.
struct OwnedRbFace {
    // Order matters for drop safety in spirit (though neither type's Drop
    // impl actually touches the other's memory): the face borrows from
    // `_data`, so we keep them paired in a single struct rather than ever
    // handing the `Face` out on its own.
    face: rustybuzz::Face<'static>,
    _data: SharedFontData,
}

impl OwnedRbFace {
    fn from_bytes(data: SharedFontData, face_index: u32) -> anyhow::Result<Self> {
        let slice: &[u8] = &data;
        // Safety: `data` owns immutable storage whose address is stable; we
        // keep `_data` alive alongside `face` for the lifetime of
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
    presentation: Presentation,
    features: Vec<rustybuzz::Feature>,
    variations: RefCell<Option<Vec<rustybuzz::Variation>>>,
    last_size_and_dpi: RefCell<Option<(f64, u32)>>,
    units_per_em: RefCell<Option<f64>>,
    /// `rustybuzz::ShapePlan`s, keyed by (direction, script) - the two
    /// buffer properties that vary across `do_shape` calls for a given
    /// font/feature set (`language` is a fixed constant for the whole
    /// shaper, see `RustybuzzShaper::lang`; `features` is fixed for the
    /// lifetime of this `FontPair`, set once in `load_fallback`).
    ///
    /// `rustybuzz::shape()` (the convenience wrapper we used to call)
    /// builds a fresh `ShapePlan` on every single call, which rustybuzz's
    /// own doc comment on `shape()` calls out explicitly: "ShapePlan
    /// initialization is pretty slow and should preferably be called once
    /// for each Face" - it re-parses the face's GSUB/GPOS layout tables.
    /// Measured via CPU-weighted profiling of a live debug session: plan
    /// construction alone was ~5% of all main-thread CPU time, over a
    /// third of total time spent shaping. We now build a plan once per
    /// distinct (direction, script) this font is asked to shape and reuse
    /// it via `shape_with_plan`.
    shape_plans: RefCell<HashMap<(rustybuzz::Direction, rustybuzz::Script), rustybuzz::ShapePlan>>,
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
        log::warn!(
            "clamp_to_char_boundaries adjusted {:?} -> {:?} in {:?}",
            range,
            start..end,
            s
        );
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
                        presentation: if handle.assume_emoji_presentation {
                            Presentation::Emoji
                        } else {
                            Presentation::Text
                        },
                        features,
                        variations: RefCell::new(None),
                        last_size_and_dpi: RefCell::new(None),
                        units_per_em: RefCell::new(None),
                        shape_plans: RefCell::new(HashMap::new()),
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
        let data = handle.handle.source.shared_data().with_context(|| {
            format!(
                "loading raw font bytes for rustybuzz face {:?}",
                handle.handle
            )
        })?;
        // `handle.handle.index` is a plain collection index (see
        // `parse_and_collect_font_info`/`ParsedFont::from_face`, which
        // keep the named-instance selector entirely in the separate
        // `variation` field rather than bit-packing it into `index` the
        // way FreeType's own face-index convention used to require).
        let face_index = handle.handle.index;

        let mut owned = OwnedRbFace::from_bytes(data.clone(), face_index)?;

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

    // do_shape threads every value the shaping loop needs independently;
    // collapsing into a parameter struct would couple the bidi range/
    // presentation handling recently adjusted for Hebrew rendering, so a
    // targeted allow is the safer trade here.
    #[allow(clippy::too_many_arguments)]
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

        // Capture the buffer's (direction is fixed above; script is
        // inferred by `guess_segment_properties()` from the text just
        // pushed) properties now, before `buf` is consumed by
        // `shape_with_plan` below - see `FontPair::shape_plans`'s doc
        // comment for why we look up/build a plan keyed on these instead
        // of calling `rustybuzz::shape()` directly.
        let plan_direction = buf.direction();
        let plan_script = buf.script();

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
                        pair.last_size_and_dpi
                            .borrow_mut()
                            .replace((point_size, dpi));
                    }

                    let pixel_size = point_size * dpi as f64 / 72.0;
                    scale = pixel_size / units_per_em;

                    let rb_face = pair.rb_face.borrow();
                    let face = &rb_face.as_ref().unwrap().face;

                    let plan_key = (plan_direction, plan_script);
                    if !pair.shape_plans.borrow().contains_key(&plan_key) {
                        let plan = rustybuzz::ShapePlan::new(
                            face,
                            plan_direction,
                            Some(plan_script),
                            Some(&self.lang),
                            pair.features.as_slice(),
                        );
                        pair.shape_plans.borrow_mut().insert(plan_key, plan);
                    }
                    let shape_plans = pair.shape_plans.borrow();
                    let plan = shape_plans.get(&plan_key).unwrap();
                    glyph_buffer = rustybuzz::shape_with_plan(face, plan, buf);
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

        for infos in &info_clusters {
            let cluster_info = cluster_resolver
                .get(infos[0].cluster)
                .expect("assigned above");
            let sub_range = clamp_to_char_boundaries(
                s,
                cluster_info.start..cluster_info.start + cluster_info.byte_len,
            );
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
                .map(|info| {
                    if info.codepoint == 0 {
                        0.
                    } else {
                        info.x_advance
                    }
                })
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
            }
        }

        // Coverage misses recur on every fallback-only run. Keep parsed
        // fonts and plans until this shaper (and its fixed font set) is dropped.
        Ok(cluster)
    }
}

impl FontShaper for RustybuzzShaper {
    fn append_handles(&mut self, handles: &[ParsedFont]) -> anyhow::Result<()> {
        self.handles.extend_from_slice(handles);
        self.fonts
            .extend(handles.iter().map(|_| RefCell::new(None)));
        Ok(())
    }

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
        let range = range.unwrap_or(0..text.len());

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
                self.map.get_mut(actual_start)
            }
            None => self.map.get_mut(&start),
        }
    }

    pub fn get(&self, start: usize) -> Option<&ClusterInfo> {
        match self.presentation_width {
            Some(pw) => {
                let cell_idx = pw.byte_to_cell_idx(start);
                let actual_start = self.start_by_cell_idx.get(&cell_idx)?;
                self.map.get(actual_start)
            }
            None => self.map.get(&start),
        }
    }
}

#[cfg(test)]
mod test;
