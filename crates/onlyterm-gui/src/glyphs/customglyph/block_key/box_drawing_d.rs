use super::super::types::*;

pub(super) fn lookup(c: u32) -> Option<BlockKey> {
    Some(match c {
        // [╟] BOX DRAWINGS VERTICAL DOUBLE AND RIGHT SINGLE
        0x255f => BlockKey::Poly(&[
            Poly {
                path: &[
                    PolyCommand::MoveTo(
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        BlockCoord::Zero,
                    ),
                    PolyCommand::LineTo(
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        BlockCoord::One,
                    ),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            },
            Poly {
                path: &[
                    PolyCommand::MoveTo(
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        BlockCoord::Zero,
                    ),
                    PolyCommand::LineTo(
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        BlockCoord::Frac(1, 2),
                    ),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                    PolyCommand::LineTo(
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        BlockCoord::Frac(1, 2),
                    ),
                    PolyCommand::LineTo(
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        BlockCoord::One,
                    ),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            },
        ]),

        // [╠] BOX DRAWINGS DOUBLE VERTICAL AND RIGHT
        0x2560 => BlockKey::Poly(&[
            Poly {
                path: &[
                    PolyCommand::MoveTo(
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        BlockCoord::Zero,
                    ),
                    PolyCommand::LineTo(
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        BlockCoord::One,
                    ),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            },
            Poly {
                path: &[
                    PolyCommand::MoveTo(
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        BlockCoord::Zero,
                    ),
                    PolyCommand::LineTo(
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                    ),
                    PolyCommand::LineTo(
                        BlockCoord::One,
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                    ),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            },
            Poly {
                path: &[
                    PolyCommand::MoveTo(
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        BlockCoord::One,
                    ),
                    PolyCommand::LineTo(
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                    ),
                    PolyCommand::LineTo(
                        BlockCoord::One,
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                    ),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            },
        ]),
        // [╡] BOX DRAWINGS VERTICAL SINGLE AND LEFT DOUBLE
        0x2561 => BlockKey::Poly(&[
            Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                    PolyCommand::LineTo(
                        BlockCoord::Frac(1, 2),
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                    ),
                    PolyCommand::LineTo(
                        BlockCoord::Zero,
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                    ),
                    PolyCommand::LineTo(
                        BlockCoord::Frac(1, 2),
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                    ),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            },
            Poly {
                path: &[
                    PolyCommand::MoveTo(
                        BlockCoord::Frac(1, 2),
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                    ),
                    PolyCommand::LineTo(
                        BlockCoord::Zero,
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                    ),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            },
        ]),
        // [╢] BOX DRAWINGS VERTICAL DOUBLE AND LEFT SINGLE
        0x2562 => BlockKey::Poly(&[
            Poly {
                path: &[
                    PolyCommand::MoveTo(
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        BlockCoord::Zero,
                    ),
                    PolyCommand::LineTo(
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        BlockCoord::One,
                    ),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            },
            Poly {
                path: &[
                    PolyCommand::MoveTo(
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        BlockCoord::Zero,
                    ),
                    PolyCommand::LineTo(
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        BlockCoord::Frac(1, 2),
                    ),
                    PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                    PolyCommand::LineTo(
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        BlockCoord::Frac(1, 2),
                    ),
                    PolyCommand::LineTo(
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        BlockCoord::One,
                    ),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            },
        ]),
        // [╣] BOX DRAWINGS DOUBLE VERTICAL AND LEFT
        0x2563 => BlockKey::Poly(&[
            Poly {
                path: &[
                    PolyCommand::MoveTo(
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        BlockCoord::Zero,
                    ),
                    PolyCommand::LineTo(
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        BlockCoord::One,
                    ),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            },
            Poly {
                path: &[
                    PolyCommand::MoveTo(
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        BlockCoord::Zero,
                    ),
                    PolyCommand::LineTo(
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                    ),
                    PolyCommand::LineTo(
                        BlockCoord::Zero,
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                    ),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            },
            Poly {
                path: &[
                    PolyCommand::MoveTo(
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        BlockCoord::One,
                    ),
                    PolyCommand::LineTo(
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                    ),
                    PolyCommand::LineTo(
                        BlockCoord::Zero,
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                    ),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            },
        ]),
        // [╤] BOX DRAWINGS DOWN SINGLE AND HORIZONTAL DOUBLE
        0x2564 => BlockKey::Poly(&[
            Poly {
                path: &[
                    PolyCommand::MoveTo(
                        BlockCoord::Zero,
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                    ),
                    PolyCommand::LineTo(
                        BlockCoord::One,
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                    ),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            },
            Poly {
                path: &[
                    PolyCommand::MoveTo(
                        BlockCoord::Zero,
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                    ),
                    PolyCommand::LineTo(
                        BlockCoord::Frac(1, 2),
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                    ),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                    PolyCommand::LineTo(
                        BlockCoord::Frac(1, 2),
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                    ),
                    PolyCommand::LineTo(
                        BlockCoord::One,
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                    ),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            },
        ]),
        // [╥] BOX DRAWINGS DOWN DOUBLE AND HORIZONTAL SINGLE
        0x2565 => BlockKey::Poly(&[
            Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            },
            Poly {
                path: &[
                    PolyCommand::MoveTo(
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        BlockCoord::Frac(1, 2),
                    ),
                    PolyCommand::LineTo(
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        BlockCoord::One,
                    ),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            },
            Poly {
                path: &[
                    PolyCommand::MoveTo(
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        BlockCoord::Frac(1, 2),
                    ),
                    PolyCommand::LineTo(
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        BlockCoord::One,
                    ),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            },
        ]),
        // [╦] BOX DRAWINGS DOUBLE DOWN AND HORIZONTAL
        0x2566 => BlockKey::Poly(&[
            Poly {
                path: &[
                    PolyCommand::MoveTo(
                        BlockCoord::Zero,
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                    ),
                    PolyCommand::LineTo(
                        BlockCoord::One,
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                    ),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            },
            Poly {
                path: &[
                    PolyCommand::MoveTo(
                        BlockCoord::Zero,
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                    ),
                    PolyCommand::LineTo(
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                    ),
                    PolyCommand::LineTo(
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        BlockCoord::One,
                    ),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            },
            Poly {
                path: &[
                    PolyCommand::MoveTo(
                        BlockCoord::One,
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                    ),
                    PolyCommand::LineTo(
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                    ),
                    PolyCommand::LineTo(
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        BlockCoord::One,
                    ),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            },
        ]),
        // [╧] BOX DRAWINGS UP SINGLE AND HORIZONTAL DOUBLE
        0x2567 => BlockKey::Poly(&[
            Poly {
                path: &[
                    PolyCommand::MoveTo(
                        BlockCoord::Zero,
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                    ),
                    PolyCommand::LineTo(
                        BlockCoord::One,
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                    ),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            },
            Poly {
                path: &[
                    PolyCommand::MoveTo(
                        BlockCoord::Zero,
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                    ),
                    PolyCommand::LineTo(
                        BlockCoord::Frac(1, 2),
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                    ),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                    PolyCommand::LineTo(
                        BlockCoord::Frac(1, 2),
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                    ),
                    PolyCommand::LineTo(
                        BlockCoord::One,
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                    ),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            },
        ]),
        // [╨] BOX DRAWINGS UP DOUBLE AND HORIZONTAL SINGLE
        0x2568 => BlockKey::Poly(&[
            Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            },
            Poly {
                path: &[
                    PolyCommand::MoveTo(
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        BlockCoord::Frac(1, 2),
                    ),
                    PolyCommand::LineTo(
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        BlockCoord::Zero,
                    ),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            },
            Poly {
                path: &[
                    PolyCommand::MoveTo(
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        BlockCoord::Frac(1, 2),
                    ),
                    PolyCommand::LineTo(
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        BlockCoord::Zero,
                    ),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            },
        ]),
        // [╩] BOX DRAWINGS DOUBLE UP AND HORIZONTAL
        0x2569 => BlockKey::Poly(&[
            Poly {
                path: &[
                    PolyCommand::MoveTo(
                        BlockCoord::Zero,
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                    ),
                    PolyCommand::LineTo(
                        BlockCoord::One,
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                    ),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            },
            Poly {
                path: &[
                    PolyCommand::MoveTo(
                        BlockCoord::Zero,
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                    ),
                    PolyCommand::LineTo(
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                    ),
                    PolyCommand::LineTo(
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        BlockCoord::Zero,
                    ),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            },
            Poly {
                path: &[
                    PolyCommand::MoveTo(
                        BlockCoord::One,
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                    ),
                    PolyCommand::LineTo(
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                    ),
                    PolyCommand::LineTo(
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        BlockCoord::Zero,
                    ),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            },
        ]),
        // [╪] BOX DRAWINGS VERTICAL SINGLE AND HORIZONTAL DOUBLE
        0x256a => BlockKey::Poly(&[
            Poly {
                path: &[
                    PolyCommand::MoveTo(
                        BlockCoord::Zero,
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                    ),
                    PolyCommand::LineTo(
                        BlockCoord::One,
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                    ),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            },
            Poly {
                path: &[
                    PolyCommand::MoveTo(
                        BlockCoord::Zero,
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                    ),
                    PolyCommand::LineTo(
                        BlockCoord::One,
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                    ),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            },
            Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            },
        ]),
        // [╫] BOX DRAWINGS VERTICAL DOUBLE AND HORIZONTAL SINGLE
        0x256b => BlockKey::Poly(&[
            Poly {
                path: &[
                    PolyCommand::MoveTo(
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        BlockCoord::Zero,
                    ),
                    PolyCommand::LineTo(
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        BlockCoord::One,
                    ),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            },
            Poly {
                path: &[
                    PolyCommand::MoveTo(
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        BlockCoord::Zero,
                    ),
                    PolyCommand::LineTo(
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        BlockCoord::One,
                    ),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            },
            Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            },
        ]),

        // [╬] BOX DRAWINGS DOUBLE VERTICAL AND HORIZONTAL
        0x256c => BlockKey::Poly(&[
            Poly {
                path: &[
                    PolyCommand::MoveTo(
                        BlockCoord::Zero,
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                    ),
                    PolyCommand::LineTo(
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                    ),
                    PolyCommand::LineTo(
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        BlockCoord::Zero,
                    ),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            },
            Poly {
                path: &[
                    PolyCommand::MoveTo(
                        BlockCoord::One,
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                    ),
                    PolyCommand::LineTo(
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                    ),
                    PolyCommand::LineTo(
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        BlockCoord::Zero,
                    ),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            },
            Poly {
                path: &[
                    PolyCommand::MoveTo(
                        BlockCoord::Zero,
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                    ),
                    PolyCommand::LineTo(
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                    ),
                    PolyCommand::LineTo(
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        BlockCoord::One,
                    ),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            },
            Poly {
                path: &[
                    PolyCommand::MoveTo(
                        BlockCoord::One,
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                    ),
                    PolyCommand::LineTo(
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                    ),
                    PolyCommand::LineTo(
                        BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        BlockCoord::One,
                    ),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            },
        ]),

        // [╭] BOX DRAWINGS LIGHT ARC DOWN AND RIGHT
        0x256d => BlockKey::Poly(&[Poly {
            path: &[
                PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(3, 4)),
                PolyCommand::QuadTo {
                    control: (BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                    to: (BlockCoord::Frac(3, 4), BlockCoord::Frac(1, 2)),
                },
                PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
            ],
            intensity: BlockAlpha::Full,
            style: PolyStyle::Outline,
        }]),
        // [╮] BOX DRAWINGS LIGHT ARC DOWN AND LEFT
        0x256e => BlockKey::Poly(&[Poly {
            path: &[
                PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(3, 4)),
                PolyCommand::QuadTo {
                    control: (BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                    to: (BlockCoord::Frac(1, 4), BlockCoord::Frac(1, 2)),
                },
                PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
            ],
            intensity: BlockAlpha::Full,
            style: PolyStyle::Outline,
        }]),
        // [╯] BOX DRAWINGS LIGHT ARC UP AND LEFT
        0x256f => BlockKey::Poly(&[Poly {
            path: &[
                PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 4)),
                PolyCommand::QuadTo {
                    control: (BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                    to: (BlockCoord::Frac(1, 4), BlockCoord::Frac(1, 2)),
                },
                PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
            ],
            intensity: BlockAlpha::Full,
            style: PolyStyle::Outline,
        }]),
        // [╰] BOX DRAWINGS LIGHT ARC UP AND RIGHT
        0x2570 => BlockKey::Poly(&[Poly {
            path: &[
                PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 4)),
                PolyCommand::QuadTo {
                    control: (BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                    to: (BlockCoord::Frac(3, 4), BlockCoord::Frac(1, 2)),
                },
                PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
            ],
            intensity: BlockAlpha::Full,
            style: PolyStyle::Outline,
        }]),

        // [╱] BOX DRAWINGS LIGHT DIAGONAL UPPER RIGHT TO LOWER LEFT
        0x2571 => BlockKey::Poly(&[Poly {
            path: &[
                PolyCommand::MoveTo(BlockCoord::One, BlockCoord::Zero),
                PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::One),
            ],
            intensity: BlockAlpha::Full,
            style: PolyStyle::Outline,
        }]),
        // [╲] BOX DRAWINGS LIGHT DIAGONAL UPPER LEFT TO LOWER RIGHT
        0x2572 => BlockKey::Poly(&[Poly {
            path: &[
                PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Zero),
                PolyCommand::LineTo(BlockCoord::One, BlockCoord::One),
            ],
            intensity: BlockAlpha::Full,
            style: PolyStyle::Outline,
        }]),
        // [╳] BOX DRAWINGS LIGHT DIAGONAL CROSS
        0x2573 => BlockKey::Poly(&[
            Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::One, BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::One),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            },
            Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::One),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            },
        ]),
        // [╴] BOX DRAWINGS LIGHT LEFT
        0x2574 => BlockKey::Poly(&[Poly {
            path: &[
                PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
            ],
            intensity: BlockAlpha::Full,
            style: PolyStyle::Outline,
        }]),
        // [╵] BOX DRAWINGS LIGHT UP
        0x2575 => BlockKey::Poly(&[Poly {
            path: &[
                PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
            ],
            intensity: BlockAlpha::Full,
            style: PolyStyle::Outline,
        }]),
        // [╶] BOX DRAWINGS LIGHT RIGHT
        0x2576 => BlockKey::Poly(&[Poly {
            path: &[
                PolyCommand::MoveTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
            ],
            intensity: BlockAlpha::Full,
            style: PolyStyle::Outline,
        }]),
        // [╷] BOX DRAWINGS LIGHT DOWN
        0x2577 => BlockKey::Poly(&[Poly {
            path: &[
                PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
            ],
            intensity: BlockAlpha::Full,
            style: PolyStyle::Outline,
        }]),
        // [╸] BOX DRAWINGS HEAVY LEFT
        0x2578 => BlockKey::Poly(&[Poly {
            path: &[
                PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
            ],
            intensity: BlockAlpha::Full,
            style: PolyStyle::OutlineHeavy,
        }]),
        // [╹] BOX DRAWINGS HEAVY UP
        0x2579 => BlockKey::Poly(&[Poly {
            path: &[
                PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
            ],
            intensity: BlockAlpha::Full,
            style: PolyStyle::OutlineHeavy,
        }]),
        // [╺] BOX DRAWINGS HEAVY RIGHT
        0x257a => BlockKey::Poly(&[Poly {
            path: &[
                PolyCommand::MoveTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
            ],
            intensity: BlockAlpha::Full,
            style: PolyStyle::OutlineHeavy,
        }]),
        // [╻] BOX DRAWINGS HEAVY DOWN
        0x257b => BlockKey::Poly(&[Poly {
            path: &[
                PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
            ],
            intensity: BlockAlpha::Full,
            style: PolyStyle::OutlineHeavy,
        }]),
        // [╼] BOX DRAWINGS LIGHT LEFT AND HEAVY RIGHT
        0x257c => BlockKey::Poly(&[
            Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            },
            Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::OutlineHeavy,
            },
        ]),
        // [╽] BOX DRAWINGS LIGHT UP AND HEAVY DOWN
        0x257d => BlockKey::Poly(&[
            Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            },
            Poly {
                path: &[
                    PolyCommand::MoveTo(
                        BlockCoord::Frac(1, 2),
                        BlockCoord::FracWithOffset(1, 2, LineScale::Div(-1)),
                    ),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::OutlineHeavy,
            },
        ]),
        // [╾] BOX DRAWINGS HEAVY LEFT AND LIGHT RIGHT
        0x257e => BlockKey::Poly(&[
            Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::OutlineHeavy,
            },
            Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            },
        ]),
        // [╿] BOX DRAWINGS HEAVY UP AND LIGHT DOWN
        0x257f => BlockKey::Poly(&[
            Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::OutlineHeavy,
            },
            Poly {
                path: &[
                    PolyCommand::MoveTo(
                        BlockCoord::Frac(1, 2),
                        BlockCoord::FracWithOffset(1, 2, LineScale::Div(-1)),
                    ),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            },
        ]),

        // [▀] UPPER HALF BLOCK
        _ => return None,
    })
}
