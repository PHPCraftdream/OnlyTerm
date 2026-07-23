#![allow(dead_code)]
//! `Painter`: a small cairo-shaped drawing API implemented on top of
//! `tiny-skia`.
//!
//! This module is scaffolding for the cairo -> tiny-skia migration
//! described in `docs/plans/2026-07-23-decairo-tiny-skia.md` (phase B).
//! It is intentionally shaped to closely mirror the subset of the
//! `cairo::Context` API that is used by
//! `wezterm-font/src/rasterizer/{colr,freetype,harfbuzz}.rs`:
//!
//! - `save`/`restore` push/pop a `(Transform, Option<Mask>)` pair, just
//!   like cairo's graphics-state stack (transform + clip).
//! - `transform`/`translate`/`scale` mutate the current transform via
//!   `tiny_skia::Transform::pre_concat`, matching cairo's semantics of
//!   pre-multiplying the CTM by the new matrix.
//! - `move_to`/`line_to`/`curve_to`/`quad_to`/`close_path`/`new_path`
//!   build up a `tiny_skia::Path` through a `PathBuilder`, mirroring
//!   cairo's current-path concept.
//! - `clip()` intersects the current clip mask with the current path
//!   (rendered under the current transform), like `cairo::Context::clip`.
//! - `push_group`/`pop_group` render into an offscreen `Pixmap` the same
//!   size as the root surface and composite it back with a `BlendMode`,
//!   mirroring `push_group`/`pop_group_to_source` + `set_operator`.
//! - `set_source_rgba`/`set_source`+`paint()` fill the current clip
//!   region with a solid color or a `Shader` (gradient/pattern), mirroring
//!   `set_source_rgba`/`set_source` + `paint()`.
//!
//! Because tiny-skia has no equivalent of cairo's `RecordingSurface` +
//! `ink_extents()`, callers drive a two-pass rendering protocol:
//!
//! 1. Construct a `Painter` in dry-run mode (`Painter::new_dry_run`) and
//!    replay all drawing operations against it. Read back `bbox()` to get
//!    the conservative ink extents (see "Тонкое место 1" in the plan:
//!    for solid/gradient paints the contribution is the bbox of the
//!    current clip stack under the current transform; for clip paths the
//!    bbox is that of the path's control points, which is a conservative
//!    superset of the true ink extents because Bezier curves lie within
//!    the convex hull of their control points).
//! 2. Create a `Pixmap` sized to that bbox, construct a real `Painter`
//!    over it via `Painter::new`, translate so that the bbox origin maps
//!    to (0, 0), and replay the exact same operations for real.
//!
//! The final `Pixmap` contents are premultiplied RGBA - the same format
//! that `RasterizedGlyph::data` expects (see the module doc on
//! `RasterizedGlyph` and the `argb_to_rgba` conversion currently used by
//! the cairo-based rasterizers, which un-swizzles cairo's premultiplied
//! `ARgb32` into premultiplied RGBA without changing premultiplication).

use tiny_skia::{
    BlendMode, Color, FillRule, Mask, Paint, Path, PathBuilder, Pixmap, PixmapPaint, Rect, Shader,
    Transform,
};

/// A conservative axis-aligned bounding box accumulated in "user space"
/// (i.e. already transformed into the coordinate system of the root
/// surface, since bboxes coming from different nested transforms are
/// only comparable once projected into a common space).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BBox {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

