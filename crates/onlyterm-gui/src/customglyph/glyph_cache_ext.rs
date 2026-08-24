use super::types::*;
use crate::glyphcache::{GlyphCache, SizedBlockKey};
use crate::utilsprites::RenderMetrics;
use ::window::bitmaps::atlas::Sprite;
use ::window::color::SrgbaPixel;
use config::DimensionContext;
use onlyterm_font::units::PixelLength;
use std::ops::Range;
use termwiz::surface::CursorShape;
use tiny_skia::{BlendMode, FillRule, Paint, PathBuilder, PixmapMut, Transform};
use window::{BitmapImage, Image, Point, Rect};

impl GlyphCache {
    fn draw_polys(
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

    pub fn block_sprite(
        &mut self,
        render_metrics: &RenderMetrics,
        key: SizedBlockKey,
    ) -> anyhow::Result<Sprite> {
        let metrics = match &key.block {
            BlockKey::PolyWithCustomMetrics {
                underline_height,
                cell_size,
                ..
            } => RenderMetrics {
                descender: PixelLength::new(0.),
                descender_row: 0,
                descender_plus_two: 0,
                underline_height: *underline_height,
                strike_row: 0,
                cell_size: *cell_size,
            },
            _ => *render_metrics,
        };

        let mut buffer = Image::new(
            metrics.cell_size.width as usize,
            metrics.cell_size.height as usize,
        );
        let black = SrgbaPixel::rgba(0, 0, 0, 0);

        let cell_rect = Rect::new(Point::new(0, 0), metrics.cell_size);

        buffer.clear_rect(cell_rect, black);

        match key.block {
            BlockKey::Blocks(blocks) => {
                let width = metrics.cell_size.width as f32;
                let height = metrics.cell_size.height as f32;
                let (x_half, y_half) = (width / 2., height / 2.);
                let (x_eighth, y_eighth) = (width / 8., height / 8.);

                for block in blocks.iter() {
                    match block {
                        Block::Custom(x0, x1, y0, y1, alpha) => {
                            let left = (*x0 as f32) * x_eighth;
                            let right = (*x1 as f32) * x_eighth;
                            let top = (*y0 as f32) * y_eighth;
                            let bottom = (*y1 as f32) * y_eighth;
                            fill_rect(&mut buffer, left..right, top..bottom, *alpha);
                        }
                        Block::UpperBlock(num) => {
                            let lower = (*num as f32) * y_eighth;
                            fill_rect(&mut buffer, 0.0..width, 0.0..lower, BlockAlpha::Full);
                        }
                        Block::LowerBlock(num) => {
                            let upper = ((8 - num) as f32) * y_eighth;
                            fill_rect(&mut buffer, 0.0..width, upper..height, BlockAlpha::Full);
                        }
                        Block::LeftBlock(num) => {
                            let right = (*num as f32) * x_eighth;
                            fill_rect(&mut buffer, 0.0..right, 0.0..height, BlockAlpha::Full);
                        }
                        Block::RightBlock(num) => {
                            let left = ((8 - num) as f32) * x_eighth;
                            fill_rect(&mut buffer, left..width, 0.0..height, BlockAlpha::Full);
                        }
                        Block::VerticalBlock(x0, x1) => {
                            let left = (*x0 as f32) * x_eighth;
                            let right = (*x1 as f32) * x_eighth;
                            fill_rect(&mut buffer, left..right, 0.0..height, BlockAlpha::Full);
                        }
                        Block::HorizontalBlock(y0, y1) => {
                            let top = (*y0 as f32) * y_eighth;
                            let bottom = (*y1 as f32) * y_eighth;
                            fill_rect(&mut buffer, 0.0..width, top..bottom, BlockAlpha::Full);
                        }
                        Block::QuadrantUL => {
                            fill_rect(&mut buffer, 0.0..x_half, 0.0..y_half, BlockAlpha::Full)
                        }
                        Block::QuadrantUR => {
                            fill_rect(&mut buffer, x_half..width, 0.0..y_half, BlockAlpha::Full)
                        }
                        Block::QuadrantLL => {
                            fill_rect(&mut buffer, 0.0..x_half, y_half..height, BlockAlpha::Full)
                        }
                        Block::QuadrantLR => {
                            fill_rect(&mut buffer, x_half..width, y_half..height, BlockAlpha::Full)
                        }
                    }
                }
            }
            BlockKey::Triangles(triangles, alpha) => {
                let mut draw = |cmd: &'static [PolyCommand], style: PolyStyle| {
                    self.draw_polys(
                        &metrics,
                        &[Poly {
                            path: cmd,
                            intensity: alpha,
                            style,
                        }],
                        &mut buffer,
                        if config::configuration().anti_alias_custom_block_glyphs {
                            PolyAA::AntiAlias
                        } else {
                            PolyAA::MoarPixels
                        },
                        BlendMode::default(),
                    );
                };

                macro_rules! start {
                    () => {
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2))
                    };
                }
                macro_rules! close {
                    () => {
                        PolyCommand::Close
                    };
                }
                macro_rules! p0 {
                    () => {
                        PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::Zero)
                    };
                }
                macro_rules! p1 {
                    () => {
                        PolyCommand::LineTo(BlockCoord::One, BlockCoord::Zero)
                    };
                }
                macro_rules! p2 {
                    () => {
                        PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::One)
                    };
                }
                macro_rules! p3 {
                    () => {
                        PolyCommand::LineTo(BlockCoord::One, BlockCoord::One)
                    };
                }

                // Draw triangles
                if triangles.contains(Triangle::UPPER) {
                    draw(&[start!(), p0!(), p1!(), close!()], PolyStyle::Fill);
                }
                if triangles.contains(Triangle::LOWER) {
                    draw(&[start!(), p2!(), p3!(), close!()], PolyStyle::Fill);
                }
                if triangles.contains(Triangle::LEFT) {
                    draw(&[start!(), p0!(), p2!(), close!()], PolyStyle::Fill);
                }
                if triangles.contains(Triangle::RIGHT) {
                    draw(&[start!(), p1!(), p3!(), close!()], PolyStyle::Fill);
                }

                // Fill antialiased lines between triangles
                let style = if alpha == BlockAlpha::Full {
                    PolyStyle::Outline
                } else {
                    PolyStyle::OutlineAlpha
                };
                if triangles.contains(Triangle::UPPER | Triangle::LEFT) {
                    draw(&[start!(), p0!()], style);
                }
                if triangles.contains(Triangle::UPPER | Triangle::RIGHT) {
                    draw(&[start!(), p1!()], style);
                }
                if triangles.contains(Triangle::LOWER | Triangle::LEFT) {
                    draw(&[start!(), p2!()], style);
                }
                if triangles.contains(Triangle::LOWER | Triangle::RIGHT) {
                    draw(&[start!(), p3!()], style);
                }
            }
            BlockKey::CellDiagonals(diagonals) => {
                let mut draw = |cmd: &'static [PolyCommand]| {
                    self.draw_polys(
                        &metrics,
                        &[Poly {
                            path: cmd,
                            intensity: BlockAlpha::Full,
                            style: PolyStyle::Outline,
                        }],
                        &mut buffer,
                        if config::configuration().anti_alias_custom_block_glyphs {
                            PolyAA::AntiAlias
                        } else {
                            PolyAA::MoarPixels
                        },
                        BlendMode::default(),
                    );
                };

                macro_rules! U {
                    () => {
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Zero)
                    };
                }
                macro_rules! D {
                    () => {
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::One)
                    };
                }
                macro_rules! L {
                    () => {
                        PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::Frac(1, 2))
                    };
                }
                macro_rules! R {
                    () => {
                        PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 2))
                    };
                }

                if diagonals.contains(CellDiagonal::UPPER_LEFT) {
                    draw(&[U!(), L!()]);
                }
                if diagonals.contains(CellDiagonal::UPPER_RIGHT) {
                    draw(&[U!(), R!()]);
                }
                if diagonals.contains(CellDiagonal::LOWER_LEFT) {
                    draw(&[D!(), L!()]);
                }
                if diagonals.contains(CellDiagonal::LOWER_RIGHT) {
                    draw(&[D!(), R!()]);
                }
            }
            BlockKey::Sextant(pattern) => {
                let width = metrics.cell_size.width as f32;
                let height = metrics.cell_size.height as f32;
                let (x_half, y_third) = (width / 2., height / 3.);
                for row in 0..3 {
                    for col in 0..2 {
                        let bit = 2 * row + col;
                        if pattern & (1u8 << bit) != 0 {
                            fill_rect(
                                &mut buffer,
                                col as f32 * x_half..(col + 1) as f32 * x_half,
                                row as f32 * y_third..(row + 1) as f32 * y_third,
                                BlockAlpha::Full,
                            );
                        }
                    }
                }
            }
            BlockKey::Octant(pattern) => {
                let width = metrics.cell_size.width as f32;
                let height = metrics.cell_size.height as f32;
                let (x_half, y_fourth) = (width / 2., height / 4.);
                for row in 0..4 {
                    for col in 0..2 {
                        let bit = 2 * row + col;
                        if pattern & (1u8 << bit) != 0 {
                            fill_rect(
                                &mut buffer,
                                col as f32 * x_half..(col + 1) as f32 * x_half,
                                row as f32 * y_fourth..(row + 1) as f32 * y_fourth,
                                BlockAlpha::Full,
                            );
                        }
                    }
                }
            }
            BlockKey::Braille(dots_pattern) => {
                // `dots_pattern` is a byte whose bits corresponds to dots
                // on a 2 by 4 dots-grid.
                // The position of a dot for a bit position (1-indexed) is as follow:
                // 1 4  |
                // 2 5  |<- These 3 lines are filled first (for the first 64 symbols)
                // 3 6  |
                // 7 8  <- This last line is filled last (for the remaining 192 symbols)
                //
                // NOTE: for simplicity & performance reasons, a dot is a square not a circle.

                let dot_area_width = metrics.cell_size.width as f32 / 2.;
                let dot_area_height = metrics.cell_size.height as f32 / 4.;
                let square_length = dot_area_width / 2.;
                let topleft_offset_x = dot_area_width / 2. - square_length / 2.;
                let topleft_offset_y = dot_area_height / 2. - square_length / 2.;

                let (width, height) = buffer.image_dimensions();
                let mut pixmap = PixmapMut::from_bytes(
                    buffer.pixel_data_slice_mut(),
                    width as u32,
                    height as u32,
                )
                .expect("make pixmap from existing bitmap");
                let mut paint = Paint::default();
                paint.set_color(tiny_skia::Color::WHITE);
                paint.force_hq_pipeline = true;
                paint.anti_alias = true;
                let identity = Transform::identity();

                const BIT_MASK_AND_DOT_POSITION: [(u8, f32, f32); 8] = [
                    (1 << 0, 0., 0.),
                    (1 << 1, 0., 1.),
                    (1 << 2, 0., 2.),
                    (1 << 3, 1., 0.),
                    (1 << 4, 1., 1.),
                    (1 << 5, 1., 2.),
                    (1 << 6, 0., 3.),
                    (1 << 7, 1., 3.),
                ];
                for (bit_mask, dot_pos_x, dot_pos_y) in &BIT_MASK_AND_DOT_POSITION {
                    if dots_pattern & bit_mask == 0 {
                        // Bit for this dot position is not set
                        continue;
                    }
                    let topleft_x = (*dot_pos_x) * dot_area_width + topleft_offset_x;
                    let topleft_y = (*dot_pos_y) * dot_area_height + topleft_offset_y;

                    let path = PathBuilder::from_rect(
                        tiny_skia::Rect::from_xywh(
                            topleft_x,
                            topleft_y,
                            square_length,
                            square_length,
                        )
                        .expect("valid rect"),
                    );
                    pixmap.fill_path(&path, &paint, FillRule::Winding, identity, None);
                }
            }
            BlockKey::Progress(chunks) => {
                let mut draw = |cmd: &'static [PolyCommand], style: PolyStyle| {
                    self.draw_polys(
                        &metrics,
                        &[Poly {
                            path: cmd,
                            intensity: BlockAlpha::Full,
                            style,
                        }],
                        &mut buffer,
                        if config::configuration().anti_alias_custom_block_glyphs {
                            PolyAA::AntiAlias
                        } else {
                            PolyAA::MoarPixels
                        },
                        BlendMode::default(),
                    );
                };

                if chunks.contains(ProgressChunk::LEFT) {
                    draw(
                        &[
                            PolyCommand::MoveTo(BlockCoord::One, BlockCoord::Frac(1, 6)),
                            PolyCommand::LineTo(BlockCoord::Frac(1, 6), BlockCoord::Frac(1, 6)),
                            PolyCommand::LineTo(BlockCoord::Frac(1, 6), BlockCoord::Frac(6 - 1, 6)),
                            PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(6 - 1, 6)),
                        ],
                        PolyStyle::OutlineHeavy,
                    );

                    if chunks.contains(ProgressChunk::FULL) {
                        draw(
                            &[
                                PolyCommand::MoveTo(
                                    BlockCoord::One,
                                    BlockCoord::FracWithOffset(1, 6, LineScale::Mul(6)),
                                ),
                                PolyCommand::LineTo(
                                    BlockCoord::FracWithOffset(1, 6, LineScale::Mul(6)),
                                    BlockCoord::FracWithOffset(1, 6, LineScale::Mul(6)),
                                ),
                                PolyCommand::LineTo(
                                    BlockCoord::FracWithOffset(1, 6, LineScale::Mul(6)),
                                    BlockCoord::FracWithOffset(6 - 1, 6, LineScale::Mul(-6)),
                                ),
                                PolyCommand::LineTo(
                                    BlockCoord::One,
                                    BlockCoord::FracWithOffset(6 - 1, 6, LineScale::Mul(-6)),
                                ),
                                PolyCommand::Close,
                            ],
                            PolyStyle::Fill,
                        );
                    }
                }
                if chunks.contains(ProgressChunk::RIGHT) {
                    draw(
                        &[
                            PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 6)),
                            PolyCommand::LineTo(BlockCoord::Frac(6 - 1, 6), BlockCoord::Frac(1, 6)),
                            PolyCommand::LineTo(
                                BlockCoord::Frac(6 - 1, 6),
                                BlockCoord::Frac(6 - 1, 6),
                            ),
                            PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::Frac(6 - 1, 6)),
                        ],
                        PolyStyle::OutlineHeavy,
                    );

                    if chunks.contains(ProgressChunk::FULL) {
                        draw(
                            &[
                                PolyCommand::MoveTo(
                                    BlockCoord::Zero,
                                    BlockCoord::FracWithOffset(1, 6, LineScale::Mul(6)),
                                ),
                                PolyCommand::LineTo(
                                    BlockCoord::FracWithOffset(6 - 1, 6, LineScale::Mul(-6)),
                                    BlockCoord::FracWithOffset(1, 6, LineScale::Mul(6)),
                                ),
                                PolyCommand::LineTo(
                                    BlockCoord::FracWithOffset(6 - 1, 6, LineScale::Mul(-6)),
                                    BlockCoord::FracWithOffset(6 - 1, 6, LineScale::Mul(-6)),
                                ),
                                PolyCommand::LineTo(
                                    BlockCoord::Zero,
                                    BlockCoord::FracWithOffset(6 - 1, 6, LineScale::Mul(-6)),
                                ),
                                PolyCommand::Close,
                            ],
                            PolyStyle::Fill,
                        );
                    }
                }
                if chunks.contains(ProgressChunk::MIDDLE) {
                    draw(
                        &[
                            PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 6)),
                            PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 6)),
                            PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(6 - 1, 6)),
                            PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(6 - 1, 6)),
                        ],
                        PolyStyle::OutlineHeavy,
                    );

                    if chunks.contains(ProgressChunk::FULL) {
                        draw(
                            &[
                                PolyCommand::MoveTo(
                                    BlockCoord::Zero,
                                    BlockCoord::FracWithOffset(1, 6, LineScale::Mul(6)),
                                ),
                                PolyCommand::LineTo(
                                    BlockCoord::One,
                                    BlockCoord::FracWithOffset(1, 6, LineScale::Mul(6)),
                                ),
                                PolyCommand::LineTo(
                                    BlockCoord::One,
                                    BlockCoord::FracWithOffset(6 - 1, 6, LineScale::Mul(-6)),
                                ),
                                PolyCommand::LineTo(
                                    BlockCoord::Zero,
                                    BlockCoord::FracWithOffset(6 - 1, 6, LineScale::Mul(-6)),
                                ),
                                PolyCommand::Close,
                            ],
                            PolyStyle::Fill,
                        );
                    }
                }
            }
            BlockKey::Branches(pattern) => {
                let mut draw =
                    |cmd: &'static [PolyCommand], style: PolyStyle, blend_mode: BlendMode| {
                        self.draw_polys(
                            &metrics,
                            &[Poly {
                                path: cmd,
                                intensity: BlockAlpha::Full,
                                style,
                            }],
                            &mut buffer,
                            if config::configuration().anti_alias_custom_block_glyphs {
                                PolyAA::AntiAlias
                            } else {
                                PolyAA::MoarPixels
                            },
                            blend_mode,
                        );
                    };

                if pattern.contains(Branch::VERTICAL) {
                    draw(
                        &[
                            PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                            PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                        ],
                        PolyStyle::OutlineHeavy,
                        BlendMode::default(),
                    );
                }
                if pattern.contains(Branch::HORIZONTAL) {
                    draw(
                        &[
                            PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                            PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                        ],
                        PolyStyle::OutlineHeavy,
                        BlendMode::default(),
                    );
                }
                if pattern.contains(Branch::RIGHT_TO_DOWN) {
                    draw(
                        &[
                            PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                            PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(7, 8)),
                            PolyCommand::QuadTo {
                                control: (BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                                to: (BlockCoord::Frac(7, 8), BlockCoord::Frac(1, 2)),
                            },
                            PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                        ],
                        PolyStyle::OutlineHeavy,
                        BlendMode::default(),
                    );
                }
                if pattern.contains(Branch::LEFT_TO_DOWN) {
                    draw(
                        &[
                            PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                            PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(7, 8)),
                            PolyCommand::QuadTo {
                                control: (BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                                to: (BlockCoord::Frac(1, 8), BlockCoord::Frac(1, 2)),
                            },
                            PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                        ],
                        PolyStyle::OutlineHeavy,
                        BlendMode::default(),
                    );
                }
                if pattern.contains(Branch::RIGHT_TO_UP) {
                    draw(
                        &[
                            PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                            PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 8)),
                            PolyCommand::QuadTo {
                                control: (BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                                to: (BlockCoord::Frac(7, 8), BlockCoord::Frac(1, 2)),
                            },
                            PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                        ],
                        PolyStyle::OutlineHeavy,
                        BlendMode::default(),
                    );
                }
                if pattern.contains(Branch::LEFT_TO_UP) {
                    draw(
                        &[
                            PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                            PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 8)),
                            PolyCommand::QuadTo {
                                control: (BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                                to: (BlockCoord::Frac(1, 8), BlockCoord::Frac(1, 2)),
                            },
                            PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                        ],
                        PolyStyle::OutlineHeavy,
                        BlendMode::default(),
                    );
                }
                if pattern.contains(Branch::LEFT) {
                    draw(
                        &[
                            PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                            PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                        ],
                        PolyStyle::OutlineHeavy,
                        BlendMode::default(),
                    );
                }
                if pattern.contains(Branch::RIGHT) {
                    draw(
                        &[
                            PolyCommand::MoveTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                            PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                        ],
                        PolyStyle::OutlineHeavy,
                        BlendMode::default(),
                    );
                }
                if pattern.contains(Branch::UP) {
                    draw(
                        &[
                            PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                            PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                        ],
                        PolyStyle::OutlineHeavy,
                        BlendMode::default(),
                    );
                }
                if pattern.contains(Branch::DOWN) {
                    draw(
                        &[
                            PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                            PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                        ],
                        PolyStyle::OutlineHeavy,
                        BlendMode::default(),
                    );
                }
                if pattern.contains(Branch::CIRCLE_FILLED) {
                    draw(
                        &[PolyCommand::Circle {
                            center: (BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                            radius: BlockCoord::Frac(2, 5),
                        }],
                        PolyStyle::Fill,
                        BlendMode::default(),
                    );
                }
                if pattern.contains(Branch::CIRCLE_OUTLINE) {
                    draw(
                        &[PolyCommand::Circle {
                            center: (BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                            radius: BlockCoord::Frac(2, 5),
                        }],
                        PolyStyle::Fill,
                        BlendMode::default(),
                    );
                    draw(
                        &[PolyCommand::Circle {
                            center: (BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                            radius: BlockCoord::Frac(3, 10),
                        }],
                        PolyStyle::Fill,
                        BlendMode::Clear,
                    );
                }
            }
            BlockKey::Spinner(segment) => {
                let mut draw =
                    |cmd: &'static [PolyCommand], style: PolyStyle, blend_mode: BlendMode| {
                        self.draw_polys(
                            &metrics,
                            &[Poly {
                                path: cmd,
                                intensity: BlockAlpha::Full,
                                style,
                            }],
                            &mut buffer,
                            if config::configuration().anti_alias_custom_block_glyphs {
                                PolyAA::AntiAlias
                            } else {
                                PolyAA::MoarPixels
                            },
                            blend_mode,
                        );
                    };

                match segment {
                    0 => {
                        draw(
                            &[PolyCommand::Circle {
                                center: (BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                                radius: BlockCoord::Frac(1, 2),
                            }],
                            PolyStyle::Fill,
                            BlendMode::default(),
                        );
                        draw(
                            &[PolyCommand::Circle {
                                center: (BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                                radius: BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-3)),
                            }],
                            PolyStyle::Fill,
                            BlendMode::Clear,
                        );
                        draw(
                            &[
                                PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                                PolyCommand::LineTo(BlockCoord::SquareOne, BlockCoord::SquareZero),
                                PolyCommand::LineTo(BlockCoord::One, BlockCoord::One),
                                PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::One),
                                PolyCommand::LineTo(BlockCoord::SquareZero, BlockCoord::SquareZero),
                                PolyCommand::Close,
                            ],
                            PolyStyle::Fill,
                            BlendMode::Clear,
                        );
                    }
                    1 => {
                        draw(
                            &[PolyCommand::Circle {
                                center: (BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                                radius: BlockCoord::Frac(1, 2),
                            }],
                            PolyStyle::Fill,
                            BlendMode::default(),
                        );
                        draw(
                            &[PolyCommand::Circle {
                                center: (BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                                radius: BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-3)),
                            }],
                            PolyStyle::Fill,
                            BlendMode::Clear,
                        );
                        draw(
                            &[
                                PolyCommand::MoveTo(
                                    BlockCoord::SquareFrac(1, 2),
                                    BlockCoord::SquareFrac(1, 2),
                                ),
                                PolyCommand::LineTo(BlockCoord::SquareFrac(1, 2), BlockCoord::Zero),
                                PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::Zero),
                                PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::One),
                                PolyCommand::LineTo(BlockCoord::SquareOne, BlockCoord::One),
                                PolyCommand::LineTo(BlockCoord::SquareOne, BlockCoord::SquareOne),
                                PolyCommand::Close,
                            ],
                            PolyStyle::Fill,
                            BlendMode::Clear,
                        );
                    }
                    2 => {
                        draw(
                            &[PolyCommand::Circle {
                                center: (BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                                radius: BlockCoord::Frac(1, 2),
                            }],
                            PolyStyle::Fill,
                            BlendMode::default(),
                        );
                        draw(
                            &[PolyCommand::Circle {
                                center: (BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                                radius: BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-3)),
                            }],
                            PolyStyle::Fill,
                            BlendMode::Clear,
                        );
                        draw(
                            &[
                                PolyCommand::MoveTo(
                                    BlockCoord::SquareFrac(1, 2),
                                    BlockCoord::SquareFrac(1, 2),
                                ),
                                PolyCommand::LineTo(BlockCoord::SquareOne, BlockCoord::SquareZero),
                                PolyCommand::LineTo(BlockCoord::One, BlockCoord::Zero),
                                PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::Zero),
                                PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::One),
                                PolyCommand::LineTo(
                                    BlockCoord::SquareFrac(1, 3),
                                    BlockCoord::SquareOne,
                                ),
                            ],
                            PolyStyle::Fill,
                            BlendMode::Clear,
                        );
                    }
                    3 => {
                        draw(
                            &[PolyCommand::Circle {
                                center: (BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                                radius: BlockCoord::Frac(1, 2),
                            }],
                            PolyStyle::Fill,
                            BlendMode::default(),
                        );
                        draw(
                            &[PolyCommand::Circle {
                                center: (BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                                radius: BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-3)),
                            }],
                            PolyStyle::Fill,
                            BlendMode::Clear,
                        );
                        draw(
                            &[
                                PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::SquareFrac(1, 2)),
                                PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::Zero),
                                PolyCommand::LineTo(BlockCoord::One, BlockCoord::Zero),
                                PolyCommand::LineTo(BlockCoord::One, BlockCoord::SquareFrac(1, 2)),
                            ],
                            PolyStyle::Fill,
                            BlendMode::Clear,
                        );
                    }
                    4 => {
                        draw(
                            &[PolyCommand::Circle {
                                center: (BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                                radius: BlockCoord::Frac(1, 2),
                            }],
                            PolyStyle::Fill,
                            BlendMode::default(),
                        );
                        draw(
                            &[PolyCommand::Circle {
                                center: (BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                                radius: BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-3)),
                            }],
                            PolyStyle::Fill,
                            BlendMode::Clear,
                        );
                        draw(
                            &[
                                PolyCommand::MoveTo(
                                    BlockCoord::SquareFrac(1, 2),
                                    BlockCoord::SquareFrac(1, 2),
                                ),
                                PolyCommand::LineTo(BlockCoord::SquareZero, BlockCoord::SquareZero),
                                PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::Zero),
                                PolyCommand::LineTo(BlockCoord::One, BlockCoord::Zero),
                                PolyCommand::LineTo(BlockCoord::One, BlockCoord::One),
                                PolyCommand::LineTo(
                                    BlockCoord::SquareFrac(2, 3),
                                    BlockCoord::SquareOne,
                                ),
                            ],
                            PolyStyle::Fill,
                            BlendMode::Clear,
                        );
                    }
                    5 => {
                        draw(
                            &[PolyCommand::Circle {
                                center: (BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                                radius: BlockCoord::Frac(1, 2),
                            }],
                            PolyStyle::Fill,
                            BlendMode::default(),
                        );
                        draw(
                            &[PolyCommand::Circle {
                                center: (BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                                radius: BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-3)),
                            }],
                            PolyStyle::Fill,
                            BlendMode::Clear,
                        );
                        draw(
                            &[
                                PolyCommand::MoveTo(
                                    BlockCoord::SquareFrac(1, 2),
                                    BlockCoord::SquareFrac(1, 2),
                                ),
                                PolyCommand::LineTo(BlockCoord::SquareFrac(1, 2), BlockCoord::Zero),
                                PolyCommand::LineTo(BlockCoord::One, BlockCoord::Zero),
                                PolyCommand::LineTo(BlockCoord::One, BlockCoord::One),
                                PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::One),
                                PolyCommand::LineTo(BlockCoord::SquareZero, BlockCoord::SquareOne),
                                PolyCommand::Close,
                            ],
                            PolyStyle::Fill,
                            BlendMode::Clear,
                        );
                    }
                    _ => {}
                }
            }
            BlockKey::Poly(polys) | BlockKey::PolyWithCustomMetrics { polys, .. } => {
                self.draw_polys(
                    &metrics,
                    polys,
                    &mut buffer,
                    if config::configuration().anti_alias_custom_block_glyphs {
                        PolyAA::AntiAlias
                    } else {
                        PolyAA::MoarPixels
                    },
                    BlendMode::default(),
                );
            }
        }

        /*
        log::info!("{:?}", block);
        buffer.log_bits();
        */

        let sprite = self.atlas.allocate(&buffer)?;
        self.block_glyphs.insert(key, sprite.clone());
        Ok(sprite)
    }
}

// Fill a rectangular region described by the x and y ranges
fn fill_rect(buffer: &mut Image, x: Range<f32>, y: Range<f32>, intensity: BlockAlpha) {
    let (width, height) = buffer.image_dimensions();
    let mut pixmap =
        PixmapMut::from_bytes(buffer.pixel_data_slice_mut(), width as u32, height as u32)
            .expect("make pixmap from existing bitmap");

    let path = PathBuilder::from_rect(
        tiny_skia::Rect::from_xywh(x.start, y.start, x.end - x.start, y.end - y.start)
            .expect("valid rect"),
    );

    let mut paint = Paint::default();
    let intensity = intensity.to_scale();
    paint.set_color(
        tiny_skia::Color::from_rgba(intensity, intensity, intensity, intensity).unwrap(),
    );
    paint.anti_alias = false;
    paint.force_hq_pipeline = true;

    pixmap.fill_path(
        &path,
        &paint,
        FillRule::Winding,
        Transform::identity(),
        None,
    );
}
