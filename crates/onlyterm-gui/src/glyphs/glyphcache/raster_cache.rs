use super::*;
use ::window::bitmaps::atlas::OutOfTextureSpace;
use euclid::num::Zero;
use onlyterm_font::units::PixelLength;
use onlyterm_font::{GlyphInfo, LoadedFont};

impl GlyphCache {
    pub(super) fn failed_glyph(info: &GlyphInfo) -> Rc<CachedGlyph> {
        Rc::new(CachedGlyph {
            brightness_adjust: 1.0,
            has_color: false,
            texture: None,
            x_advance: info.x_advance,
            x_offset: info.x_offset,
            y_offset: info.y_offset,
            bearing_x: PixelLength::zero(),
            bearing_y: PixelLength::zero(),
            scale: 1.0,
        })
    }

    pub(super) fn load_glyph_sync(
        &mut self,
        info: &GlyphInfo,
        font: &Rc<LoadedFont>,
        followed_by_space: bool,
        num_cells: u8,
    ) -> anyhow::Result<Rc<CachedGlyph>> {
        let result = font
            .rasterize_glyph(info.glyph_pos, info.font_idx)
            .and_then(|glyph| {
                self.load_glyph_with_raster(info, font, followed_by_space, num_cells, &glyph)
            });
        match result {
            Ok(glyph) => Ok(glyph),
            Err(err)
                if err
                    .root_cause()
                    .downcast_ref::<OutOfTextureSpace>()
                    .is_some() =>
            {
                Err(err)
            }
            Err(err) => {
                log::error!("glyph rasterization failed; using blank: {err:#}");
                Ok(Self::failed_glyph(info))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utilsprites::RenderMetrics;
    use onlyterm_bidi::Direction;

    #[test]
    fn first_glyph_lookup_returns_pixels_without_a_later_paint() {
        config::use_test_configuration();
        let fonts = Rc::new(FontConfiguration::new(None, 96).unwrap());
        let metrics = RenderMetrics::new(&fonts).unwrap();
        let style = config::TextStyle::default();
        let font = fonts.resolve_font(&style).unwrap();
        let infos = font
            .blocking_shape("ABC", None, Direction::LeftToRight, None, None)
            .unwrap();
        let mut cache = GlyphCache::new_in_memory(&fonts, 128).unwrap();
        for info in infos {
            let first = cache
                .cached_glyph(&info, &style, false, &font, &metrics, info.num_cells)
                .unwrap();
            assert!(
                first.texture.is_some(),
                "first lookup must not return a pending blank"
            );
            let again = cache
                .cached_glyph(&info, &style, false, &font, &metrics, info.num_cells)
                .unwrap();
            assert!(Rc::ptr_eq(&first, &again));
        }
    }
}
