//! `RustybuzzShaper`: an implementation of the `FontShaper` trait built on
//! the pure-Rust `rustybuzz` crate instead of the C/C++ `harfbuzz` library.
//!
//! This is part of the freetype+harfbuzz -> rustybuzz+swash migration
//! (see docs/plans/2026-07-23-freetype-harfbuzz-migration.md, phase H1).
//!
//! H0 (`wezterm-font/examples/dump_shaping.rs`) already established that:
//! - `glyph_id`/`cluster` sequences from raw `rustybuzz::shape()` match the
//!   production `HarfbuzzShaper` 1:1 on latin text, ligatures (FiraCode-style)
//!   and emoji ZWJ sequences.
//! - `x_advance`/`y_advance`/`x_offset`/`y_offset` differ, because the
//!   production harfbuzz path (`USE_OT_FUNCS=false` in `shaper/harfbuzz.rs`)
//!   delegates glyph metrics to FreeType via `hb_ft_font_create_referenced`
//!   + `hb_ft_font_set_load_flags`, which means every advance/offset that
//!   comes back from `hb_shape` has already been hinted (grid-fit) by
//!   FreeType's TrueType bytecode interpreter/autohinter and expressed in
//!   26.6 fixed-point pixels (`make_glyphinfo` in `shaper/harfbuzz.rs` simply
//!   divides by 64.0 to get pixels).
//!
//!   rustybuzz has no equivalent of `hb_font_set_scale`/`hb_ft_font_*`: it
//!   always reports glyph positions in the font's raw design units
//!   (`unitsPerEm` space) with no hinting/grid-fitting applied at all. To be
//!   directly comparable we must scale ourselves:
//!       advance_px = raw_units * (point_size * dpi / 72) / units_per_em
//!   (this is the same formula `ftwrap::Face::set_font_size` uses to compute
//!   the nominal pixel height, and the same one dump_shaping.rs's
//!   `shape_with_rustybuzz` already applies).
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
use crate::ftwrap;
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

/// Converts a big-endian-packed FreeType 4cc tag (as used by
/// `FT_Var_Axis::tag`) into the 4 raw bytes that `ttf_parser::Tag`/
/// `rustybuzz::Tag` expect.
fn ft_tag_to_bytes(tag: u32) -> [u8; 4] {
    [
        ((tag >> 24) & 0xff) as u8,
        ((tag >> 16) & 0xff) as u8,
        ((tag >> 8) & 0xff) as u8,
        (tag & 0xff) as u8,
    ]
}

/// If `handle` selects a named instance of a variable font (as FreeType
/// resolves it via `FT_Set_Named_Instance`, one per `ParsedFont` produced by
/// `Face::variations()`), extract that instance's design-space coordinates
/// via `FT_Get_MM_Var` so that we can apply the equivalent variation to the
/// rustybuzz/ttf-parser face (which, unlike FreeType, has no notion of
/// "face index selects a named instance" - it only understands explicit
/// axis tag/value pairs via `set_variation`).
fn variation_coords_for(face: &ftwrap::Face) -> Vec<rustybuzz::Variation> {
    let mut coords = vec![];
    unsafe {
        let ft_face = face.face;
        let index = (*ft_face).face_index;
        let variation = index >> 16;
        if variation <= 0 {
            return coords;
        }
        let vidx = (variation - 1) as usize;

        let mut mm = std::ptr::null_mut();
        if !ftwrap::succeeded(ftwrap::FT_Get_MM_Var(ft_face, &mut mm)) {
            return coords;
        }

        {
            let mm = &*mm;
            let num_axis = mm.num_axis as usize;
            if (vidx as u32) < mm.num_namedstyles {
                let styles = std::slice::from_raw_parts(mm.namedstyle, mm.num_namedstyles as usize);
                let instance = &styles[vidx];
                let axes = std::slice::from_raw_parts(mm.axis, num_axis);
                let instance_coords = std::slice::from_raw_parts(instance.coords, num_axis);

                for (axis, &value) in axes.iter().zip(instance_coords.iter()) {
                    let tag = rustybuzz::ttf_parser::Tag::from_bytes(&ft_tag_to_bytes(axis.tag as u32));
                    coords.push(rustybuzz::Variation {
                        tag,
                        value: value.to_num::<f64>() as f32,
                    });
                }
            }
        }

        ftwrap::FT_Done_MM_Var(face.library(), mm);
    }
    coords
}