impl BBox {
    pub fn empty() -> Self {
        Self {
            x0: f32::INFINITY,
            y0: f32::INFINITY,
            x1: f32::NEG_INFINITY,
            y1: f32::NEG_INFINITY,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.x1 <= self.x0 || self.y1 <= self.y0
    }

    pub fn from_points<I: IntoIterator<Item = (f32, f32)>>(points: I) -> Self {
        let mut bbox = Self::empty();
        for (x, y) in points {
            bbox.add_point(x, y);
        }
        bbox
    }

    pub fn add_point(&mut self, x: f32, y: f32) {
        self.x0 = self.x0.min(x);
        self.y0 = self.y0.min(y);
        self.x1 = self.x1.max(x);
        self.y1 = self.y1.max(y);
    }

    pub fn union(&self, other: &BBox) -> BBox {
        if self.is_empty() {
            return *other;
        }
        if other.is_empty() {
            return *self;
        }
        BBox {
            x0: self.x0.min(other.x0),
            y0: self.y0.min(other.y0),
            x1: self.x1.max(other.x1),
            y1: self.y1.max(other.y1),
        }
    }

    /// Applies `transform` to all 4 corners of this bbox and returns the
    /// (still axis-aligned) bbox of the result. This is the same
    /// conservative approach cairo effectively uses for user-space
    /// extents of an axis-aligned region under an arbitrary transform.
    pub fn transformed(&self, transform: Transform) -> BBox {
        if self.is_empty() {
            return *self;
        }
        let mut corners: [tiny_skia::Point; 4] = [
            (self.x0, self.y0).into(),
            (self.x1, self.y0).into(),
            (self.x1, self.y1).into(),
            (self.x0, self.y1).into(),
        ];
        transform.map_points(&mut corners);
        let mut out = BBox::empty();
        for p in corners {
            out.add_point(p.x, p.y);
        }
        out
    }

    pub fn width(&self) -> f32 {
        if self.is_empty() {
            0.
        } else {
            self.x1 - self.x0
        }
    }

    pub fn height(&self) -> f32 {
        if self.is_empty() {
            0.
        } else {
            self.y1 - self.y0
        }
    }
}

/// One entry of the save/restore stack: the graphics state that cairo's
/// `save`/`restore` preserves for our purposes (CTM + clip mask).
#[derive(Clone)]
struct GraphicsState {
    transform: Transform,
    clip: Option<Mask>,
    clip_bbox: Option<BBox>,
}

/// Which "mode" a `Painter` operates in.
enum Surface {
    /// Dry-run: no actual pixels are touched. Used for the first pass of
    /// the two-pass bbox+render protocol. `bbox` accumulates the union of
    /// every painted region's extents, in *root* (surface) space.
    DryRun { bbox: BBox },
    /// Real rendering pass: operations are actually rasterized into
    /// `pixmap`.
    Real { pixmap: Pixmap },
}

/// A cairo::Context-shaped drawing surface built on tiny-skia.
///
/// See the module documentation for the two-pass bbox+render protocol
/// that callers (the future colr.rs/freetype.rs/harfbuzz.rs ports) are
/// expected to drive.
pub struct Painter {
    surface: Surface,
    width: u32,
    height: u32,
    transform: Transform,
    clip: Option<Mask>,
    stack: Vec<GraphicsState>,
    /// Dry-run-only bookkeeping: bbox (root/user space) of the current
    /// effective clip region, i.e. the intersection of all `clip()`
    /// calls in effect right now. Mirrors what `self.clip` (the real
    /// `Mask`) represents, but as a bbox since dry-run mode has no
    /// pixels to build a real mask from. `None` means "unclipped" (the
    /// full surface).
    clip_bbox: Option<BBox>,
    /// Stack of offscreen pixmaps for nested `push_group`/`pop_group`.
    /// Only meaningful in `Surface::Real` mode; in dry-run mode groups
    /// are simply inert (the bbox contribution comes from the paint
    /// operations executed "inside" the group, since dry-run has no
    /// pixels to composite).
    group_stack: Vec<Pixmap>,
    /// Path currently under construction (cairo's "current path").
    path_builder: PathBuilder,
    /// Bounding box, in user space (pre-transform) of the current path's
    /// control points, used for the dry-run clip-bbox estimate.
    current_path_bbox: BBox,
}

impl Painter {
    /// Creates a real painter that rasterizes into a `width x height`
    /// pixmap, starting with an identity transform and no clip.
    pub fn new(width: u32, height: u32) -> anyhow::Result<Self> {
        let pixmap = Pixmap::new(width.max(1), height.max(1))
            .ok_or_else(|| anyhow::anyhow!("Painter::new: invalid pixmap size {width}x{height}"))?;
        Ok(Self {
            surface: Surface::Real { pixmap },
            width,
            height,
            transform: Transform::identity(),
            clip: None,
            stack: vec![],
            clip_bbox: None,
            group_stack: vec![],
            path_builder: PathBuilder::new(),
            current_path_bbox: BBox::empty(),
        })
    }

