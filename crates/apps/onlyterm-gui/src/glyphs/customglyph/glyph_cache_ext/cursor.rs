use super::super::types::*;
use crate::glyphcache::GlyphCache;
use crate::utilsprites::RenderMetrics;
use ::window::bitmaps::atlas::Sprite;
use ::window::color::SrgbaPixel;
use onlyterm_config::DimensionContext;
use termwiz::surface::CursorShape;
use tiny_skia::{BlendMode, Paint, PathBuilder, PixmapMut};
use window::{BitmapImage, Image, Point, Rect};

impl GlyphCache {
    pub(super) fn draw_polys(
        &mut self,
        metrics: &RenderMetrics,
        polys: &[Poly],
        buffer: &mut Image,
        aa: PolyAA,
        blend_mode: BlendMode,
    ) {
        let (width, height) = buffer.image_dimensions();
        let mut pixmap =
            PixmapMut::from_bytes(buffer.pixel_data_slice_mut(), width as u32, height as u32)
                .expect("make pixmap from existing bitmap");

        for Poly {
            path,
            intensity,
            style,
        } in polys
        {
            let mut paint = Paint {
                blend_mode,
                ..Default::default()
            };
            let intensity = intensity.to_scale();
            paint.set_color(
                tiny_skia::Color::from_rgba(intensity, intensity, intensity, intensity).unwrap(),
            );
            paint.anti_alias = match aa {
                PolyAA::AntiAlias => true,
                PolyAA::MoarPixels => false,
            };
            paint.force_hq_pipeline = true;
            let mut pb = PathBuilder::new();
            for item in path.iter() {
                item.to_skia(width, height, metrics.underline_height as f32, &mut pb);
            }
            let path = pb.finish().expect("poly path to be valid");
            style.apply(metrics.underline_height as f32, &paint, &path, &mut pixmap);
        }
    }

    pub fn cursor_sprite(
        &mut self,
        shape: Option<CursorShape>,
        metrics: &RenderMetrics,
        width: u8,
    ) -> anyhow::Result<Sprite> {
        if let Some(sprite) = self.cursor_glyphs.get(&(shape, width)) {
            return Ok(sprite.clone());
        }

        let mut metrics = metrics.scale_cell_width(width as f64);
        if let Some(d) = &self.fonts.config().cursor_thickness {
            metrics.underline_height = d.evaluate_as_pixels(DimensionContext {
                dpi: self.fonts.get_dpi() as f32,
                pixel_max: metrics.underline_height as f32,
                pixel_cell: metrics.cell_size.height as f32,
            }) as isize;
        }

        let mut buffer = Image::new(
            metrics.cell_size.width as usize,
            metrics.cell_size.height as usize,
        );
        let black = SrgbaPixel::rgba(0, 0, 0, 0);
        let cell_rect = Rect::new(Point::new(0, 0), metrics.cell_size);
        buffer.clear_rect(cell_rect, black);

        match shape {
            None => {}
            Some(CursorShape::Default) => {
                buffer.clear_rect(cell_rect, SrgbaPixel::rgba(0xff, 0xff, 0xff, 0xff));
            }
            Some(CursorShape::BlinkingBlock | CursorShape::SteadyBlock) => {
                self.draw_polys(
                    &metrics,
                    &[Poly {
                        path: &[
                            PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Zero),
                            PolyCommand::LineTo(BlockCoord::One, BlockCoord::Zero),
                            PolyCommand::LineTo(BlockCoord::One, BlockCoord::One),
                            PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::One),
                            PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::Zero),
                        ],
                        intensity: BlockAlpha::Full,
                        style: PolyStyle::OutlineHeavy,
                    }],
                    &mut buffer,
                    PolyAA::AntiAlias,
                    BlendMode::default(),
                );
            }
            Some(CursorShape::BlinkingBar | CursorShape::SteadyBar) => {
                self.draw_polys(
                    &metrics,
                    &[Poly {
                        path: &[
                            PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Zero),
                            PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::One),
                        ],
                        intensity: BlockAlpha::Full,
                        style: PolyStyle::OutlineHeavy,
                    }],
                    &mut buffer,
                    PolyAA::AntiAlias,
                    BlendMode::default(),
                );
            }
            Some(CursorShape::BlinkingUnderline | CursorShape::SteadyUnderline) => {
                self.draw_polys(
                    &metrics,
                    &[Poly {
                        path: &[
                            PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::One),
                            PolyCommand::LineTo(BlockCoord::One, BlockCoord::One),
                        ],
                        intensity: BlockAlpha::Full,
                        style: PolyStyle::OutlineHeavy,
                    }],
                    &mut buffer,
                    PolyAA::AntiAlias,
                    BlendMode::default(),
                );
            }
        }

        let sprite = self.atlas.allocate(&buffer)?;
        self.cursor_glyphs.insert((shape, width), sprite.clone());
        Ok(sprite)
    }
}