struct FontPair {
    face: ftwrap::Face,
    rb_face: RefCell<Option<OwnedRbFace>>,
    shaped_any: bool,
    presentation: Presentation,
    features: Vec<rustybuzz::Feature>,
    variations: Vec<rustybuzz::Variation>,
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
    lib: ftwrap::Library,
    metrics: RefCell<HashMap<MetricsKey, FontMetrics>>,
    features: Vec<rustybuzz::Feature>,
    lang: rustybuzz::Language,
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
        let lib = ftwrap::Library::new()?;
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
            lib,
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
                    let face = self.lib.face_from_locator(&handle.handle)?;

                    let variations = variation_coords_for(&face);

                    let features = match &handle.harfbuzz_features {
                        Some(features) => features
                            .iter()
                            .filter_map(|s| rb_feature_from_string(s).ok())
                            .collect(),
                        None => self.features.clone(),
                    };

                    *opt_pair = Some(FontPair {
                        face,
                        rb_face: RefCell::new(None),
                        shaped_any: false,
                        presentation: if handle.assume_emoji_presentation {
                            Presentation::Emoji
                        } else {
                            Presentation::Text
                        },
                        features,
                        variations,
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
        let mut ft_index = handle.handle.index;
        if handle.handle.variation != 0 {
            // Match FreeType's own face_index convention
            // (see ftwrap::Library::face_from_locator) so that, at minimum,
            // we select the right sub-face out of a TTC. The named-instance
            // bits aren't understood by ttf-parser, but we compensate for
            // that separately via `variations` (explicit axis coordinates
            // applied with `set_variation` below).
            ft_index |= handle.handle.variation << 16;
        }
        // ttf-parser's face_index is a plain collection index; mask off any
        // named-instance bits so we don't hand it a nonsensical value.
        let face_index = ft_index & 0xffff;

        let mut owned = OwnedRbFace::from_bytes(data.into_owned().into_boxed_slice(), face_index)?;
        for variation in &pair.variations {
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
            match self.load_fallback(font_idx, dpi).context("load_fallback")? {
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

                    let units_per_em = self.ensure_rb_face(font_idx, &pair)?;

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
            let cluster_info = match cluster_resolver.get_mut(info.cluster as usize) {
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
            let sub_range = cluster_info.start..cluster_info.start + cluster_info.byte_len;
            let substr = &s[sub_range.clone()];

            if cluster_info.incomplete {
                let first_info = &infos[0];

                let mut shape = match self.do_shape(
                    font_idx + 1,
                    s,
                    font_size,
                    dpi,
                    no_glyphs,
                    presentation,
                    direction,
                    first_info.cluster..first_info.cluster + first_info.len,
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

            let total_width: f64 = infos.iter().map(|info| info.x_advance).sum();
            let mut remaining_cells = cluster_info.cell_width;

            for info in infos.iter() {
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
        let mut pair = self
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

        // We reuse ftwrap::Face::set_font_size to compute metrics: rustybuzz/
        // ttf-parser doesn't have a "cell metrics" helper equivalent, and
        // reproducing FreeType's exact hinted cell metrics here matters for
        // grid alignment (this is the same reasoning as HarfbuzzShaper's
        // metrics_for_idx, and is unaffected by the shaping-engine swap).
        let selected_size = pair.face.set_font_size(size * scale, dpi)?;
        let y_scale = unsafe { (*(*pair.face.face).size).metrics.y_scale.to_num::<f64>() };
        let mut metrics = FontMetrics {
            cell_height: PixelLength::new(selected_size.height),
            cell_width: PixelLength::new(selected_size.width),
            descender: PixelLength::new(unsafe {
                (*(*pair.face.face).size).metrics.descender.f26d6().to_num()
            }),
            underline_thickness: PixelLength::new(
                unsafe { (*pair.face.face).underline_thickness as f64 } * y_scale / 64.,
            ),
            underline_position: PixelLength::new(
                unsafe { (*pair.face.face).underline_position as f64 } * y_scale / 64.,
            ),
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
        while let Ok(Some(mut pair)) = self.load_fallback(metrics_idx, dpi) {
            let selected_size = pair
                .face
                .set_font_size(size * self.handles[metrics_idx].scale.unwrap_or(1.), dpi)?;
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
            let start = info.cluster as usize;

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
        cluster_starts.sort();

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
    use crate::shaper::harfbuzz::HarfbuzzShaper;
    use crate::FontDatabase;
    use config::FontAttributes;

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

    /// Assert that RustybuzzShaper and HarfbuzzShaper agree on glyph_id and
    /// cluster for every glyph produced for `text` (the H0-established
    /// guarantee), and that x_advance is within `eps` pixels for each glyph
    /// (not bit-exact, since rustybuzz has no FreeType-hinting equivalent -
    /// see the module doc comment).
    fn assert_shape_parity(text: &str, eps: f64) {
        let config = config::configuration();
        let handle = jetbrains_handle();

        let hb_shaper = HarfbuzzShaper::new(&config, &[handle.clone()]).unwrap();
        let rb_shaper = RustybuzzShaper::new(&config, &[handle]).unwrap();

        let mut hb_no_glyphs = vec![];
        let hb_info = hb_shaper
            .shape(
                text,
                10.,
                72,
                &mut hb_no_glyphs,
                None,
                Direction::LeftToRight,
                None,
                None,
            )
            .unwrap();
        assert!(hb_no_glyphs.is_empty(), "{:?}", hb_no_glyphs);

        let mut rb_no_glyphs = vec![];
        let rb_info = rb_shaper
            .shape(
                text,
                10.,
                72,
                &mut rb_no_glyphs,
                None,
                Direction::LeftToRight,
                None,
                None,
            )
            .unwrap();
        assert!(rb_no_glyphs.is_empty(), "{:?}", rb_no_glyphs);

        assert_eq!(
            hb_info.len(),
            rb_info.len(),
            "glyph count mismatch for {text:?}: harfbuzz={hb_info:#?} rustybuzz={rb_info:#?}"
        );

        for (hb, rb) in hb_info.iter().zip(rb_info.iter()) {
            assert_eq!(
                hb.glyph_pos, rb.glyph_pos,
                "glyph_id mismatch for {text:?}: hb={hb:?} rb={rb:?}"
            );
            assert_eq!(
                hb.cluster, rb.cluster,
                "cluster mismatch for {text:?}: hb={hb:?} rb={rb:?}"
            );
            assert!(
                (hb.x_advance.get() - rb.x_advance.get()).abs() <= eps,
                "x_advance mismatch beyond eps={eps} for {text:?}: hb={hb:?} rb={rb:?}"
            );
        }
    }

    #[test]
    fn parity_simple_latin() {
        let _ = env_logger::Builder::new()
            .is_test(true)
            .filter_level(log::LevelFilter::Trace)
            .try_init();
        assert_shape_parity("abc", 1.0);
        assert_shape_parity("x x", 1.0);
        assert_shape_parity("x\u{3000}x", 1.0);
    }

    #[test]
    fn parity_ligatures() {
        let _ = env_logger::Builder::new()
            .is_test(true)
            .filter_level(log::LevelFilter::Trace)
            .try_init();
        // JetBrains Mono ligates `<-`/`<--` etc, exercising the same
        // ligature-clustering path that HarfbuzzShaper's `ligatures` test
        // covers.
        assert_shape_parity("<", 1.0);
        assert_shape_parity("<-", 1.0);
        assert_shape_parity("<--", 1.0);
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
}