    /// Creates a dry-run painter used only to compute the ink extents of
    /// a sequence of paint operations (see the module docs). `width`
    /// and `height` are nominal/unused for pixel storage, but are kept
    /// around for parity with `new` and potential future clip-to-surface
    /// bounding.
    pub fn new_dry_run() -> Self {
        Self {
            surface: Surface::DryRun {
                bbox: BBox::empty(),
            },
            width: 0,
            height: 0,
            transform: Transform::identity(),
            clip: None,
            stack: vec![],
            clip_bbox: None,
            group_stack: vec![],
            path_builder: PathBuilder::new(),
            current_path_bbox: BBox::empty(),
        }
    }

    pub fn is_dry_run(&self) -> bool {
        matches!(self.surface, Surface::DryRun { .. })
    }

    /// Returns the accumulated bounding box (only meaningful after
    /// running the operations against a dry-run painter).
    pub fn bbox(&self) -> BBox {
        match &self.surface {
            Surface::DryRun { bbox } => *bbox,
            Surface::Real { .. } => BBox::empty(),
        }
    }

    /// Returns the finished pixmap (only meaningful for a real painter).
    pub fn into_pixmap(self) -> Option<Pixmap> {
        match self.surface {
            Surface::Real { pixmap } => Some(pixmap),
            Surface::DryRun { .. } => None,
        }
    }

    fn active_pixmap_mut(&mut self) -> Option<&mut Pixmap> {
        if let Some(top) = self.group_stack.last_mut() {
            Some(top)
        } else {
            match &mut self.surface {
                Surface::Real { pixmap } => Some(pixmap),
                Surface::DryRun { .. } => None,
            }
        }
    }

    // ----- state stack -----------------------------------------------

    /// Push the current (transform, clip) onto the stack, mirroring
    /// `cairo::Context::save`.
    pub fn save(&mut self) {
        self.stack.push(GraphicsState {
            transform: self.transform,
            clip: self.clip.clone(),
            clip_bbox: self.clip_bbox,
        });
    }

    /// Pop the most recently saved (transform, clip), mirroring
    /// `cairo::Context::restore`.
    pub fn restore(&mut self) {
        if let Some(state) = self.stack.pop() {
            self.transform = state.transform;
            self.clip = state.clip;
            self.clip_bbox = state.clip_bbox;
        }
    }

    // ----- transform ----------------------------------------------------

    /// Pre-concatenates `t` onto the current transform, mirroring
    /// `cairo::Context::transform` (applies `t` before the existing CTM,
    /// i.e. in the *current* user space).
    pub fn transform(&mut self, t: Transform) {
        self.transform = self.transform.pre_concat(t);
    }

    pub fn translate(&mut self, tx: f32, ty: f32) {
        self.transform(Transform::from_translate(tx, ty));
    }

    pub fn scale(&mut self, sx: f32, sy: f32) {
        self.transform(Transform::from_scale(sx, sy));
    }

    pub fn current_transform(&self) -> Transform {
        self.transform
    }

    // ----- path construction --------------------------------------------

    /// Discards the current path, mirroring `cairo::Context::new_path`.
    pub fn new_path(&mut self) {
        self.path_builder = PathBuilder::new();
        self.current_path_bbox = BBox::empty();
    }

    pub fn move_to(&mut self, x: f32, y: f32) {
        self.path_builder.move_to(x, y);
        self.current_path_bbox.add_point(x, y);
    }

    pub fn line_to(&mut self, x: f32, y: f32) {
        self.path_builder.line_to(x, y);
        self.current_path_bbox.add_point(x, y);
    }

    /// Quadratic-to. tiny-skia supports this natively (unlike cairo,
    /// which requires it to be expressed as a cubic - see
    /// `colr::apply_draw_ops_to_context`'s `QuadTo` handling. That
    /// conversion becomes unnecessary once callers use this method
    /// directly).
    pub fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        self.path_builder.quad_to(cx, cy, x, y);
        // Control points lie within the eventual convex hull, so folding
        // them into the bbox is a safe, if slightly conservative, over
        // approximation.
        self.current_path_bbox.add_point(cx, cy);
        self.current_path_bbox.add_point(x, y);
    }

    pub fn curve_to(&mut self, c1x: f32, c1y: f32, c2x: f32, c2y: f32, x: f32, y: f32) {
        self.path_builder.cubic_to(c1x, c1y, c2x, c2y, x, y);
        self.current_path_bbox.add_point(c1x, c1y);
        self.current_path_bbox.add_point(c2x, c2y);
        self.current_path_bbox.add_point(x, y);
    }

