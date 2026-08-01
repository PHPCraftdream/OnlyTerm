use config::RgbaColor;

// `image_colors::extract_colors_from_image_impl` (and the private helpers it
// alone used: `CachedAnalysis`, `color_diff_lab`, `contrast_ratio`,
// `extract_distinct_colors_lab`) had its only caller
// (`extract_colors_from_image_rhai`, part of the deleted rhai port below)
// removed along with `register_rhai`. Nothing outside the rhai port ever
// called it, so rather than deleting the whole module (out of scope here --
// this task only covers `register_rhai` call sites), it is left in place and
// silenced; re-wiring or removing it belongs to a future task that touches
// image-based color extraction specifically.
#[allow(dead_code)]
mod image_colors;
pub mod schemes;

#[derive(Clone)]
pub struct ColorWrap(RgbaColor);

impl ColorWrap {
    pub fn complement(&self) -> Self {
        Self(self.0.complement().into())
    }
    pub fn complement_ryb(&self) -> Self {
        Self(self.0.complement_ryb().into())
    }
    pub fn triad(&self) -> (Self, Self) {
        let (a, b) = self.0.triad();
        (Self(a.into()), Self(b.into()))
    }
    pub fn square(&self) -> (Self, Self, Self) {
        let (a, b, c) = self.0.square();
        (Self(a.into()), Self(b.into()), Self(c.into()))
    }
    pub fn saturate(&self, factor: f64) -> Self {
        Self(self.0.saturate(factor).into())
    }
    pub fn saturate_fixed(&self, amount: f64) -> Self {
        Self(self.0.saturate_fixed(amount).into())
    }
    pub fn lighten(&self, factor: f64) -> Self {
        Self(self.0.lighten(factor).into())
    }
    pub fn lighten_fixed(&self, amount: f64) -> Self {
        Self(self.0.lighten_fixed(amount).into())
    }
    pub fn adjust_hue_fixed(&self, amount: f64) -> Self {
        Self(self.0.adjust_hue_fixed(amount).into())
    }
    pub fn adjust_hue_fixed_ryb(&self, amount: f64) -> Self {
        Self(self.0.adjust_hue_fixed_ryb(amount).into())
    }
}

impl PartialEq for ColorWrap {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl std::fmt::Display for ColorWrap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s: String = self.0.into();
        write!(f, "{s}")
    }
}

// `gradient_colors_impl`/`register_rhai`/the `*_rhai` free functions that
// used to live below this point were an "L4a" rhai port of the functionality
// above (rhai equivalents of `ColorWrap`'s methods, scheme loading/saving,
// gradients, image color extraction, ...), registered into a `rhai::Engine`
// for scripts to call. With the scripting layer removed, `register_rhai` had
// no remaining caller anywhere in the workspace (it was only ever invoked by
// this crate's own `tests/rhai_smoke.rs`, which exercised nothing else); it
// has been deleted along with its private helpers. Every one of `ColorWrap`,
// `Gradient`/`Palette`/`ColorSchemeMetaData` handling, and
// `image_colors::extract_colors_from_image_impl` continues to be reachable
// as plain Rust (see `impl ColorWrap` above and `crates/sync-color-schemes`,
// which already calls this crate's `schemes::*` loaders directly, not
// through rhai).