    pub fn close_path(&mut self) {
        self.path_builder.close();
    }

    /// Bounding box of the current path's control points, in user
    /// (pre-transform) space. Conservative for curved segments, per
    /// "Тонкое место 1" in the migration plan.
    pub fn current_path_bbox(&self) -> BBox {
        self.current_path_bbox
    }

    fn take_path(&mut self) -> Option<Path> {
        std::mem::replace(&mut self.path_builder, PathBuilder::new()).finish()
    }

    // ----- clipping ------------------------------------------------------

    /// Intersects the current clip with the current path, mirroring
    /// `cairo::Context::clip()`. Consumes/resets the current path, same
    /// as cairo (a subsequent `clip()` call with no new path is a
    /// no-op).
    pub fn clip(&mut self) {
        let path_bbox = self.current_path_bbox;
        let path = self.take_path();

        // Track the bbox contribution for dry-run bbox estimation: the
        // new clip is the path bbox transformed into root space,
        // intersected with whatever clip bbox we already have.
        let transformed = path_bbox.transformed(self.transform);
        self.clip = match (&self.clip, &path, self.width, self.height) {
            (_, None, _, _) => self.clip.clone(),
            (existing, Some(path), width, height) if width > 0 && height > 0 => {
                let mut mask = match existing {
                    Some(m) => m.clone(),
                    None => {
                        let mut full = Mask::new(width, height).expect("nonzero mask size");
                        // A fresh cairo clip starts as "everything visible";
                        // emulate that by filling the mask fully opaque
                        // before intersecting.
                        full.data_mut().iter_mut().for_each(|b| *b = 255);
                        full
                    }
                };
                mask.intersect_path(path, FillRule::Winding, true, self.transform);
                Some(mask)
            }
            _ => self.clip.clone(),
        };

        // Dry-run mode has no real pixels/Mask to intersect against, so
        // track the equivalent information as a plain bbox instead: the
        // new effective clip bbox is the previous one intersected with
        // this path's (transformed) bbox.
        self.clip_bbox = match self.clip_bbox {
            Some(existing) => Some(intersect_bbox(existing, transformed)),
            None => Some(transformed),
        };
    }

    /// Returns the bbox (root/user space) of the current effective clip
    /// region - i.e. the intersection of all `clip()` calls since the
    /// last time the clip was reset by `restore()`-ing past them. Used
    /// by paint operations to know their contribution to the overall
    /// ink extents when running in dry-run mode.
    fn effective_clip_bbox(&self) -> BBox {
        self.clip_bbox.unwrap_or(BBox {
            x0: 0.,
            y0: 0.,
            x1: self.width as f32,
            y1: self.height as f32,
        })
    }

    // ----- groups ----------------------------------------------------------

    /// Starts rendering into a new offscreen group, mirroring
    /// `cairo::Context::push_group()`. In dry-run mode this is a no-op
    /// (paint operations executed while "inside" the group still
    /// contribute to the overall bbox exactly as if un-grouped, since
    /// group compositing doesn't grow the ink extents beyond what was
    /// painted).
    pub fn push_group(&mut self) {
        if let Surface::Real { .. } = &self.surface {
            let pixmap = Pixmap::new(self.width.max(1), self.height.max(1))
                .expect("push_group: nonzero pixmap size");
            self.group_stack.push(pixmap);
        }
    }

    /// Finishes the current group and composites it over its parent
    /// (either the next group down, or the root surface) using
    /// `blend_mode`, respecting the *parent's* current clip mask.
    /// Mirrors `pop_group_to_source()` + `set_operator()` + `paint()`.
    pub fn pop_group(&mut self, blend_mode: BlendMode) {
        if let Surface::Real { .. } = &self.surface {
            let Some(group) = self.group_stack.pop() else {
                return;
            };
            let paint = PixmapPaint {
                blend_mode,
                ..Default::default()
            };
            let mask = self.clip.clone();
            if let Some(parent) = self.active_pixmap_mut() {
                parent.draw_pixmap(
                    0,
                    0,
                    group.as_ref(),
                    &paint,
                    Transform::identity(),
                    mask.as_ref(),
                );
            }
        }
    }

    // ----- painting ----------------------------------------------------

    /// Fills the current clip region with a solid color, mirroring
    /// `set_source_rgba(...)` followed by `paint()`.
    pub fn paint_solid(&mut self, color: Color) {
        self.paint_shader(Shader::SolidColor(color));
    }

    /// Fills the current clip region with an arbitrary `Shader`
    /// (gradient or pattern), mirroring `set_source(pattern)` followed
    /// by `paint()`. Gradients/patterns will be supplied by future
    /// tasks (#3-#5) once colr.rs/freetype.rs/harfbuzz.rs are ported.
    pub fn paint_shader(&mut self, shader: Shader) {
        match &mut self.surface {
            Surface::DryRun { bbox } => {
                let clip_bbox = self.clip_bbox.unwrap_or(BBox {
                    x0: 0.,
                    y0: 0.,
                    x1: self.width as f32,
                    y1: self.height as f32,
                });
                *bbox = bbox.union(&clip_bbox);
            }
            Surface::Real { .. } => {
                let paint = Paint {
                    shader,
                    anti_alias: true,
                    ..Default::default()
                };
                let clip = self.clip.clone();
                let width = self.width;
                let height = self.height;
                if let Some(rect) = Rect::from_xywh(0., 0., width as f32, height as f32) {
                    if let Some(pixmap) = self.active_pixmap_mut() {
                        pixmap.fill_rect(rect, &paint, Transform::identity(), clip.as_ref());
                    }
                }
            }
        }
    }

    /// Fills the current path (without consuming the clip) - a
    /// convenience used by simpler callers/tests that just want to
    /// paint a shape without going through the clip+paint dance. Not a
    /// direct cairo equivalent, but expressible in terms of the same
    /// primitives (`clip()` + `paint_solid()` inside a `save`/`restore`
    /// pair achieves the same result).
    pub fn fill_path_solid(&mut self, color: Color) {
        self.save();
        self.clip();
        self.paint_solid(color);
        self.restore();
    }

    /// Fills the current clip region with per-pixel colors computed by
    /// `f`, mirroring `set_source(pattern)` + `paint()` for gradient
    /// shapes that tiny-skia's built-in `Shader` enum cannot express
    /// natively.
    ///
    /// `f` is called once per device-space pixel that is inside the
    /// current surface, and receives that pixel's coordinates already
    /// mapped back into the *user space* that is active right now (i.e.
    /// the inverse of `current_transform()` has already been applied),
    /// matching the coordinate system that gradient paint parameters
    /// (e.g. `PaintOp::PaintRadialGradient`/`PaintSweepGradient`) are
    /// expressed in. It must return a straight-alpha (non-premultiplied)
    /// `Color`; this method takes care of premultiplying it and
    /// compositing it with the existing pixel contents via
    /// `BlendMode::SourceOver` (tiny-skia's, and cairo's, default
    /// operator), attenuated by the current clip mask's per-pixel
    /// coverage - the same compositing `paint_shader` performs for a
    /// `Shader`.
    ///
    /// This exists for gradient shapes that tiny-skia 0.11 has no
    /// native shader for: a general two-radius conical/radial gradient
    /// (tiny-skia's `RadialGradient` only supports a zero-radius start
    /// circle, but COLRv1 fonts may specify a non-zero `r0`) and a
    /// sweep/conic gradient (tiny-skia has no sweep gradient shader at
    /// all, unlike what was assumed when this migration was planned).
    pub fn paint_with<F: FnMut(f32, f32) -> Color>(&mut self, mut f: F) {
        match &mut self.surface {
            Surface::DryRun { bbox } => {
                let clip_bbox = self.clip_bbox.unwrap_or(BBox {
                    x0: 0.,
                    y0: 0.,
                    x1: self.width as f32,
                    y1: self.height as f32,
                });
                *bbox = bbox.union(&clip_bbox);
            }
            Surface::Real { .. } => {
                let Some(inverse) = self.transform.invert() else {
                    return;
                };
                let clip = self.clip.clone();
                let width = self.width;
                let height = self.height;
                let Some(pixmap) = self.active_pixmap_mut() else {
                    return;
                };
                for y in 0..height {
                    for x in 0..width {
                        let idx = (y * width + x) as usize;
                        let mask_alpha = clip.as_ref().map(|m| m.data()[idx]).unwrap_or(255);
                        if mask_alpha == 0 {
                            continue;
                        }

                        let mut pt: tiny_skia::Point = (x as f32 + 0.5, y as f32 + 0.5).into();
                        inverse.map_point(&mut pt);
                        let color = f(pt.x, pt.y);
                        let mut premul = color.premultiply().to_color_u8();
                        if mask_alpha != 255 {
                            premul = scale_premultiplied(premul, mask_alpha);
                        }

                        let dst = &mut pixmap.pixels_mut()[idx];
                        *dst = composite_source_over(premul, *dst);
                    }
                }
            }
        }
    }
}

/// Scales a premultiplied color's channels (including alpha) by
/// `coverage / 255`, as when applying a clip mask's per-pixel coverage
/// to a source color before compositing.
fn scale_premultiplied(
    c: tiny_skia::PremultipliedColorU8,
    coverage: u8,
) -> tiny_skia::PremultipliedColorU8 {
    let scale = |v: u8| -> u8 { ((v as u32 * coverage as u32 + 127) / 255) as u8 };
    tiny_skia::PremultipliedColorU8::from_rgba(
        scale(c.red()),
        scale(c.green()),
        scale(c.blue()),
        scale(c.alpha()),
    )
    .expect("scaled channels stay <= scaled alpha")
}

/// Standard Porter-Duff "source over destination" compositing of two
/// premultiplied colors, matching `BlendMode::SourceOver` (tiny-skia's
/// default blend mode, and cairo's default `Operator::Over`).
fn composite_source_over(
    src: tiny_skia::PremultipliedColorU8,
    dst: tiny_skia::PremultipliedColorU8,
) -> tiny_skia::PremultipliedColorU8 {
    let inv_sa = 255 - src.alpha() as u32;
    let blend = |s: u8, d: u8| -> u8 { (s as u32 + (d as u32 * inv_sa + 127) / 255) as u8 };
    tiny_skia::PremultipliedColorU8::from_rgba(
        blend(src.red(), dst.red()),
        blend(src.green(), dst.green()),
        blend(src.blue(), dst.blue()),
        blend(src.alpha(), dst.alpha()),
    )
    .expect("source-over of two premultiplied colors stays premultiplied")
}

fn intersect_bbox(a: BBox, b: BBox) -> BBox {
    if a.is_empty() || b.is_empty() {
        return BBox::empty();
    }
    let x0 = a.x0.max(b.x0);
    let y0 = a.y0.max(b.y0);
    let x1 = a.x1.min(b.x1);
    let y1 = a.y1.min(b.y1);
    BBox { x0, y0, x1, y1 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bbox_of_simple_rect_path() {
        let mut p = Painter::new_dry_run();
        p.move_to(10., 20.);
        p.line_to(110., 20.);
        p.line_to(110., 70.);
        p.line_to(10., 70.);
        p.close_path();

        let bbox = p.current_path_bbox();
        assert_eq!(bbox.x0, 10.);
        assert_eq!(bbox.y0, 20.);
        assert_eq!(bbox.x1, 110.);
        assert_eq!(bbox.y1, 70.);
        assert_eq!(bbox.width(), 100.);
        assert_eq!(bbox.height(), 50.);
    }

    #[test]
    fn bbox_curve_is_conservative_hull_of_control_points() {
        let mut p = Painter::new_dry_run();
        p.move_to(0., 0.);
        // control points stick out well beyond the actual curve
        p.curve_to(50., -100., 150., -100., 100., 0.);
        let bbox = p.current_path_bbox();
        // hull must contain all 4 points: (0,0) (50,-100) (150,-100) (100,0)
        assert_eq!(bbox.x0, 0.);
        assert_eq!(bbox.y0, -100.);
        assert_eq!(bbox.x1, 150.);
        assert_eq!(bbox.y1, 0.);
    }

    #[test]
    fn dry_run_paint_solid_extends_bbox_to_clip_extent() {
        let mut p = Painter::new_dry_run();
        p.width = 64;
        p.height = 64;
        p.save();
        p.move_to(5., 5.);
        p.line_to(20., 5.);
        p.line_to(20., 20.);
        p.line_to(5., 20.);
        p.close_path();
        p.clip();
        p.paint_solid(Color::from_rgba8(255, 0, 0, 255));
        p.restore();

        let bbox = p.bbox();
        assert_eq!(bbox.x0, 5.);
        assert_eq!(bbox.y0, 5.);
        assert_eq!(bbox.x1, 20.);
        assert_eq!(bbox.y1, 20.);
    }

    #[test]
    fn save_restore_preserves_transform() {
        let mut p = Painter::new(8, 8).unwrap();
        p.translate(3., 4.);
        p.save();
        p.scale(2., 2.);
        p.translate(1., 1.);
        let t_inside = p.current_transform();
        p.restore();
        let t_after = p.current_transform();

        assert_eq!(t_after.tx, 3.);
        assert_eq!(t_after.ty, 4.);
        assert_eq!(t_after.sx, 1.);
        assert_eq!(t_after.sy, 1.);

        // sanity: the transform inside the save/restore pair really was
        // different, i.e. restore did something.
        assert_ne!(t_inside, t_after);
    }

    #[test]
    fn nested_save_restore_stack_order() {
        let mut p = Painter::new(8, 8).unwrap();
        p.translate(1., 0.);
        p.save();
        p.translate(2., 0.);
        p.save();
        p.translate(4., 0.);
        assert_eq!(p.current_transform().tx, 7.);
        p.restore();
        assert_eq!(p.current_transform().tx, 3.);
        p.restore();
        assert_eq!(p.current_transform().tx, 1.);
    }

    fn fill_whole_pixmap(painter: &mut Painter, color: Color) {
        painter.save();
        painter.new_path();
        painter.move_to(0., 0.);
        painter.line_to(painter.width as f32, 0.);
        painter.line_to(painter.width as f32, painter.height as f32);
        painter.line_to(0., painter.height as f32);
        painter.close_path();
        painter.clip();
        painter.paint_solid(color);
        painter.restore();
    }

    #[test]
    fn group_composite_over_blend_mode() {
        // Blue background, then a group containing a red fill composited
        // with BlendMode::SourceOver should just show red (SourceOver
        // fully replaces where source alpha is 1).
        let mut p = Painter::new(4, 4).unwrap();
        fill_whole_pixmap(&mut p, Color::from_rgba8(0, 0, 255, 255));

        p.push_group();
        fill_whole_pixmap(&mut p, Color::from_rgba8(255, 0, 0, 255));
        p.pop_group(BlendMode::SourceOver);

        let pixmap = p.into_pixmap().unwrap();
        let pixel = pixmap.pixel(0, 0).unwrap();
        assert_eq!((pixel.red(), pixel.green(), pixel.blue()), (255, 0, 0));
    }

    #[test]
    fn group_composite_multiply_blend_mode() {
        // Red (255,0,0) background; group with (0,255,0) green painted
        // using BlendMode::Multiply should produce black (each channel
        // multiplies to 0 except where both are non-zero).
        let mut p = Painter::new(4, 4).unwrap();
        fill_whole_pixmap(&mut p, Color::from_rgba8(255, 0, 0, 255));

        p.push_group();
        fill_whole_pixmap(&mut p, Color::from_rgba8(0, 255, 0, 255));
        p.pop_group(BlendMode::Multiply);

        let pixmap = p.into_pixmap().unwrap();
        let pixel = pixmap.pixel(0, 0).unwrap();
        assert_eq!((pixel.red(), pixel.green(), pixel.blue()), (0, 0, 0));
    }

    #[test]
    fn premultiplied_output_matches_rasterized_glyph_expectations() {
        // RasterizedGlyph::data is documented as premultiplied RGBA.
        // tiny-skia's Pixmap always stores premultiplied color; verify a
        // half-transparent red fill comes back premultiplied (R <= A).
        let mut p = Painter::new(2, 2).unwrap();
        fill_whole_pixmap(&mut p, Color::from_rgba8(255, 0, 0, 128));
        let pixmap = p.into_pixmap().unwrap();
        let pixel = pixmap.pixel(0, 0).unwrap();
        assert_eq!(pixel.alpha(), 128);
        // premultiplied red at alpha=128 must be roughly 128, definitely
        // not 255 (which would indicate un-premultiplied storage).
        assert!(pixel.red() <= 129);
        assert!(pixel.red() > 0);
    }
}
